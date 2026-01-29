//! Fund splitting operations for distributing ETH across multiple addresses.
//! Supports equal and random distribution modes with transaction queue management.

use crate::config::Config;
use crate::ledger_dispatch;
use crate::ledger_transaction_manager::{LedgerTransactionManager, PendingTransaction, TransactionManagerConfig};
use crate::types::AccountInfo;
use crate::utils;
use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use anyhow::{anyhow, Result};
use rand::Rng;
use std::sync::Arc;
use tracing::{info, warn};

pub enum SplitMode {
    Random,
    Equal,
}

impl SplitMode {
    fn label(&self) -> &'static str {
        match self {
            SplitMode::Random => "SplitFundsRandom",
            SplitMode::Equal => "SplitFundsEqual",
        }
    }
}

/// Progress update for split preparation
#[derive(Debug, Clone)]
pub enum PrepareProgress {
    /// Checking pre-found addresses
    CheckingPreFound { current: usize, total: usize, found_empty: usize },
    /// Scanning for more empty addresses
    ScanningIndex { index: u32, found_empty: usize, needed: u32 },
    /// Building transactions
    BuildingTransactions { current: usize, total: usize },
    /// Complete
    Complete { total_transactions: usize },
}

async fn find_empty_receivers(
    provider: &Arc<Provider<Http>>,
    config: &Config,
    source_index: u32,
    needed: u32,
    start_index: u32,
    pre_found_empty_addresses: Option<Vec<crate::balance::BalanceScanRecord>>,
    progress_sender: Option<&tokio::sync::mpsc::UnboundedSender<PrepareProgress>>,
    use_native_ledger: bool,
) -> Result<Vec<AccountInfo>> {
    let chain_id = config.chain_id;
    let mut receivers = Vec::new();
    let mut scanned_indexes: std::collections::HashSet<u32> = std::collections::HashSet::new();

    // First, use any pre-found empty addresses (from funded address scanning)
    if let Some(ref pre_found) = pre_found_empty_addresses {
        let total = pre_found.len();
        info!("Using {} pre-found empty addresses from funded scanning", total);
        
        for (i, record) in pre_found.iter().enumerate() {
            // Track scanned indexes to avoid rescanning
            scanned_indexes.insert(record.index);
            
            if receivers.len() >= needed as usize {
                break;
            }
            // Skip the source address
            if record.index == source_index {
                continue;
            }
            
            // Send progress update
            if let Some(sender) = progress_sender {
                let _ = sender.send(PrepareProgress::CheckingPreFound {
                    current: i + 1,
                    total,
                    found_empty: receivers.len(),
                });
            }
            
            // Double-check the address is still empty (balance and nonce)
            let balance = provider.get_balance(record.address, None).await?;
            let nonce = provider.get_transaction_count(record.address, None).await?.as_u64();

            if balance.is_zero() && nonce == 0 {
                receivers.push(AccountInfo {
                    index: record.index,
                    address: record.address,
                    balance,
                    nonce,
                    derivation_path: record.derivation_path.clone(),
                });
            }
        }
    }

    // If we still need more, scan for additional addresses
    if receivers.len() < needed as usize {
        let mut current_idx = start_index;
        let max_scan = start_index + 200;

        info!("Scanning for {} empty receiver addresses starting from index {}... (already have {})",
              needed, start_index, receivers.len());

        while receivers.len() < needed as usize && current_idx < max_scan {
            // Skip source address
            if current_idx == source_index {
                current_idx += 1;
                continue;
            }
            
            // Skip already scanned indexes
            if scanned_indexes.contains(&current_idx) {
                current_idx += 1;
                continue;
            }

            // Send progress update
            if let Some(sender) = progress_sender {
                let _ = sender.send(PrepareProgress::ScanningIndex {
                    index: current_idx,
                    found_empty: receivers.len(),
                    needed,
                });
            }

            match ledger_dispatch::get_ledger_address_with_retry_config(use_native_ledger, chain_id, current_idx, Some(config)).await {
                Ok(addr) => {
                    let balance = provider.get_balance(addr, None).await?;
                    let nonce = provider.get_transaction_count(addr, None).await?.as_u64();

                    if balance.is_zero() && nonce == 0 {
                        receivers.push(AccountInfo {
                            index: current_idx,
                            address: addr,
                            balance,
                            nonce,
                            derivation_path: config.get_derivation_path(current_idx),
                        });
                        info!(
                            "Found empty receiver at index {}: {:?}",
                            current_idx, addr
                        );
                    }
                }
                Err(e) => {
                    warn!("Failed to scan index {}: {}", current_idx, e);
                }
            }

            current_idx += 1;
        }
    }

    Ok(receivers)
}

/// Prepare random split transactions
/// The remaining_balance_wei parameter specifies the exact amount to leave on the source address
fn prepare_random_transactions(
    receivers: &[AccountInfo],
    source_balance: U256,
    min_transfer_amount: U256,
    tx_fee: U256,
    _gas_reserve: U256, // Unused, we calculate dynamically
    gas_limit: u64,
    gas_price: U256,
    operation_name: &str,
    remaining_balance_wei: U256,
) -> Result<Vec<PendingTransaction>> {
    let mut transactions = Vec::new();
    let mut rng = rand::thread_rng();
    let total_receivers = receivers.len();
    let mut current_balance = source_balance;

    for (i, receiver) in receivers.iter().enumerate() {
        let remaining_receivers = total_receivers - i;
        let is_last = remaining_receivers == 1;
        
        // Reserve gas for ALL remaining transactions (including this one)
        let dynamic_gas_reserve = tx_fee * U256::from(remaining_receivers);
        
        // Calculate what we must keep: remaining_balance + gas for remaining transactions
        let must_keep = remaining_balance_wei + dynamic_gas_reserve;

        if current_balance <= must_keep + min_transfer_amount {
            info!("Balance depleted. Stopping transaction preparation.");
            break;
        }

        let available_to_send = current_balance - must_keep;
        
        let amount = if is_last {
            // Last transaction: send EXACTLY what's needed to leave remaining_balance
            // amount = current_balance - remaining_balance - tx_fee
            if current_balance > remaining_balance_wei + tx_fee + min_transfer_amount {
                current_balance - remaining_balance_wei - tx_fee
            } else {
                info!("Not enough for final transaction.");
                break;
            }
        } else {
            // Earlier transactions: limit to proportional share to ensure all receivers get funds
            let proportion = U256::from(remaining_receivers);
            let max_send = available_to_send / proportion;

            if max_send < min_transfer_amount {
                info!("Not enough remaining for next receiver.");
                break;
            }

            // Calculate random amount between min and max
            let range = max_send - min_transfer_amount;
            let range_u128: u128 = range.try_into().unwrap_or(u128::MAX);
            let ratio: f64 = rng.gen_range(0.0..1.0);
            let add_amount = (range_u128 as f64 * ratio) as u128;
            min_transfer_amount + U256::from(add_amount)
        };

        transactions.push(PendingTransaction {
            to: receiver.address,
            value: amount,
            gas_limit,
            gas_price,
            operation_name: format!("{}_to_{}", operation_name, receiver.index),
        });

        current_balance = current_balance - (amount + tx_fee);
    }

    Ok(transactions)
}

/// Prepare equal split transactions
/// The remaining_balance_wei parameter specifies the exact amount to leave on the source address
fn prepare_equal_transactions(
    receivers: &[AccountInfo],
    source_balance: U256,
    min_transfer_amount: U256,
    tx_fee: U256,
    gas_limit: u64,
    gas_price: U256,
    operation_name: &str,
    remaining_balance_wei: U256,
) -> Result<Vec<PendingTransaction>> {
    let receiver_count = U256::from(receivers.len());
    let total_estimated_fees = tx_fee * receiver_count;

    // Calculate distributable balance: source - remaining_balance - all fees
    if source_balance <= remaining_balance_wei + total_estimated_fees {
        return Err(anyhow!("Balance too low to cover fees and remaining balance for equal split."));
    }

    let distributable_balance = source_balance - remaining_balance_wei - total_estimated_fees;
    let amount_per_receiver = distributable_balance / receiver_count;

    if amount_per_receiver < min_transfer_amount {
        return Err(anyhow!(
            "Calculated equal share {} is less than minimum required {}.",
            utils::format_ether(amount_per_receiver),
            utils::format_ether(min_transfer_amount)
        ));
    }

    info!(
        "Preparing {} equal transactions of {} ETH each (leaving {} ETH on source)",
        receivers.len(),
        utils::format_ether(amount_per_receiver),
        utils::format_ether(remaining_balance_wei)
    );

    let transactions: Vec<PendingTransaction> = receivers
        .iter()
        .enumerate()
        .map(|(idx, receiver)| PendingTransaction {
            to: receiver.address,
            value: amount_per_receiver,
                    gas_limit,
                    gas_price,
            operation_name: format!("{}_equal_{}", operation_name, idx),
        })
        .collect();

    Ok(transactions)
}

/// Prepare split transactions without executing them
/// Returns the transactions and the transaction manager
pub async fn prepare_split_transactions(
    config: Config,
    output_count: u32,
    gas_speed_override: Option<f32>,
    mode: SplitMode,
    source_idx_override: Option<usize>,
    recipient_addresses: Option<Vec<String>>,
    pre_found_empty_addresses: Option<Vec<crate::balance::BalanceScanRecord>>,
    scan_start_index: u32,
    progress_sender: Option<tokio::sync::mpsc::UnboundedSender<PrepareProgress>>,
    remaining_balance: Option<u64>,
    use_native_ledger: bool,
) -> Result<(Vec<(PendingTransaction, String, String)>, Arc<LedgerTransactionManager>)> {
    let provider = config.get_provider().await?;
    let chain_id = config.chain_id;
    let operation_name = mode.label();

    // Get source address
    let source = if let Some(source_address_index) = source_idx_override {
        info!(
            "Using specified source address index: {}",
            source_address_index
        );

        let addr = ledger_dispatch::get_ledger_address_with_retry_config(
            use_native_ledger,
            chain_id, 
            source_address_index as u32, 
            Some(&config)
        ).await?;
        let balance = provider.get_balance(addr, None).await?;
        let nonce = provider.get_transaction_count(addr, None).await?.as_u64();

        if balance.is_zero() {
            return Err(anyhow!(
                "Source address at index {} has zero balance.",
                source_address_index
            ));
        }

        AccountInfo {
            index: source_address_index as u32,
            address: addr,
            balance,
            nonce,
            derivation_path: config.get_derivation_path(source_address_index as u32),
        }
    } else {
        return Err(anyhow!(
            "Source address must be specified. Use source_idx_override."
        ));
    };

    info!(
        "Source: Index {} ({:?}): {} ETH",
        source.index,
        source.address,
        utils::format_ether(source.balance)
    );

    // Find empty receivers or use provided addresses
    let receivers = if let Some(ref addresses) = recipient_addresses {
        // Use provided addresses
        let mut receivers = Vec::new();
        for addr_str in addresses {
            let addr: Address = addr_str.parse().map_err(|_| anyhow!("Invalid address: {}", addr_str))?;
            let balance = provider.get_balance(addr, None).await?;
            let nonce = provider.get_transaction_count(addr, None).await?.as_u64();

            receivers.push(AccountInfo {
                index: 0, // Not from hardware wallet, so index is irrelevant
                address: addr,
                balance,
                nonce,
                derivation_path: format!("external:{}", addr_str),
            });
        }
        receivers
    } else {
        // Find empty receivers from hardware wallet, using pre-found empty addresses if available
        find_empty_receivers(&provider, &config, source.index, output_count, scan_start_index, pre_found_empty_addresses, progress_sender.as_ref(), use_native_ledger).await?
    };

    if receivers.len() < output_count as usize {
        let has_recipient_addresses = recipient_addresses.is_some();
        let error_msg = if has_recipient_addresses {
            format!("Invalid recipient addresses provided. Expected {}, got {}.", output_count, receivers.len())
        } else {
            format!("Could not find enough empty addresses. Found {}, needed {}.", receivers.len(), output_count)
        };
        return Err(anyhow!(error_msg));
    }

    // Get gas price
    let gas_price = provider.get_gas_price().await?;
    info!(
        "Current Gas Price: {} Gwei",
        ethers::utils::format_units(gas_price, "gwei")?
    );

    // Calculate transaction parameters
    let gas_limit = 21000u64;
    
    // Apply gas speed multiplier to gas price for faster/more reliable transactions
    let gas_speed = gas_speed_override.unwrap_or(config.gas_speed_multiplier);
    // Convert f32 multiplier to integer basis points for U256 math (e.g., 1.5 -> 150)
    let gas_speed_bp = (gas_speed * 100.0) as u64;
    let effective_gas_price = gas_price * U256::from(gas_speed_bp) / U256::from(100u64);
    let tx_fee = effective_gas_price * U256::from(gas_limit);

    // Minimum transfer amount is 5x the base transaction fee (to prevent dust)
    let base_tx_fee = gas_price * U256::from(gas_limit);
    let min_transfer_amount = base_tx_fee * U256::from(5u64);

    info!(
        "Gas price: {} Gwei ({:.1}x speed), Min transfer: {} ETH",
        ethers::utils::format_units(effective_gas_price, "gwei").unwrap_or_default(),
        gas_speed,
        utils::format_ether(min_transfer_amount)
    );

    // Initialize transaction manager
    let manager_config = TransactionManagerConfig {
        inter_transaction_delay_ms: 3000,
        max_retries: 2,
        retry_delay_ms: 2000,
        wait_for_confirmation: true,
        confirmation_timeout_secs: 90,
        derivation_mode: config.derivation_mode,
        custom_account: config.custom_account,
        custom_address_index: config.custom_address_index,
        coin_type: config.coin_type,
        use_native_ledger,
    };

    let manager = Arc::new(LedgerTransactionManager::new(
        provider.clone(),
        manager_config,
        chain_id,
        source.address,
        source.index,
        config.rpc_url.clone(),
    )
    .await?);

    // Initialize manager (fetches current nonce)
    manager.initialize().await?;

    let source_balance = source.balance;

    // Calculate remaining balance in wei
    let remaining_balance_wei = remaining_balance
        .map(|r| U256::from(r))
        .unwrap_or(U256::zero());

    // Validate remaining balance
    if remaining_balance_wei >= source_balance {
        return Err(anyhow!("Remaining balance ({}) is greater than or equal to available balance ({}).",
            ethers::utils::format_ether(remaining_balance_wei),
            ethers::utils::format_ether(source_balance)));
    }

    // Calculate total gas needed for all transactions
    let total_gas_needed = tx_fee * U256::from(output_count);
    let available_to_distribute = source_balance - remaining_balance_wei - total_gas_needed;

    if available_to_distribute < min_transfer_amount * U256::from(output_count) {
        return Err(anyhow!("Balance too low to split meaningfully after fees and remaining balance. Available: {}, needed: {}",
            utils::format_ether(available_to_distribute),
            utils::format_ether(min_transfer_amount * U256::from(output_count))));
    }

    let gas_reserve = tx_fee * U256::from(10);

    // Prepare transactions based on mode
    let transactions = match mode {
        SplitMode::Random => {
            prepare_random_transactions(
                &receivers[..output_count as usize],
                source_balance,
                min_transfer_amount,
                tx_fee,
                gas_reserve,
                gas_limit,
                effective_gas_price,
                operation_name,
                remaining_balance_wei,
            )?
        }
        SplitMode::Equal => {
            prepare_equal_transactions(
                &receivers[..output_count as usize],
                source_balance,
                min_transfer_amount,
                tx_fee,
                gas_limit,
                effective_gas_price,
                operation_name,
                remaining_balance_wei,
            )?
        }
    };

    if transactions.is_empty() {
        return Err(anyhow!("No valid transactions to execute."));
    }

    // Create transaction list with descriptions and labels
    let mut tx_list = Vec::new();
    for (idx, tx) in transactions.into_iter().enumerate() {
        let receiver = &receivers[idx];
        let description = format!(
            "{} #{}: {} ETH",
            operation_name,
            idx + 1,
            utils::format_ether(tx.value)
        );
        let dest_label = format!("{} → {:?}", receiver.derivation_path, receiver.address);
        tx_list.push((tx, description, dest_label));
    }

    Ok((tx_list, manager))
}