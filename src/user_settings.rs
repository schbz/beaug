use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

const SETTINGS_FILE: &str = "beaug_settings.json";

/// A user-defined custom EVM network
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CustomNetwork {
    /// Display name for the network
    pub label: String,
    /// Chain ID (must be unique)
    pub chain_id: u64,
    /// Native token symbol (e.g., "ETH", "MATIC")
    pub native_token: String,
    /// RPC endpoint URL
    pub rpc_url: String,
}

impl CustomNetwork {
    pub fn new(label: String, chain_id: u64, native_token: String, rpc_url: String) -> Self {
        Self {
            label,
            chain_id,
            native_token,
            rpc_url,
        }
    }
}

fn default_custom_networks() -> Vec<CustomNetwork> {
    Vec::new()
}

fn default_coin_type() -> Option<u32> {
    None  // None means use default (60)
}

fn default_use_native_ledger() -> bool {
    true  // Default to native ethers-rs Ledger support
}

/// User settings that persist between sessions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSettings {
    /// Selected network chain ID
    pub selected_chain_id: u64,
    /// Custom RPC overrides per chain ID
    #[serde(default)]
    pub custom_rpcs: HashMap<u64, String>,
    /// Default gas speed multiplier (0.8 = Slow, 1.0 = Standard, 1.5 = Fast, 2.0+ = Aggressive)
    #[serde(default = "default_gas_speed")]
    pub default_gas_speed: f32,
    /// Default tip percentage for bulk disperse donations (0.0 to 10.0)
    #[serde(default = "default_tip_percentage")]
    pub default_tip_percentage: f32,
    /// Default output count for split operations
    #[serde(default = "default_split_outputs")]
    pub default_split_outputs: u32,
    /// Default output count for bulk operations
    #[serde(default = "default_bulk_outputs")]
    pub default_bulk_outputs: u32,
    /// Default scan start index
    #[serde(default = "default_scan_start_index")]
    pub default_scan_start_index: u32,
    /// Default scan empty streak
    #[serde(default = "default_scan_empty_streak")]
    pub default_scan_empty_streak: u32,
    /// Auto-refresh interval for Ledger status (seconds)
    #[serde(default = "default_ledger_refresh_interval")]
    pub ledger_refresh_interval_secs: u64,
    /// User-defined custom networks
    #[serde(default = "default_custom_networks")]
    pub custom_networks: Vec<CustomNetwork>,
    /// Custom coin type override (None = use default 60 for all chains)
    #[serde(default = "default_coin_type")]
    pub coin_type_override: Option<u32>,
    /// Default remaining balance to keep on source address (in wei)
    #[serde(default = "default_remaining_balance")]
    pub default_remaining_balance: u64,
    /// Use native ethers-rs Ledger support instead of Foundry cast
    #[serde(default = "default_use_native_ledger")]
    pub use_native_ledger: bool,
}

fn default_gas_speed() -> f32 {
    1.0 // Standard speed
}

fn default_tip_percentage() -> f32 {
    0.0 // No tip by default
}

fn default_split_outputs() -> u32 {
    5
}

fn default_bulk_outputs() -> u32 {
    10
}

fn default_scan_start_index() -> u32 {
    0
}

fn default_scan_empty_streak() -> u32 {
    5
}

fn default_ledger_refresh_interval() -> u64 {
    5
}

fn default_remaining_balance() -> u64 {
    0
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            selected_chain_id: 11155111, // Sepolia by default
            custom_rpcs: HashMap::new(),
            default_gas_speed: default_gas_speed(),
            default_tip_percentage: default_tip_percentage(),
            default_split_outputs: default_split_outputs(),
            default_bulk_outputs: default_bulk_outputs(),
            default_scan_start_index: default_scan_start_index(),
            default_scan_empty_streak: default_scan_empty_streak(),
            ledger_refresh_interval_secs: default_ledger_refresh_interval(),
            custom_networks: default_custom_networks(),
            coin_type_override: default_coin_type(),
            default_remaining_balance: default_remaining_balance(),
            use_native_ledger: default_use_native_ledger(),
        }
    }
}

impl UserSettings {
    /// Get the settings file path
    fn settings_path() -> PathBuf {
        // Try to use the app data directory, fall back to current directory
        if let Some(config_dir) = dirs::config_dir() {
            let app_dir = config_dir.join("beaug");
            if !app_dir.exists() {
                let _ = fs::create_dir_all(&app_dir);
            }
            app_dir.join(SETTINGS_FILE)
        } else {
            PathBuf::from(SETTINGS_FILE)
        }
    }

    /// Load settings from disk, or return defaults if not found
    pub fn load() -> Self {
        let path = Self::settings_path();
        if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(settings) => {
                        tracing::info!("Loaded settings from {:?}", path);
                        return settings;
                    }
                    Err(e) => {
                        tracing::warn!("Failed to parse settings file: {}", e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read settings file: {}", e);
                }
            }
        }
        tracing::info!("Using default settings");
        Self::default()
    }

    /// Save settings to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::settings_path();
        let content = serde_json::to_string_pretty(self)?;
        fs::write(&path, content)?;
        tracing::info!("Saved settings to {:?}", path);
        Ok(())
    }

    /// Get custom RPC for a chain, or None if using default
    pub fn get_custom_rpc(&self, chain_id: u64) -> Option<&String> {
        self.custom_rpcs.get(&chain_id).filter(|s| !s.is_empty())
    }

    /// Set custom RPC for a chain (empty string removes the override)
    pub fn set_custom_rpc(&mut self, chain_id: u64, rpc: String) {
        if rpc.trim().is_empty() {
            self.custom_rpcs.remove(&chain_id);
        } else {
            self.custom_rpcs.insert(chain_id, rpc.trim().to_string());
        }
    }

    /// Get the settings file path for display
    pub fn settings_path_display() -> String {
        Self::settings_path().display().to_string()
    }

    /// Add a custom network (returns false if chain_id already exists)
    pub fn add_custom_network(&mut self, network: CustomNetwork) -> bool {
        // Check if chain_id already exists in custom networks
        if self.custom_networks.iter().any(|n| n.chain_id == network.chain_id) {
            return false;
        }
        self.custom_networks.push(network);
        true
    }

    /// Remove a custom network by chain_id
    pub fn remove_custom_network(&mut self, chain_id: u64) -> bool {
        let initial_len = self.custom_networks.len();
        self.custom_networks.retain(|n| n.chain_id != chain_id);
        self.custom_networks.len() < initial_len
    }

    /// Get a custom network by chain_id
    pub fn get_custom_network(&self, chain_id: u64) -> Option<&CustomNetwork> {
        self.custom_networks.iter().find(|n| n.chain_id == chain_id)
    }

    /// Update an existing custom network
    pub fn update_custom_network(&mut self, network: CustomNetwork) -> bool {
        if let Some(existing) = self.custom_networks.iter_mut().find(|n| n.chain_id == network.chain_id) {
            *existing = network;
            true
        } else {
            false
        }
    }

    /// Get the effective coin type (custom override or default 60)
    pub fn effective_coin_type(&self) -> u32 {
        self.coin_type_override.unwrap_or(crate::config::DEFAULT_COIN_TYPE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== CustomNetwork tests ====================

    #[test]
    fn test_custom_network_new() {
        let network = CustomNetwork::new(
            "Test Network".to_string(),
            12345,
            "TEST".to_string(),
            "https://rpc.test.com".to_string(),
        );
        assert_eq!(network.label, "Test Network");
        assert_eq!(network.chain_id, 12345);
        assert_eq!(network.native_token, "TEST");
        assert_eq!(network.rpc_url, "https://rpc.test.com");
    }

    #[test]
    fn test_custom_network_equality() {
        let network1 = CustomNetwork::new("Net".to_string(), 1, "ETH".to_string(), "https://a.com".to_string());
        let network2 = CustomNetwork::new("Net".to_string(), 1, "ETH".to_string(), "https://a.com".to_string());
        assert_eq!(network1, network2);
    }

    // ==================== UserSettings::default tests ====================

    #[test]
    fn test_user_settings_default_chain_id() {
        let settings = UserSettings::default();
        assert_eq!(settings.selected_chain_id, 11155111); // Sepolia
    }

    #[test]
    fn test_user_settings_default_values() {
        let settings = UserSettings::default();
        assert_eq!(settings.default_gas_speed, 1.0);
        assert_eq!(settings.default_tip_percentage, 0.0);
        assert_eq!(settings.default_split_outputs, 5);
        assert_eq!(settings.default_bulk_outputs, 10);
        assert_eq!(settings.default_scan_start_index, 0);
        assert_eq!(settings.default_scan_empty_streak, 5);
        assert_eq!(settings.ledger_refresh_interval_secs, 5);
        assert!(settings.custom_networks.is_empty());
        assert!(settings.coin_type_override.is_none());
        assert_eq!(settings.default_remaining_balance, 0);
    }

    // ==================== add_custom_network tests ====================

    #[test]
    fn test_add_custom_network_success() {
        let mut settings = UserSettings::default();
        let network = CustomNetwork::new("Test".to_string(), 99999, "TST".to_string(), "https://test.com".to_string());
        
        let result = settings.add_custom_network(network.clone());
        
        assert!(result);
        assert_eq!(settings.custom_networks.len(), 1);
        assert_eq!(settings.custom_networks[0], network);
    }

    #[test]
    fn test_add_custom_network_duplicate_chain_id_fails() {
        let mut settings = UserSettings::default();
        let network1 = CustomNetwork::new("Test1".to_string(), 99999, "TST".to_string(), "https://test1.com".to_string());
        let network2 = CustomNetwork::new("Test2".to_string(), 99999, "TST2".to_string(), "https://test2.com".to_string());
        
        settings.add_custom_network(network1);
        let result = settings.add_custom_network(network2);
        
        assert!(!result);
        assert_eq!(settings.custom_networks.len(), 1);
        assert_eq!(settings.custom_networks[0].label, "Test1"); // Original unchanged
    }

    // ==================== remove_custom_network tests ====================

    #[test]
    fn test_remove_custom_network_existing() {
        let mut settings = UserSettings::default();
        let network = CustomNetwork::new("Test".to_string(), 99999, "TST".to_string(), "https://test.com".to_string());
        settings.add_custom_network(network);
        
        let result = settings.remove_custom_network(99999);
        
        assert!(result);
        assert!(settings.custom_networks.is_empty());
    }

    #[test]
    fn test_remove_custom_network_non_existing() {
        let mut settings = UserSettings::default();
        
        let result = settings.remove_custom_network(99999);
        
        assert!(!result);
    }

    // ==================== get_custom_network tests ====================

    #[test]
    fn test_get_custom_network_found() {
        let mut settings = UserSettings::default();
        let network = CustomNetwork::new("Test".to_string(), 99999, "TST".to_string(), "https://test.com".to_string());
        settings.add_custom_network(network.clone());
        
        let result = settings.get_custom_network(99999);
        
        assert!(result.is_some());
        assert_eq!(result.unwrap(), &network);
    }

    #[test]
    fn test_get_custom_network_not_found() {
        let settings = UserSettings::default();
        
        let result = settings.get_custom_network(99999);
        
        assert!(result.is_none());
    }

    // ==================== update_custom_network tests ====================

    #[test]
    fn test_update_custom_network_existing() {
        let mut settings = UserSettings::default();
        let network = CustomNetwork::new("Original".to_string(), 99999, "TST".to_string(), "https://test.com".to_string());
        settings.add_custom_network(network);
        
        let updated = CustomNetwork::new("Updated".to_string(), 99999, "NEW".to_string(), "https://new.com".to_string());
        let result = settings.update_custom_network(updated.clone());
        
        assert!(result);
        assert_eq!(settings.custom_networks.len(), 1);
        assert_eq!(settings.custom_networks[0].label, "Updated");
        assert_eq!(settings.custom_networks[0].native_token, "NEW");
    }

    #[test]
    fn test_update_custom_network_non_existing() {
        let mut settings = UserSettings::default();
        let network = CustomNetwork::new("Test".to_string(), 99999, "TST".to_string(), "https://test.com".to_string());
        
        let result = settings.update_custom_network(network);
        
        assert!(!result);
        assert!(settings.custom_networks.is_empty());
    }

    // ==================== get_custom_rpc / set_custom_rpc tests ====================

    #[test]
    fn test_set_and_get_custom_rpc() {
        let mut settings = UserSettings::default();
        
        settings.set_custom_rpc(1, "https://my-eth-node.com".to_string());
        
        let rpc = settings.get_custom_rpc(1);
        assert!(rpc.is_some());
        assert_eq!(rpc.unwrap(), "https://my-eth-node.com");
    }

    #[test]
    fn test_get_custom_rpc_not_set() {
        let settings = UserSettings::default();
        
        let rpc = settings.get_custom_rpc(1);
        
        assert!(rpc.is_none());
    }

    #[test]
    fn test_set_custom_rpc_empty_removes() {
        let mut settings = UserSettings::default();
        settings.set_custom_rpc(1, "https://my-eth-node.com".to_string());
        
        settings.set_custom_rpc(1, "".to_string());
        
        assert!(settings.get_custom_rpc(1).is_none());
    }

    #[test]
    fn test_set_custom_rpc_whitespace_removes() {
        let mut settings = UserSettings::default();
        settings.set_custom_rpc(1, "https://my-eth-node.com".to_string());
        
        settings.set_custom_rpc(1, "   ".to_string());
        
        assert!(settings.get_custom_rpc(1).is_none());
    }

    #[test]
    fn test_set_custom_rpc_trims_whitespace() {
        let mut settings = UserSettings::default();
        
        settings.set_custom_rpc(1, "  https://my-eth-node.com  ".to_string());
        
        let rpc = settings.get_custom_rpc(1);
        assert_eq!(rpc.unwrap(), "https://my-eth-node.com");
    }

    // ==================== effective_coin_type tests ====================

    #[test]
    fn test_effective_coin_type_default() {
        let settings = UserSettings::default();
        
        assert_eq!(settings.effective_coin_type(), 60); // DEFAULT_COIN_TYPE
    }

    #[test]
    fn test_effective_coin_type_with_override() {
        let mut settings = UserSettings::default();
        settings.coin_type_override = Some(714); // BNB coin type
        
        assert_eq!(settings.effective_coin_type(), 714);
    }
}
