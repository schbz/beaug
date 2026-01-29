//! Ledger signing operations using Foundry's cast CLI tool.
//! Handles transaction signing and broadcasting via the cast command.

use crate::ledger_ops::get_cast_path;
use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use tracing::{info, warn, error};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const CAST_TIMEOUT_SECS: u64 = 300; // 5 minutes

async fn run_command_with_timeout(
    mut command: Command,
    timeout_secs: u64,
) -> anyhow::Result<std::process::Output> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    // Hide console window on Windows
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command.spawn()?;

    let mut child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture cast stdout"))?;
    let mut child_stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("Failed to capture cast stderr"))?;

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        child_stdout.read_to_end(&mut buf).await?;
        anyhow::Result::<Vec<u8>>::Ok(buf)
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        child_stderr.read_to_end(&mut buf).await?;
        anyhow::Result::<Vec<u8>>::Ok(buf)
    });

    let status = match timeout(Duration::from_secs(timeout_secs), child.wait()).await {
        Ok(res) => res?,
        Err(_) => {
            let _ = child.kill().await;
            return Err(anyhow::anyhow!(
                "Timed out waiting for cast to finish ({}s).",
                timeout_secs
            ));
        }
    };

    let stdout = stdout_task
        .await
        .map_err(|e| anyhow::anyhow!("stdout task join error: {}", e))??;
    let stderr = stderr_task
        .await
        .map_err(|e| anyhow::anyhow!("stderr task join error: {}", e))??;

    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

/// Get address from Ledger at given index using cast (uses default coin type 60)
pub async fn get_ledger_address(_chain_id: u64, index: u32) -> anyhow::Result<Address> {
    get_ledger_address_with_derivation(_chain_id, index, crate::config::DerivationMode::AccountIndex, 0, crate::config::DEFAULT_COIN_TYPE).await
}

/// Get address from Ledger at given index using cast with custom derivation
pub async fn get_ledger_address_with_derivation(
    _chain_id: u64, 
    index: u32,
    derivation_mode: crate::config::DerivationMode,
    custom_account: u32,
    coin_type: u32,
) -> anyhow::Result<Address> {
    let hd_path = derivation_mode.get_path(index, custom_account, 0, coin_type);
    
    info!("Getting Ledger address at index {} via cast...", index);

    // Serialize Ledger/HID access across the entire process.
    let _lock = crate::ledger_lock::ledger_lock().lock().await;

    let mut command = Command::new(get_cast_path());
    command
        .arg("wallet")
        .arg("address")
        .arg("--ledger")
        .arg("--hd-path")
        .arg(&hd_path);

    let output = run_command_with_timeout(command, CAST_TIMEOUT_SECS).await?;
    
    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let addr_str = stdout.trim();
        let address: Address = addr_str.parse()?;
        info!("Got address {:?} from Ledger at index {}", address, index);
        Ok(address)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("Failed to get Ledger address: {}", stderr))
    }
}

/// Sign and send a transaction using Foundry's cast (uses default derivation settings)
pub async fn sign_and_send_transaction(
    _provider: Arc<Provider<Http>>,
    rpc_url: &str,
    from_index: u32,
    to: Address,
    value: U256,
    gas_limit: u64,
    gas_price: U256,
    nonce: u64,
    _chain_id: u64,
) -> anyhow::Result<TxHash> {
    sign_and_send_transaction_with_derivation(
        _provider, rpc_url, from_index, to, value, gas_limit, gas_price, nonce, _chain_id,
        crate::config::DerivationMode::AccountIndex, 0, crate::config::DEFAULT_COIN_TYPE
    ).await
}

/// Sign and send a transaction using Foundry's cast with custom derivation
pub async fn sign_and_send_transaction_with_derivation(
    _provider: Arc<Provider<Http>>,
    rpc_url: &str,
    from_index: u32,
    to: Address,
    value: U256,
    gas_limit: u64,
    gas_price: U256,
    nonce: u64,
    _chain_id: u64,
    derivation_mode: crate::config::DerivationMode,
    custom_account: u32,
    coin_type: u32,
) -> anyhow::Result<TxHash> {
    sign_and_send_transaction_with_full_derivation(
        _provider, rpc_url, from_index, to, value, gas_limit, gas_price, nonce, _chain_id,
        derivation_mode, custom_account, 0, coin_type
    ).await
}

/// Sign and send a transaction using Foundry's cast with full custom derivation
/// Supports both EIP-1559 (Type 2) and legacy (Type 0) transactions based on chain support
pub async fn sign_and_send_transaction_with_full_derivation(
    _provider: Arc<Provider<Http>>,
    rpc_url: &str,
    from_index: u32,
    to: Address,
    value: U256,
    gas_limit: u64,
    gas_price: U256,
    nonce: u64,
    chain_id: u64,
    derivation_mode: crate::config::DerivationMode,
    custom_account: u32,
    custom_address_index: u32,
    coin_type: u32,
) -> anyhow::Result<TxHash> {
    let hd_path = derivation_mode.get_path(from_index, custom_account, custom_address_index, coin_type);

    // Serialize Ledger/HID access across the entire process.
    let _lock = crate::ledger_lock::ledger_lock().lock().await;
    
    // Convert values to strings for cast
    let value_str = format!("{}wei", value);
    let gas_price_str = format!("{}wei", gas_price);
    let to_str = format!("{:?}", to);
    
    // Check if chain supports EIP-1559 (for logging purposes)
    let use_eip1559 = crate::config::chain_supports_eip1559(chain_id);
    
    info!(
        "Sending {} to {} via cast (Ledger index {}, nonce {}, gas_price {}, tx_type: {})",
        value_str, to_str, from_index, nonce, gas_price_str,
        if use_eip1559 { "EIP-1559" } else { "legacy" }
    );
    
    // Build cast send command
    // Note: We always use --gas-price. For EIP-1559 chains cast will auto-calculate priority fee.
    // For legacy chains we must explicitly pass --legacy to avoid EIP-1559 fee estimation issues.
    // We also pass --chain to ensure correct EIP-155 signature encoding.
    let mut command = Command::new(get_cast_path());
    command
        .arg("send")
        .arg("--ledger")
        .arg("--hd-path")
        .arg(&hd_path)
        .arg("--rpc-url")
        .arg(rpc_url)
        .arg("--chain")
        .arg(chain_id.to_string())
        .arg("--gas-limit")
        .arg(gas_limit.to_string())
        .arg("--gas-price")
        .arg(&gas_price_str)
        .arg("--nonce")
        .arg(nonce.to_string());
    
    // Use legacy transaction type for chains that don't support EIP-1559
    if !use_eip1559 {
        command.arg("--legacy");
    }
    
    // Add value if non-zero
    if !value.is_zero() {
        command.arg("--value").arg(&value_str);
    }
    
    // Add destination address
    command.arg(&to_str);
    
    info!("Cast command: cast send --ledger --hd-path {} --rpc-url <rpc> --chain {} --gas-limit {} --gas-price {} --nonce {}{} --value {} {}",
        hd_path, chain_id, gas_limit, gas_price_str, nonce, if use_eip1559 { "" } else { " --legacy" }, value_str, to_str);

    let output = run_command_with_timeout(command, CAST_TIMEOUT_SECS).await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    if !output.status.success() {
        error!("cast send failed: {}", stderr);
        return Err(anyhow::anyhow!("cast send failed: {}", stderr));
    }
    
    // Parse transaction hash from output
    // cast send outputs either just the tx hash OR a full receipt
    let tx_hash_str = stdout.trim();
    info!("Transaction sent: {}", tx_hash_str);
    
    // First, check if cast returned a full receipt (newer versions do this)
    if tx_hash_str.contains("transactionHash") {
        // Parse the receipt format - look for "transactionHash      0x..."
        for line in tx_hash_str.lines() {
            if line.trim_start().starts_with("transactionHash") {
                // Extract the hash value after "transactionHash" and whitespace
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[1].starts_with("0x") {
                    let hash: TxHash = parts[1].parse()?;
                    return Ok(hash);
                }
            }
        }
    }
    
    // Fall back to simple hash parsing for older cast versions
    if let Some(hash_line) = tx_hash_str.lines().find(|l| l.starts_with("0x") && l.len() >= 66) {
        let hash: TxHash = hash_line.trim().parse()?;
        Ok(hash)
    } else if tx_hash_str.starts_with("0x") && tx_hash_str.len() >= 66 {
        let hash: TxHash = tx_hash_str[..66].parse()?;
        Ok(hash)
    } else {
        // Sometimes cast outputs the hash differently, try extracting it
        warn!("Unexpected output format: {}", tx_hash_str);
        // Look for a hash in the output
        for word in tx_hash_str.split_whitespace() {
            if word.starts_with("0x") && word.len() >= 66 {
                if let Ok(hash) = word[..66].parse::<TxHash>() {
                    return Ok(hash);
                }
            }
        }
        Err(anyhow::anyhow!("Could not parse transaction hash from: {}", tx_hash_str))
    }
}

/// Sign and send a contract call using cast (uses default derivation settings)
pub async fn sign_and_send_contract_call(
    _provider: Arc<Provider<Http>>,
    rpc_url: &str,
    from_index: u32,
    to: Address,
    calldata: Vec<u8>,
    value: U256,
    gas_limit: u64,
    gas_price: U256,
    nonce: u64,
    _chain_id: u64,
) -> anyhow::Result<TxHash> {
    sign_and_send_contract_call_with_derivation(
        _provider, rpc_url, from_index, to, calldata, value, gas_limit, gas_price, nonce, _chain_id,
        crate::config::DerivationMode::AccountIndex, 0, crate::config::DEFAULT_COIN_TYPE
    ).await
}

/// Sign and send a contract call using cast with custom derivation
pub async fn sign_and_send_contract_call_with_derivation(
    _provider: Arc<Provider<Http>>,
    rpc_url: &str,
    from_index: u32,
    to: Address,
    calldata: Vec<u8>,
    value: U256,
    gas_limit: u64,
    gas_price: U256,
    nonce: u64,
    _chain_id: u64,
    derivation_mode: crate::config::DerivationMode,
    custom_account: u32,
    coin_type: u32,
) -> anyhow::Result<TxHash> {
    sign_and_send_contract_call_with_full_derivation(
        _provider, rpc_url, from_index, to, calldata, value, gas_limit, gas_price, nonce, _chain_id,
        derivation_mode, custom_account, 0, coin_type
    ).await
}

/// Sign and send a contract call using cast with full custom derivation
/// Supports both EIP-1559 (Type 2) and legacy (Type 0) transactions based on chain support
pub async fn sign_and_send_contract_call_with_full_derivation(
    _provider: Arc<Provider<Http>>,
    rpc_url: &str,
    from_index: u32,
    to: Address,
    calldata: Vec<u8>,
    value: U256,
    gas_limit: u64,
    gas_price: U256,
    nonce: u64,
    chain_id: u64,
    derivation_mode: crate::config::DerivationMode,
    custom_account: u32,
    custom_address_index: u32,
    coin_type: u32,
) -> anyhow::Result<TxHash> {
    let hd_path = derivation_mode.get_path(from_index, custom_account, custom_address_index, coin_type);

    // Serialize Ledger/HID access across the entire process.
    let _lock = crate::ledger_lock::ledger_lock().lock().await;
    
    let value_str = format!("{}wei", value);
    let gas_price_str = format!("{}wei", gas_price);
    let calldata_hex = format!("0x{}", hex::encode(&calldata));
    let to_str = format!("{:?}", to);
    
    // Check if chain supports EIP-1559 (for logging purposes)
    let use_eip1559 = crate::config::chain_supports_eip1559(chain_id);
    
    info!(
        "Sending contract call to {} via cast (Ledger index {}, nonce {}, calldata {} bytes, gas_limit: {}, gas_price: {}, value: {}, tx_type: {})",
        to_str, from_index, nonce, calldata.len(), gas_limit, gas_price_str, value_str,
        if use_eip1559 { "EIP-1559" } else { "legacy" }
    );
    
    // Build cast send command
    // Note: We always use --gas-price. For EIP-1559 chains cast will auto-calculate priority fee.
    // For legacy chains we must explicitly pass --legacy to avoid EIP-1559 fee estimation issues.
    // We also pass --chain to ensure correct EIP-155 signature encoding.
    let mut command = Command::new(get_cast_path());
    command
        .arg("send")
        .arg("--ledger")
        .arg("--hd-path")
        .arg(&hd_path)
        .arg("--rpc-url")
        .arg(rpc_url)
        .arg("--chain")
        .arg(chain_id.to_string())
        .arg("--gas-limit")
        .arg(gas_limit.to_string())
        .arg("--gas-price")
        .arg(&gas_price_str)
        .arg("--nonce")
        .arg(nonce.to_string());
    
    // Use legacy transaction type for chains that don't support EIP-1559
    if !use_eip1559 {
        command.arg("--legacy");
    }
    
    if !value.is_zero() {
        command.arg("--value").arg(&value_str);
    }
    
    command.arg(&to_str).arg(&calldata_hex);
    
    // Log the full command for debugging (without showing full calldata if it's large)
    let calldata_preview = if calldata_hex.len() > 100 {
        format!("{}...({} bytes)", &calldata_hex[..66], calldata.len())
    } else {
        calldata_hex.clone()
    };
    info!("Cast command: cast send --ledger --hd-path {} --rpc-url <rpc> --chain {} --gas-limit {} --gas-price {} --nonce {}{} {} {}",
        hd_path, chain_id, gas_limit, gas_price_str, nonce, if use_eip1559 { "" } else { " --legacy" }, to_str, calldata_preview);

    let output = run_command_with_timeout(command, CAST_TIMEOUT_SECS).await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    
    if !output.status.success() {
        error!("cast send failed: {} (stderr: {})", stderr.trim(), if stdout.is_empty() { "none" } else { stdout.trim() });
        return Err(anyhow::anyhow!("cast send failed: {}", stderr));
    }
    
    let tx_hash_str = stdout.trim();
    info!("Contract call sent: {}", tx_hash_str);
    
    // First, check if cast returned a full receipt (newer versions do this)
    if tx_hash_str.contains("transactionHash") {
        // Parse the receipt format - look for "transactionHash      0x..."
        for line in tx_hash_str.lines() {
            if line.trim_start().starts_with("transactionHash") {
                // Extract the hash value after "transactionHash" and whitespace
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 && parts[1].starts_with("0x") {
                    let hash: TxHash = parts[1].parse()?;
                    return Ok(hash);
                }
            }
        }
    }
    
    // Fall back to simple hash parsing for older cast versions
    if let Some(hash_line) = tx_hash_str.lines().find(|l| l.starts_with("0x") && l.len() >= 66) {
        let hash: TxHash = hash_line.trim().parse()?;
        Ok(hash)
    } else if tx_hash_str.starts_with("0x") && tx_hash_str.len() >= 66 {
        let hash: TxHash = tx_hash_str[..66].parse()?;
        Ok(hash)
    } else {
        for word in tx_hash_str.split_whitespace() {
            if word.starts_with("0x") && word.len() >= 66 {
                if let Ok(hash) = word[..66].parse::<TxHash>() {
                    return Ok(hash);
                }
            }
        }
        Err(anyhow::anyhow!("Could not parse transaction hash from: {}", tx_hash_str))
    }
}

/// Check if cast is available
pub fn check_cast_available() -> anyhow::Result<()> {
    let mut cmd = std::process::Command::new(get_cast_path());
    cmd.arg("--version");
    
    // Hide console window on Windows
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    
    let output = cmd.output();
    match output {
        Ok(o) if o.status.success() => Ok(()),
        _ => Err(anyhow::anyhow!(
            "Foundry's 'cast' command not found. Please install Foundry: https://getfoundry.sh"
        )),
    }
}
