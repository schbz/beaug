//! Native Ledger hardware wallet operations using ethers-rs built-in support.
//! Provides an alternative to the cast CLI method for Ledger interactions.

use crate::config::{chain_supports_eip1559, DerivationMode};
use crate::ledger_lock;
use ethers::prelude::*;
use ethers::signers::Ledger;
use ethers::types::transaction::eip2718::TypedTransaction;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn, error};

/// Maximum number of retry attempts for transient HID errors
const MAX_RETRY_ATTEMPTS: usize = 5;

/// Base delay between retries in milliseconds (multiplied by attempt number)
const RETRY_BASE_DELAY_MS: u64 = 300;

/// Build an HD derivation path string for the Ledger
fn build_hd_path(
    index: u32,
    derivation_mode: DerivationMode,
    custom_account: u32,
    custom_address_index: u32,
    coin_type: u32,
) -> String {
    derivation_mode.get_path(index, custom_account, custom_address_index, coin_type)
}

/// Check if an error is transient and worth retrying
/// Returns true for HID timing/busy errors that may resolve on retry
fn is_transient_error(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    
    // Windows-specific HID timing issue when device is newly connected/unlocked
    if lower.contains("overlapped i/o operation") || lower.contains("overlapped io operation") {
        return true;
    }
    
    // Device is busy with another operation
    if lower.contains("busy") {
        return true;
    }
    
    // HIDAPI errors that may be transient on device startup
    if lower.contains("hidapi") && !lower.contains("no device") && !lower.contains("device not found") {
        return true;
    }
    
    // Transport errors that may be timing-related
    if lower.contains("transport") && lower.contains("error") {
        return true;
    }
    
    // I/O errors that may be transient
    if lower.contains("i/o error") || lower.contains("io error") {
        return true;
    }
    
    false
}

/// Check if an error indicates the device is locked or app not open
fn is_locked_error(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    lower.contains("locked")
        || lower.contains("6983")
        || lower.contains("6985")
        || lower.contains("denied")
        || lower.contains("not open")
}

/// Check if an error indicates the device is not connected
fn is_disconnected_error(err_str: &str) -> bool {
    let lower = err_str.to_lowercase();
    lower.contains("device not found")
        || lower.contains("no device")
        || (lower.contains("hidapi") && (lower.contains("no device") || lower.contains("device not found")))
        || lower.contains("not connected")
}

/// Get an address from the Ledger using native ethers-rs support
/// Includes retry logic for transient HID errors that can occur when the device
/// is newly connected or unlocked.
pub async fn get_ledger_address_native(
    _chain_id: u64,
    index: u32,
    derivation_mode: DerivationMode,
    custom_account: u32,
    custom_address_index: u32,
    coin_type: u32,
) -> Result<Address> {
    let hd_path = build_hd_path(index, derivation_mode, custom_account, custom_address_index, coin_type);
    
    for attempt in 1..=MAX_RETRY_ATTEMPTS {
        // Serialize Ledger/HID access
        let _lock = ledger_lock::ledger_lock().lock().await;
        
        info!("Getting Ledger address at path {} via native ethers-rs (attempt {}/{})...", 
              hd_path, attempt, MAX_RETRY_ATTEMPTS);
        
        // Construct the HDPath for the Ledger using the Custom variant for arbitrary paths
        let derivation_path = ethers::signers::HDPath::Other(hd_path.clone());
        
        let ledger_result = Ledger::new(derivation_path, 1).await;
        
        match ledger_result {
            Ok(ledger) => {
                match ledger.get_address().await {
                    Ok(address) => {
                        info!("Got address {:?} from Ledger at path {}", address, hd_path);
                        return Ok(address);
                    }
                    Err(e) => {
                        let err_str = e.to_string();
                        
                        // Check if error is transient and we should retry
                        if is_transient_error(&err_str) && attempt < MAX_RETRY_ATTEMPTS {
                            warn!("Transient Ledger error (attempt {}): {}, retrying...", attempt, err_str);
                            drop(_lock); // Release lock before sleeping
                            sleep(Duration::from_millis(RETRY_BASE_DELAY_MS * attempt as u64)).await;
                            continue;
                        }
                        
                        // Non-transient error or max retries reached
                        return Err(map_ledger_error(e));
                    }
                }
            }
            Err(e) => {
                let err_str = e.to_string();
                
                // Check if this is a transient error worth retrying
                if is_transient_error(&err_str) && attempt < MAX_RETRY_ATTEMPTS {
                    warn!("Transient Ledger connection error (attempt {}): {}, retrying...", attempt, err_str);
                    drop(_lock); // Release lock before sleeping
                    sleep(Duration::from_millis(RETRY_BASE_DELAY_MS * attempt as u64)).await;
                    continue;
                }
                
                // For locked/disconnected errors, fail immediately (no point retrying)
                if is_locked_error(&err_str) || is_disconnected_error(&err_str) {
                    return Err(map_ledger_error(e));
                }
                
                // Unknown error - retry if attempts remain
                if attempt < MAX_RETRY_ATTEMPTS {
                    warn!("Ledger error (attempt {}): {}, retrying...", attempt, err_str);
                    drop(_lock); // Release lock before sleeping
                    sleep(Duration::from_millis(RETRY_BASE_DELAY_MS * attempt as u64)).await;
                    continue;
                }
                
                return Err(map_ledger_error(e));
            }
        }
    }
    
    Err(anyhow!("Failed to get Ledger address after {} attempts", MAX_RETRY_ATTEMPTS))
}

/// Get an address with default derivation settings (backward compatible)
pub async fn get_ledger_address_native_simple(
    chain_id: u64,
    index: u32,
) -> Result<Address> {
    get_ledger_address_native(
        chain_id,
        index,
        DerivationMode::default(),
        0,
        0,
        crate::config::DEFAULT_COIN_TYPE,
    ).await
}

/// Sign and send a transaction using native ethers-rs Ledger support
/// Includes retry logic for transient HID errors during connection and signing
pub async fn sign_and_send_transaction_native(
    provider: Arc<Provider<Http>>,
    _rpc_url: &str,
    from_index: u32,
    to: Address,
    value: U256,
    gas_limit: u64,
    gas_price: U256,
    nonce: u64,
    chain_id: u64,
    derivation_mode: DerivationMode,
    custom_account: u32,
    custom_address_index: u32,
    coin_type: u32,
) -> Result<TxHash> {
    let hd_path = build_hd_path(from_index, derivation_mode, custom_account, custom_address_index, coin_type);
    
    // Build the transaction based on chain support (done outside retry loop)
    let tx = if chain_supports_eip1559(chain_id) {
        // EIP-1559 transaction
        let tx = Eip1559TransactionRequest::new()
            .to(to)
            .value(value)
            .gas(gas_limit)
            .max_fee_per_gas(gas_price)
            .max_priority_fee_per_gas(gas_price / 10) // 10% priority fee
            .nonce(nonce)
            .chain_id(chain_id);
        TypedTransaction::Eip1559(tx)
    } else {
        // Legacy transaction
        let tx = TransactionRequest::new()
            .to(to)
            .value(value)
            .gas(gas_limit)
            .gas_price(gas_price)
            .nonce(nonce)
            .chain_id(chain_id);
        TypedTransaction::Legacy(tx)
    };
    
    info!("Transaction built: to={:?}, value={}, gas_limit={}, gas_price={}, nonce={}", 
          to, value, gas_limit, gas_price, nonce);
    
    // Retry loop for connection and signing (NOT for broadcast)
    for attempt in 1..=MAX_RETRY_ATTEMPTS {
        // Serialize Ledger/HID access
        let _lock = ledger_lock::ledger_lock().lock().await;
        
        info!("Signing transaction via native Ledger (path: {}, chain_id: {}, attempt {}/{})", 
              hd_path, chain_id, attempt, MAX_RETRY_ATTEMPTS);
        
        // Construct the HDPath for the Ledger
        let derivation_path = ethers::signers::HDPath::Other(hd_path.clone());
        
        // Connect to Ledger
        let ledger = match Ledger::new(derivation_path, chain_id).await {
            Ok(l) => l,
            Err(e) => {
                let err_str = e.to_string();
                if is_transient_error(&err_str) && attempt < MAX_RETRY_ATTEMPTS {
                    warn!("Transient Ledger connection error (attempt {}): {}, retrying...", attempt, err_str);
                    drop(_lock);
                    sleep(Duration::from_millis(RETRY_BASE_DELAY_MS * attempt as u64)).await;
                    continue;
                }
                return Err(map_ledger_error(e));
            }
        };
        
        // Sign the transaction
        let signature = match ledger.sign_transaction(&tx).await {
            Ok(sig) => sig,
            Err(e) => {
                let err_str = e.to_string();
                // Only retry transient errors - NOT user rejections or locked device
                if is_transient_error(&err_str) && attempt < MAX_RETRY_ATTEMPTS {
                    warn!("Transient signing error (attempt {}): {}, retrying...", attempt, err_str);
                    drop(_lock);
                    sleep(Duration::from_millis(RETRY_BASE_DELAY_MS * attempt as u64)).await;
                    continue;
                }
                return Err(map_ledger_error(e));
            }
        };
        
        info!("Transaction signed successfully");
        
        // Encode and send the signed transaction (NO retry after this point)
        let signed_tx = tx.rlp_signed(&signature);
        let pending_tx = provider.send_raw_transaction(signed_tx).await
            .map_err(|e| anyhow!("Failed to send transaction: {}", e))?;
        
        let tx_hash = pending_tx.tx_hash();
        info!("Transaction sent: {:?}", tx_hash);
        
        return Ok(tx_hash);
    }
    
    Err(anyhow!("Failed to sign transaction after {} attempts", MAX_RETRY_ATTEMPTS))
}

/// Sign and send a contract call using native ethers-rs Ledger support
/// Includes retry logic for transient HID errors during connection and signing
pub async fn sign_and_send_contract_call_native(
    provider: Arc<Provider<Http>>,
    _rpc_url: &str,
    from_index: u32,
    to: Address,
    calldata: Vec<u8>,
    value: U256,
    gas_limit: u64,
    gas_price: U256,
    nonce: u64,
    chain_id: u64,
    derivation_mode: DerivationMode,
    custom_account: u32,
    custom_address_index: u32,
    coin_type: u32,
) -> Result<TxHash> {
    let hd_path = build_hd_path(from_index, derivation_mode, custom_account, custom_address_index, coin_type);
    
    // Build the transaction based on chain support (done outside retry loop)
    let tx = if chain_supports_eip1559(chain_id) {
        // EIP-1559 transaction
        let tx = Eip1559TransactionRequest::new()
            .to(to)
            .value(value)
            .data(calldata.clone())
            .gas(gas_limit)
            .max_fee_per_gas(gas_price)
            .max_priority_fee_per_gas(gas_price / 10)
            .nonce(nonce)
            .chain_id(chain_id);
        TypedTransaction::Eip1559(tx)
    } else {
        // Legacy transaction
        let tx = TransactionRequest::new()
            .to(to)
            .value(value)
            .data(calldata.clone())
            .gas(gas_limit)
            .gas_price(gas_price)
            .nonce(nonce)
            .chain_id(chain_id);
        TypedTransaction::Legacy(tx)
    };
    
    info!("Contract call built: to={:?}, value={}, data_len={}, gas_limit={}", 
          to, value, calldata.len(), gas_limit);
    
    // Retry loop for connection and signing (NOT for broadcast)
    for attempt in 1..=MAX_RETRY_ATTEMPTS {
        // Serialize Ledger/HID access
        let _lock = ledger_lock::ledger_lock().lock().await;
        
        info!("Signing contract call via native Ledger (path: {}, chain_id: {}, calldata: {} bytes, attempt {}/{})", 
              hd_path, chain_id, calldata.len(), attempt, MAX_RETRY_ATTEMPTS);
        
        // Construct the HDPath for the Ledger
        let derivation_path = ethers::signers::HDPath::Other(hd_path.clone());
        
        // Connect to Ledger
        let ledger = match Ledger::new(derivation_path, chain_id).await {
            Ok(l) => l,
            Err(e) => {
                let err_str = e.to_string();
                if is_transient_error(&err_str) && attempt < MAX_RETRY_ATTEMPTS {
                    warn!("Transient Ledger connection error (attempt {}): {}, retrying...", attempt, err_str);
                    drop(_lock);
                    sleep(Duration::from_millis(RETRY_BASE_DELAY_MS * attempt as u64)).await;
                    continue;
                }
                return Err(map_ledger_error(e));
            }
        };
        
        // Sign the transaction
        let signature = match ledger.sign_transaction(&tx).await {
            Ok(sig) => sig,
            Err(e) => {
                let err_str = e.to_string();
                // Only retry transient errors - NOT user rejections or locked device
                if is_transient_error(&err_str) && attempt < MAX_RETRY_ATTEMPTS {
                    warn!("Transient signing error (attempt {}): {}, retrying...", attempt, err_str);
                    drop(_lock);
                    sleep(Duration::from_millis(RETRY_BASE_DELAY_MS * attempt as u64)).await;
                    continue;
                }
                return Err(map_ledger_error(e));
            }
        };
        
        info!("Contract call signed successfully");
        
        // Encode and send the signed transaction (NO retry after this point)
        let signed_tx = tx.rlp_signed(&signature);
        let pending_tx = provider.send_raw_transaction(signed_tx).await
            .map_err(|e| anyhow!("Failed to send contract call: {}", e))?;
        
        let tx_hash = pending_tx.tx_hash();
        info!("Contract call sent: {:?}", tx_hash);
        
        return Ok(tx_hash);
    }
    
    Err(anyhow!("Failed to sign contract call after {} attempts", MAX_RETRY_ATTEMPTS))
}

/// Check if native Ledger is available and connected
/// Uses retry logic from get_ledger_address_native to handle transient HID errors
pub async fn check_ledger_status_native() -> crate::ledger_ops::LedgerStatus {
    // Try to get the first address to check connectivity
    // get_ledger_address_native_simple includes retry logic for transient errors
    match get_ledger_address_native_simple(1, 0).await {
        Ok(address) => crate::ledger_ops::LedgerStatus::Connected { address },
        Err(e) => {
            let err_str = e.to_string();
            
            // Use the helper functions for consistent error classification
            if is_locked_error(&err_str) {
                crate::ledger_ops::LedgerStatus::Locked
            } else if is_disconnected_error(&err_str) {
                crate::ledger_ops::LedgerStatus::Disconnected
            } else {
                // For any other error (including transient errors that exhausted retries),
                // show a truncated message
                crate::ledger_ops::LedgerStatus::Unknown(
                    err_str.chars().take(40).collect()
                )
            }
        }
    }
}

/// Map Ledger errors to user-friendly messages
fn map_ledger_error<E: std::fmt::Display>(e: E) -> anyhow::Error {
    let err_str = e.to_string();
    error!("Ledger error: {}", err_str);
    
    let lower = err_str.to_lowercase();
    
    if lower.contains("device not found") || lower.contains("no device") || lower.contains("hidapi") {
        anyhow!("Ledger device not found. Please ensure it's connected and unlocked.")
    } else if lower.contains("locked") || lower.contains("6983") || lower.contains("6985") {
        anyhow!("Ledger is locked or Ethereum app is not open.")
    } else if lower.contains("denied") || lower.contains("rejected") {
        anyhow!("Transaction was rejected on the Ledger device.")
    } else if lower.contains("timeout") {
        anyhow!("Ledger operation timed out. Please try again.")
    } else if lower.contains("busy") {
        anyhow!("Ledger device is busy. Please wait and try again.")
    } else {
        anyhow!("Ledger error: {}", err_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_hd_path_account_index() {
        let path = build_hd_path(5, DerivationMode::AccountIndex, 0, 0, 60);
        assert_eq!(path, "m/44'/60'/5'/0/0");
    }

    #[test]
    fn test_build_hd_path_address_index() {
        let path = build_hd_path(5, DerivationMode::AddressIndex, 2, 0, 60);
        assert_eq!(path, "m/44'/60'/2'/0/5");
    }

    #[test]
    fn test_build_hd_path_custom_coin_type() {
        let path = build_hd_path(0, DerivationMode::AccountIndex, 0, 0, 714);
        assert_eq!(path, "m/44'/714'/0'/0/0");
    }

    #[test]
    fn test_is_transient_error() {
        // Windows-specific overlapped I/O error
        assert!(is_transient_error("Overlapped I/O operation is in progress"));
        assert!(is_transient_error("overlapped io operation"));
        
        // Busy device
        assert!(is_transient_error("Device is busy"));
        assert!(is_transient_error("BUSY"));
        
        // Transport errors
        assert!(is_transient_error("transport error occurred"));
        
        // I/O errors
        assert!(is_transient_error("i/o error during communication"));
        assert!(is_transient_error("io error"));
        
        // HIDAPI error that's NOT disconnected
        assert!(is_transient_error("hidapi timeout error"));
        
        // Should NOT be transient
        assert!(!is_transient_error("device not found"));
        assert!(!is_transient_error("hidapi: no device found"));
        assert!(!is_transient_error("locked"));
        assert!(!is_transient_error("denied"));
    }

    #[test]
    fn test_is_locked_error() {
        assert!(is_locked_error("Device is locked"));
        assert!(is_locked_error("Error 6983"));
        assert!(is_locked_error("Error 6985"));
        assert!(is_locked_error("Request denied"));
        assert!(is_locked_error("App not open"));
        
        // Should NOT be locked
        assert!(!is_locked_error("device not found"));
        assert!(!is_locked_error("busy"));
    }

    #[test]
    fn test_is_disconnected_error() {
        assert!(is_disconnected_error("Device not found"));
        assert!(is_disconnected_error("No device connected"));
        assert!(is_disconnected_error("hidapi: no device found"));
        assert!(is_disconnected_error("hidapi: device not found"));
        assert!(is_disconnected_error("Ledger not connected"));
        
        // Should NOT be disconnected
        assert!(!is_disconnected_error("locked"));
        assert!(!is_disconnected_error("busy"));
        assert!(!is_disconnected_error("denied"));
    }
}
