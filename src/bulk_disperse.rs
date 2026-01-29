//! Bulk disperse operations using smart contracts for efficient batch transfers.
//! Uses the Beaug contract to distribute funds to multiple recipients in a single transaction.

use crate::config::Config;
use crate::disperse;
use crate::ledger_dispatch;
use crate::types::AccountInfo;
use crate::{operation_log, utils};
use ethers::prelude::*;
use ethers::abi::{Function, Token, Param, ParamType, StateMutability};
use anyhow::{anyhow, Result};
use tracing::info;

/// Beaug contract function signature: beaugDisperse(address[] recipients, uint256[] amounts)
/// This is the single unified disperse function - GUI calculates amounts for equal distribution
#[allow(deprecated)]
fn get_beaug_disperse_function() -> Function {
    // function beaugDisperse(address[] calldata recipients, uint256[] calldata amounts) external payable
    Function {
        name: "beaugDisperse".to_string(),
        inputs: vec![
            Param {
                name: "recipients".to_string(),
                kind: ParamType::Array(Box::new(ParamType::Address)),
                internal_type: None,
            },
            Param {
                name: "amounts".to_string(),
                kind: ParamType::Array(Box::new(ParamType::Uint(256))),
                internal_type: None,
            },
        ],
        outputs: vec![],
        constant: None,
        state_mutability: StateMutability::Payable,
    }
}


/// Types of bulk disperse operations
#[derive(Debug, Clone)]
pub enum BulkDisperseType {
    /// Equal distribution - just addresses, total amount split evenly
    Equal(Vec<Address>),
    /// Mixed distribution - addresses with specific amounts
    Mixed(Vec<(Address, U256)>),
}

/// Parse a string input that can be either:
/// - Just addresses (for equal distribution)
/// - Addresses with amounts (for mixed distribution)
pub fn parse_bulk_disperse_input(input: &str) -> Result<BulkDisperseType> {
    let lines: Vec<&str> = input.lines()
        .filter(|line| !line.trim().is_empty())
        .collect();

    if lines.is_empty() {
        return Ok(BulkDisperseType::Equal(vec![]));
    }

    // Check the first line to determine the format
    let first_line = lines[0];
    let parts: Vec<&str> = if first_line.contains(',') {
        first_line.split(',').collect()
    } else {
        first_line.split_whitespace().collect()
    };

    match parts.len() {
        1 => {
            // Address only format - parse all lines as addresses
            let mut addresses = Vec::new();
            for (line_num, line) in lines.iter().enumerate() {
                let trimmed = line.trim();
                let addr_parts: Vec<&str> = if trimmed.contains(',') {
                    trimmed.split(',').collect()
                } else {
                    trimmed.split_whitespace().collect()
                };

                if addr_parts.len() != 1 {
                    return Err(anyhow!(
                        "Line {}: Expected only address for equal distribution, got {} parts",
                        line_num + 1, addr_parts.len()
                    ));
                }

                let address: Address = addr_parts[0].trim().parse().map_err(|_| {
                    anyhow!("Line {}: Invalid address format: {}", line_num + 1, addr_parts[0])
                })?;
                addresses.push(address);
            }
            Ok(BulkDisperseType::Equal(addresses))
        }
        2 => {
            // Address and amount format - parse all lines as address,amount pairs
            let mut recipients = Vec::new();
            for (line_num, line) in lines.iter().enumerate() {
                let parts: Vec<&str> = if line.contains(',') {
                    line.split(',').collect()
                } else {
                    line.split_whitespace().collect()
                };

                if parts.len() != 2 {
                    return Err(anyhow!(
                        "Line {}: Invalid format. Expected `address,amount_in_eth` or `address amount_in_eth`",
                        line_num + 1
                    ));
                }

                let address: Address = parts[0].trim().parse().map_err(|_| {
                    anyhow!("Line {}: Invalid address format: {}", line_num + 1, parts[0])
                })?;

                // Convert ETH string directly to Wei (avoids f64 precision issues)
                let amount_wei = utils::parse_eth_str_to_wei(parts[1].trim()).map_err(|e| {
                    anyhow!("Line {}: Invalid amount '{}': {}", line_num + 1, parts[1].trim(), e)
                })?;
                recipients.push((address, amount_wei));
            }
            Ok(BulkDisperseType::Mixed(recipients))
        }
        _ => {
            Err(anyhow!(
                "Invalid format. Expected either addresses only (one per line) or address,amount pairs"
            ))
        }
    }
}

/// Execute bulk disperse using the Disperse contract (single transaction)
/// 
/// Tips are handled as regular recipients - if tip_amount and tip_recipient are provided,
/// they are appended to the recipients/amounts arrays and included in the transaction.
pub async fn bulk_disperse(
    config: Config,
    disperse_type: BulkDisperseType,
    amount_to_send: U256,
    disperse_address_override: Option<String>,
    source_idx_override: Option<usize>,
    tip_amount: Option<U256>,
    tip_recipient: Option<Address>,
    gas_speed: f32,
    use_native_ledger: bool,
) -> Result<()> {
    let provider = config.get_provider().await?;
    let chain_id = config.chain_id;
    let operation_name = "Beaug Bulk Disperse";
    
    // Tips are now handled as regular recipients (added to arrays below)
    // Validate and extract tip info early to avoid multiple unwraps later
    let tip = tip_amount.unwrap_or(U256::zero());
    let verified_tip_recipient: Option<Address> = if !tip.is_zero() {
        let recipient = tip_recipient.ok_or_else(|| {
            anyhow!("Tip amount specified but no tip recipient address provided")
        })?;
        info!("Including tip of {} ETH to {:?}", utils::format_ether(tip), recipient);
        Some(recipient)
    } else {
        None
    };

    // Resolve disperse contract address
    let disperse_address: Address = if let Some(addr_str) = disperse_address_override {
        addr_str
            .parse()
            .map_err(|_| anyhow!("Invalid disperse contract address"))?
    } else {
        disperse::get_disperse_address(chain_id)
            .ok_or_else(|| anyhow!(
                "No known Disperse contract for chain {}. Please deploy one or specify --disperse-contract",
                chain_id
            ))?
    };

    info!("Using Disperse contract at {:?}", disperse_address);

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
        return Err(anyhow!("Source address must be specified"));
    };

    // Extract addresses and calculate amounts
    // For Beaug, we always use beaugDisperse(recipients, amounts)
    // For equal distribution: GUI calculates equal amounts from amount_to_send / recipient_count
    // For mixed distribution: use the specified amounts
    // Tips are appended as a regular recipient
    let (mut recipient_addresses, mut amounts, is_equal_distribution) = match disperse_type {
        BulkDisperseType::Equal(addresses) => {
            if addresses.is_empty() {
                return Err(anyhow!("No recipients specified"));
            }
            // Calculate equal amounts: amount_to_send / recipient_count
            let amount_per_recipient = amount_to_send / U256::from(addresses.len());
            if amount_per_recipient.is_zero() {
                return Err(anyhow!("Amount per recipient would be zero"));
            }
            let calculated_amounts: Vec<U256> = vec![amount_per_recipient; addresses.len()];
            info!(
                "Equal distribution: {} ETH each to {} recipients",
                utils::format_ether(amount_per_recipient),
                addresses.len()
            );
            (addresses, calculated_amounts, true)
        }
        BulkDisperseType::Mixed(recipients) => {
            if recipients.is_empty() {
                return Err(anyhow!("No recipients specified"));
            }
            let (addresses, amounts): (Vec<Address>, Vec<U256>) = recipients.into_iter().unzip();
            (addresses, amounts, false)
        }
    };
    
    // Append tip as a regular recipient if specified
    if let Some(tip_addr) = verified_tip_recipient {
        recipient_addresses.push(tip_addr);
        amounts.push(tip);
    }
    
    // Calculate total being distributed (includes tip)
    let total_to_distribute: U256 = amounts.iter().fold(U256::zero(), |acc, x| acc + *x);
    
    // Total value to send equals exactly the sum of amounts (contract requires exact match)
    let total_value_to_send = total_to_distribute;

    // Get gas price and apply speed multiplier
    let base_gas_price = provider.get_gas_price().await?;
    let gas_price = base_gas_price * U256::from((gas_speed * 100.0) as u64) / U256::from(100u64);
    info!(
        "Base Gas Price: {} Gwei, With {:.1}x multiplier: {} Gwei",
        ethers::utils::format_units(base_gas_price, "gwei")?,
        gas_speed,
        ethers::utils::format_units(gas_price, "gwei")?
    );

    // Calculate gas limit based on number of recipients
    // With scoring system, each first-time recipient requires:
    // - Transfer: ~30k gas
    // - hasReceivedFunds write: ~20k gas (SSTORE)
    // - scoredAddresses.push: ~40k gas (array expansion + SSTORE)
    // - scores write: ~20k gas (SSTORE)
    // - Event emission: ~3k gas
    // Base overhead: ~100k for function call, calldata, etc.
    // Safe formula: 150k base + 120k per recipient (accounts for first-time scoring)
    // Plus 10% safety buffer for network variability
    let base_gas_limit = 150_000u64 + 120_000u64 * recipient_addresses.len() as u64;
    let gas_limit = (base_gas_limit as f64 * 1.10) as u64; // 10% safety buffer
    let max_gas_limit = 15_000_000u64; // Safety cap (block gas limit)
    let gas_limit = gas_limit.min(max_gas_limit);
    
    // Calculate estimated gas cost
    let estimated_gas_cost = gas_price * U256::from(gas_limit);
    let total_needed = total_value_to_send + estimated_gas_cost;

    // Check that source has enough balance for total + gas
    if source.balance < total_needed {
        if !tip.is_zero() {
            return Err(anyhow!(
                "Balance too low. Source has {} ETH but needs {} ETH (amount: {} + tip: {} + gas: ~{}).",
                utils::format_ether(source.balance),
                utils::format_ether(total_needed),
                utils::format_ether(amount_to_send),
                utils::format_ether(tip),
                utils::format_ether(estimated_gas_cost)
            ));
        } else {
            return Err(anyhow!(
                "Balance too low. Source has {} ETH but needs {} ETH (amount: {} + gas: ~{}).",
                utils::format_ether(source.balance),
                utils::format_ether(total_needed),
                utils::format_ether(amount_to_send),
                utils::format_ether(estimated_gas_cost)
            ));
        }
    }

    // For mixed distribution, ensure amount_to_send is sufficient for specified amounts
    if !is_equal_distribution && amount_to_send < total_to_distribute {
        return Err(anyhow!(
            "Amount to send ({}) is less than the sum of specified amounts ({}).",
            utils::format_ether(amount_to_send),
            utils::format_ether(total_to_distribute)
        ));
    }

    // Log summary
    info!("Bulk Disperse: Contract {:?}, Source {:?} (Index {})", 
        disperse_address, source.address, source.index);
    info!("Amount: {} ETH, Recipients: {} (including tip if any)", 
        utils::format_ether(total_value_to_send), recipient_addresses.len());
    if let Some(tip_addr) = verified_tip_recipient {
        info!("Tip: {} ETH to {:?}", utils::format_ether(tip), tip_addr);
    }

    // Encode function call - always use beaugDisperse(recipients, amounts)
    // Contract requires exact msg.value == sum(amounts)
    let func = get_beaug_disperse_function();
    let recipient_tokens: Vec<Token> = recipient_addresses.iter().map(|a| Token::Address(*a)).collect();
    let amount_tokens: Vec<Token> = amounts.iter().map(|a| Token::Uint(*a)).collect();
    let calldata = func.encode_input(&[Token::Array(recipient_tokens), Token::Array(amount_tokens)])?;

    // Sign and send via the selected Ledger backend (include tip in the value sent)
    let tx_hash = ledger_dispatch::sign_and_send_contract_call(
        use_native_ledger,
        provider.clone(),
        &config.rpc_url,
        source.index,
        disperse_address,
        calldata.to_vec(),
        total_value_to_send,
        gas_limit,
        gas_price,
        source.nonce,
        chain_id,
        config.derivation_mode,
        config.custom_account,
        config.custom_address_index,
        config.coin_type,
    )
    .await?;

    // Wait for receipt
    let mut attempts = 0;
    let max_attempts = 120;
    let (block_number, gas_used) = loop {
        if let Ok(Some(receipt)) = provider.get_transaction_receipt(tx_hash).await {
            break (
                receipt.block_number.map(|n| n.as_u64()),
                receipt.gas_used.map(|g| g.as_u64()).unwrap_or(0),
            );
        }
        attempts += 1;
        if attempts >= max_attempts {
            return Err(anyhow!("Timeout waiting for transaction receipt"));
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    };

    info!("Bulk Disperse Complete! Tx: {:?}, Block: {:?}, Gas: {:?}", 
        tx_hash, block_number, gas_used);

    // Always show amounts (they're calculated for equal distribution)
    let distribution_lines = recipient_addresses
        .iter()
        .zip(amounts.iter())
        .enumerate()
        .map(|(i, (address, amount))| {
            format!("{}. {:?} → {} ETH", i + 1, address, utils::format_ether(*amount))
        })
        .collect::<Vec<_>>()
        .join("\n");
    
    let distribution_type = if is_equal_distribution { "Equal" } else { "Mixed" };

    let tip_info = if let Some(tip_addr) = verified_tip_recipient {
        format!("\nTip: {} ETH to {:?}", utils::format_ether(tip), tip_addr)
    } else {
        String::new()
    };

    operation_log::append_log(
        operation_name,
        chain_id,
        format!(
            "Beaug disperse executed ({})\nSource: {} → {:?}\nBeaug contract: {:?}\nRecipients: {}\n{}\nTotal distributed: {} ETH{}\nTx hash: {:?}\nBlock: {:?}\nGas used: {:?}",
            distribution_type,
            source.derivation_path,
            source.address,
            disperse_address,
            recipient_addresses.len(),
            distribution_lines,
            utils::format_ether(total_to_distribute),
            tip_info,
            tx_hash,
            block_number,
            gas_used
        ),
    )?;

    Ok(())
}

