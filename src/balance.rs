use crate::{config::Config, ledger_dispatch, utils};
use ethers::prelude::*;
use anyhow::Result;
use tokio::sync::mpsc;
use tokio::sync::oneshot;

#[derive(Debug, Clone)]
pub struct BalanceScanRecord {
    pub index: u32,
    pub address: Address,
    pub balance: U256,
    pub derivation_path: String,
}

#[derive(Debug, Clone)]
pub struct BalanceScanResult {
    pub records: Vec<BalanceScanRecord>,
    pub empty_addresses: Vec<(u32, Address)>,
    pub last_scanned_index: u32,
    pub met_target: bool,
    pub cancelled: bool,
}

impl BalanceScanResult {
    pub fn summary(&self) -> String {
        if self.cancelled {
            return "Scan cancelled.".to_string();
        }
        if self.met_target {
            format!(
                "Found {} consecutive empty addresses (target met). Scanned up to index {}.",
                self.empty_addresses.len(),
                self.last_scanned_index
            )
        } else {
            format!(
                "Found {} consecutive empty addresses. Scanned up to index {}.",
                self.empty_addresses.len(),
                self.last_scanned_index
            )
        }
    }

    pub fn formatted_records(&self) -> Vec<String> {
        self.records
            .iter()
            .map(|r| {
                format!(
                    "{}: {:?} - {} ETH",
                    r.derivation_path,
                    r.address,
                    utils::format_ether(r.balance)
                )
            })
            .collect()
    }
}

pub async fn scan_consecutive_empty(
    config: &Config,
    empty_target: u32,
    start_index: u32,
    use_native_ledger: bool,
) -> Result<BalanceScanResult> {
    let provider = config.get_provider().await?;
    let mut consecutive_empty = 0;
    let mut index = start_index;
    let mut last_scanned_index = start_index.saturating_sub(1);
    let mut empty_sequence: Vec<(u32, Address)> = Vec::new();
    let mut records: Vec<BalanceScanRecord> = Vec::new();
    let mut cancelled = false;

    while consecutive_empty < empty_target {
        match ledger_dispatch::get_ledger_address_with_retry_config(use_native_ledger, config.chain_id, index, Some(config)).await {
            Ok(addr) => {
                let balance = provider.get_balance(addr, None).await?;
                records.push(BalanceScanRecord {
                    index,
                    address: addr,
                    balance,
                    derivation_path: config.get_derivation_path(index),
                });
                last_scanned_index = index;

                if balance.is_zero() {
                    consecutive_empty += 1;
                    empty_sequence.push((index, addr));
                } else {
                    consecutive_empty = 0;
                    empty_sequence.clear();
                }
            }
            Err(_) => {
                cancelled = true;
                break;
            }
        }
        index += 1;
    }

    Ok(BalanceScanResult {
        records,
        empty_addresses: empty_sequence,
        last_scanned_index,
        met_target: consecutive_empty >= empty_target,
        cancelled,
    })
}

#[derive(Debug, Clone)]
pub struct FundedAddressScan {
    pub funded: Vec<BalanceScanRecord>,
    pub empty: Vec<BalanceScanRecord>,
}

/// Progress update for streaming scan
#[derive(Debug, Clone)]
pub enum ScanProgress {
    AddressFound(BalanceScanRecord),
    Completed(BalanceScanResult),
}

/// Scan addresses with real-time progress updates and cancellation support
pub async fn scan_consecutive_empty_streaming(
    config: Config,
    empty_target: u32,
    start_index: u32,
    progress_sender: mpsc::UnboundedSender<ScanProgress>,
    mut cancel_receiver: oneshot::Receiver<()>,
    use_native_ledger: bool,
) -> Result<()> {
    let provider = config.get_provider().await?;
    let mut consecutive_empty = 0;
    let mut index = start_index;
    let mut last_scanned_index = start_index.saturating_sub(1);
    let mut empty_sequence: Vec<(u32, Address)> = Vec::new();
    let mut records: Vec<BalanceScanRecord> = Vec::new();
    let mut cancelled = false;

    loop {
        // Check for cancellation
        if cancel_receiver.try_recv().is_ok() {
            cancelled = true;
            break;
        }

        // Check if we've met the target
        if consecutive_empty >= empty_target {
            break;
        }

        match ledger_dispatch::get_ledger_address_with_retry_config(use_native_ledger, config.chain_id, index, Some(&config)).await {
            Ok(addr) => {
                let balance = provider.get_balance(addr, None).await?;
                let record = BalanceScanRecord {
                    index,
                    address: addr,
                    balance,
                    derivation_path: config.get_derivation_path(index),
                };
                
                // Send progress update
                let _ = progress_sender.send(ScanProgress::AddressFound(record.clone()));
                
                records.push(record);
                last_scanned_index = index;

                if balance.is_zero() {
                    consecutive_empty += 1;
                    empty_sequence.push((index, addr));
                } else {
                    consecutive_empty = 0;
                    empty_sequence.clear();
                }
            }
            Err(_) => {
                cancelled = true;
                break;
            }
        }
        index += 1;
    }

    // Send final result
    let result = BalanceScanResult {
        records,
        empty_addresses: empty_sequence,
        last_scanned_index,
        met_target: consecutive_empty >= empty_target,
        cancelled,
    };
    
    let _ = progress_sender.send(ScanProgress::Completed(result));
    Ok(())
}

/// Progress update for funded address scan
#[derive(Debug, Clone)]
pub enum FundedScanProgress {
    AddressFound(BalanceScanRecord),
    Completed(FundedAddressScan),
}

pub async fn scan_for_funded_addresses(
    config: &Config,
    start_index: u32,
    empty_streak_target: u32,
    use_native_ledger: bool,
) -> Result<FundedAddressScan> {
    let provider = config.get_provider().await?;
    let mut funded = Vec::new();
    let mut empty = Vec::new();
    let mut consecutive_empty = 0;
    let mut index = start_index;
    let scan_limit = start_index + 50;

    while index < scan_limit && consecutive_empty < empty_streak_target {
        match ledger_dispatch::get_ledger_address_with_retry_config(use_native_ledger, config.chain_id, index, Some(config)).await {
            Ok(addr) => {
                let balance = provider.get_balance(addr, None).await?;
                let record = BalanceScanRecord {
                    index,
                    address: addr,
                    balance,
                    derivation_path: config.get_derivation_path(index),
                };

                if balance.is_zero() {
                    consecutive_empty += 1;
                    empty.push(record);
                } else {
                    consecutive_empty = 0;
                    funded.push(record);
                }
            }
            Err(_) => break,
        }
        index += 1;
    }

    Ok(FundedAddressScan { funded, empty })
}

/// Streaming version of funded address scan with real-time progress updates and cancellation support
pub async fn scan_for_funded_addresses_streaming(
    config: Config,
    start_index: u32,
    empty_streak_target: u32,
    progress_sender: mpsc::UnboundedSender<FundedScanProgress>,
    mut cancel_receiver: oneshot::Receiver<()>,
    use_native_ledger: bool,
) -> Result<()> {
    let provider = config.get_provider().await?;
    let mut funded = Vec::new();
    let mut empty = Vec::new();
    let mut consecutive_empty = 0;
    let mut index = start_index;
    let scan_limit = start_index + 50;

    while index < scan_limit && consecutive_empty < empty_streak_target {
        // Check for cancellation
        if cancel_receiver.try_recv().is_ok() {
            break;
        }

        match ledger_dispatch::get_ledger_address_with_retry_config(use_native_ledger, config.chain_id, index, Some(&config)).await {
            Ok(addr) => {
                let balance = provider.get_balance(addr, None).await?;
                let record = BalanceScanRecord {
                    index,
                    address: addr,
                    balance,
                    derivation_path: config.get_derivation_path(index),
                };

                // Send progress update for each address found
                let _ = progress_sender.send(FundedScanProgress::AddressFound(record.clone()));

                if balance.is_zero() {
                    consecutive_empty += 1;
                    empty.push(record);
                } else {
                    consecutive_empty = 0;
                    funded.push(record);
                }
            }
            Err(_) => {
                break;
            }
        }
        index += 1;
    }

    // Send final result
    let result = FundedAddressScan { funded, empty };
    let _ = progress_sender.send(FundedScanProgress::Completed(result));
    Ok(())
}

