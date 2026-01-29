//! Ledger operation dispatch layer.
//! Routes Ledger operations to either Cast CLI or native ethers-rs implementation
//! based on user settings.

use crate::config::DerivationMode;
use crate::ethers_ledger_signer;
use crate::ledger_ops::{self, LedgerStatus};
use crate::native_ledger;
use crate::user_settings::UserSettings;
use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use anyhow::Result;
use std::sync::Arc;
use tracing::info;

/// Ledger backend selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LedgerBackend {
    /// Use Foundry's cast CLI (stable, requires external dependency)
    Cast,
    /// Use native ethers-rs Ledger support (experimental, no external deps)
    Native,
}

impl LedgerBackend {
    /// Get the backend from user settings
    pub fn from_settings(settings: &UserSettings) -> Self {
        if settings.use_native_ledger {
            LedgerBackend::Native
        } else {
            LedgerBackend::Cast
        }
    }
    
    /// Get display name for the backend
    pub fn display_name(&self) -> &'static str {
        match self {
            LedgerBackend::Cast => "Foundry Cast",
            LedgerBackend::Native => "Native (ethers-rs)",
        }
    }
    
    /// Get description for the backend
    pub fn description(&self) -> &'static str {
        match self {
            LedgerBackend::Cast => "Uses Foundry's cast CLI for Ledger operations. Mature and well-tested.",
            LedgerBackend::Native => "Uses ethers-rs native Ledger support. No external dependencies, experimental.",
        }
    }
}

/// Check Ledger status using the appropriate backend
pub async fn check_ledger_status(use_native: bool, chain_id: u64) -> LedgerStatus {
    if use_native {
        info!("Checking Ledger status via native ethers-rs");
        native_ledger::check_ledger_status_native().await
    } else {
        info!("Checking Ledger status via cast");
        ledger_ops::check_ledger_status(chain_id).await
    }
}

/// Get Ledger address using the appropriate backend (simple version)
pub async fn get_ledger_address(
    use_native: bool,
    chain_id: u64,
    index: u32,
) -> Result<Address> {
    if use_native {
        native_ledger::get_ledger_address_native_simple(chain_id, index).await
    } else {
        ledger_ops::get_ledger_address(chain_id, index).await
    }
}

/// Get Ledger address with custom derivation settings
pub async fn get_ledger_address_with_config(
    use_native: bool,
    chain_id: u64,
    index: u32,
    derivation_mode: DerivationMode,
    custom_account: u32,
    custom_address_index: u32,
    coin_type: u32,
) -> Result<Address> {
    if use_native {
        native_ledger::get_ledger_address_native(
            chain_id,
            index,
            derivation_mode,
            custom_account,
            custom_address_index,
            coin_type,
        ).await
    } else {
        // Cast version uses config for derivation path
        ethers_ledger_signer::get_ledger_address_with_derivation(
            chain_id,
            index,
            derivation_mode,
            custom_account,
            coin_type,
        ).await
    }
}

/// Get Ledger address with retry (for config-based operations)
pub async fn get_ledger_address_with_retry_config(
    use_native: bool,
    chain_id: u64,
    index: u32,
    config: Option<&crate::config::Config>,
) -> Result<Address> {
    if use_native {
        // Extract derivation params from config
        let (derivation_mode, custom_account, custom_address_index, coin_type) = if let Some(cfg) = config {
            (cfg.derivation_mode, cfg.custom_account, cfg.custom_address_index, cfg.coin_type)
        } else {
            (DerivationMode::default(), 0, 0, crate::config::DEFAULT_COIN_TYPE)
        };
        
        native_ledger::get_ledger_address_native(
            chain_id,
            index,
            derivation_mode,
            custom_account,
            custom_address_index,
            coin_type,
        ).await
    } else {
        ledger_ops::get_ledger_address_with_retry_config(chain_id, index, config).await
    }
}

/// Sign and send a transaction using the appropriate backend
pub async fn sign_and_send_transaction(
    use_native: bool,
    provider: Arc<Provider<Http>>,
    rpc_url: &str,
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
    if use_native {
        info!("Signing transaction via native Ledger");
        native_ledger::sign_and_send_transaction_native(
            provider,
            rpc_url,
            from_index,
            to,
            value,
            gas_limit,
            gas_price,
            nonce,
            chain_id,
            derivation_mode,
            custom_account,
            custom_address_index,
            coin_type,
        ).await
    } else {
        info!("Signing transaction via cast");
        ethers_ledger_signer::sign_and_send_transaction_with_full_derivation(
            provider,
            rpc_url,
            from_index,
            to,
            value,
            gas_limit,
            gas_price,
            nonce,
            chain_id,
            derivation_mode,
            custom_account,
            custom_address_index,
            coin_type,
        ).await
    }
}

/// Sign and send a contract call using the appropriate backend
pub async fn sign_and_send_contract_call(
    use_native: bool,
    provider: Arc<Provider<Http>>,
    rpc_url: &str,
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
    if use_native {
        info!("Signing contract call via native Ledger");
        native_ledger::sign_and_send_contract_call_native(
            provider,
            rpc_url,
            from_index,
            to,
            calldata,
            value,
            gas_limit,
            gas_price,
            nonce,
            chain_id,
            derivation_mode,
            custom_account,
            custom_address_index,
            coin_type,
        ).await
    } else {
        info!("Signing contract call via cast");
        ethers_ledger_signer::sign_and_send_contract_call_with_full_derivation(
            provider,
            rpc_url,
            from_index,
            to,
            calldata,
            value,
            gas_limit,
            gas_price,
            nonce,
            chain_id,
            derivation_mode,
            custom_account,
            custom_address_index,
            coin_type,
        ).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backend_from_settings_native() {
        // Native is now the default
        let settings = UserSettings::default();
        let backend = LedgerBackend::from_settings(&settings);
        assert_eq!(backend, LedgerBackend::Native);
    }

    #[test]
    fn test_backend_from_settings_cast() {
        // Cast is the backup option when use_native_ledger is false
        let mut settings = UserSettings::default();
        settings.use_native_ledger = false;
        let backend = LedgerBackend::from_settings(&settings);
        assert_eq!(backend, LedgerBackend::Cast);
    }

    #[test]
    fn test_backend_display_names() {
        assert_eq!(LedgerBackend::Cast.display_name(), "Foundry Cast");
        assert_eq!(LedgerBackend::Native.display_name(), "Native (ethers-rs)");
    }
}
