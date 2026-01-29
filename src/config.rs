use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::env;
use std::sync::Arc;
use url::Url;
use ethers::providers::{Http, Provider};

/// Network category for grouping in the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkCategory {
    EthereumMainnet,
    EthereumTestnet,
    L2Mainnet,
    OtherMainnet,
    L2Testnet,
}

/// A predefined EVM-compatible network with label, chain ID, native token, and default RPC.
#[derive(Clone, Debug)]
pub struct EvmNetwork {
    pub label: &'static str,
    pub chain_id: u64,
    pub native_token: &'static str,
    pub default_rpc: &'static str,
    pub category: NetworkCategory,
}

impl EvmNetwork {
    pub const fn new(
        label: &'static str,
        chain_id: u64,
        native_token: &'static str,
        default_rpc: &'static str,
        category: NetworkCategory,
    ) -> Self {
        Self {
            label,
            chain_id,
            native_token,
            default_rpc,
            category,
        }
    }
}

use NetworkCategory::*;

/// Comprehensive list of major EVM networks.
pub const NETWORKS: &[EvmNetwork] = &[
    EvmNetwork::new("Ethereum", 1, "ETH", "https://ethereum-rpc.publicnode.com", EthereumMainnet),
    EvmNetwork::new("Sepolia", 11155111, "ETH", "https://ethereum-sepolia-rpc.publicnode.com", EthereumTestnet),
    EvmNetwork::new("Hoodi", 560048, "ETH", "https://rpc.hoodi.ethpandaops.io", EthereumTestnet),
    EvmNetwork::new("Optimism", 10, "ETH", "https://mainnet.optimism.io", L2Mainnet),
    EvmNetwork::new("Base", 8453, "ETH", "https://mainnet.base.org", L2Mainnet),
    EvmNetwork::new("Polygon", 137, "POL", "https://polygon-rpc.com", L2Mainnet),
    EvmNetwork::new("Linea", 59144, "ETH", "https://rpc.linea.build", L2Mainnet),
    EvmNetwork::new("Gnosis Chain", 100, "xDAI", "https://rpc.gnosischain.com", L2Mainnet),
    EvmNetwork::new("BNB Chain", 56, "BNB", "https://bsc-dataseed.binance.org", OtherMainnet),
    EvmNetwork::new("Avalanche C-Chain", 43114, "AVAX", "https://avalanche-c-chain-rpc.publicnode.com", OtherMainnet),
    EvmNetwork::new("Ethereum Classic", 61, "ETC", "https://etc.rivet.link", OtherMainnet),
    EvmNetwork::new("Pulsechain", 369, "PLS", "https://rpc.pulsechain.com", OtherMainnet),
    EvmNetwork::new("Celo", 42220, "CELO", "https://forno.celo.org", OtherMainnet),
];

/// Find a network by chain ID
pub fn find_network_by_chain_id(chain_id: u64) -> Option<&'static EvmNetwork> {
    NETWORKS.iter().find(|n| n.chain_id == chain_id)
}

/// Find the index of a network in NETWORKS by chain ID
pub fn find_network_index(chain_id: u64) -> Option<usize> {
    NETWORKS.iter().position(|n| n.chain_id == chain_id)
}

/// Check if a chain ID is used by a built-in network
pub fn is_builtin_chain_id(chain_id: u64) -> bool {
    NETWORKS.iter().any(|n| n.chain_id == chain_id)
}

/// Get the block explorer URL for a given chain ID
/// Returns the base URL for transaction/address lookups
pub fn get_block_explorer_url(chain_id: u64) -> Option<&'static str> {
    match chain_id {
        // Ethereum and testnets
        1 => Some("https://etherscan.io"),
        11155111 => Some("https://sepolia.etherscan.io"),
        560048 => Some("https://hoodi.ethpandaops.io"), // Hoodi explorer
        // L2s
        10 => Some("https://optimistic.etherscan.io"),
        8453 => Some("https://basescan.org"),
        137 => Some("https://polygonscan.com"),
        59144 => Some("https://lineascan.build"),
        100 => Some("https://gnosisscan.io"),
        // Other mainnets
        56 => Some("https://bscscan.com"),
        43114 => Some("https://snowtrace.io"),
        61 => Some("https://etc.blockscout.com"),
        369 => Some("https://scan.pulsechain.com"),
        42220 => Some("https://celoscan.io"),
        _ => None,
    }
}

/// Get the full URL to view a transaction on the block explorer
pub fn get_tx_explorer_url(chain_id: u64, tx_hash: &str) -> Option<String> {
    get_block_explorer_url(chain_id).map(|base| format!("{}/tx/{}", base, tx_hash))
}

/// Get the full URL to view an address on the block explorer
pub fn get_address_explorer_url(chain_id: u64, address: &str) -> Option<String> {
    get_block_explorer_url(chain_id).map(|base| format!("{}/address/{}", base, address))
}

/// Default BIP-44 coin type for Ethereum (used for all EVM chains for compatibility)
pub const DEFAULT_COIN_TYPE: u32 = 60;

/// SLIP-44 registered coin types for specific chains
/// Note: Using these will result in different addresses than MetaMask/Ledger Live
/// which use coin type 60 for all EVM chains
pub fn get_slip44_coin_type(chain_id: u64) -> u32 {
    match chain_id {
        1 => 60,        // Ethereum Mainnet
        61 => 61,       // Ethereum Classic (note: chain_id 61, coin_type 61)
        56 => 714,      // BNB Chain (BNB)
        137 => 966,     // Polygon (MATIC)
        43114 => 9005,  // Avalanche C-Chain
        250 => 1007,    // Fantom Opera
        // L2s and most EVM chains use Ethereum's coin type for compatibility
        _ => DEFAULT_COIN_TYPE,
    }
}

/// Check if a chain supports EIP-1559 (Type 2 transactions)
/// Returns true for chains that support dynamic fee transactions
pub fn chain_supports_eip1559(chain_id: u64) -> bool {
    match chain_id {
        // Ethereum and its testnets (post-London upgrade)
        1 | 11155111 | 560048 => true,
        // Major L2s that support EIP-1559
        10 => true,     // Optimism
        8453 => true,   // Base
        137 => true,    // Polygon (post-London)
        59144 => true,  // Linea
        100 => true,    // Gnosis Chain
        43114 => true,  // Avalanche C-Chain
        42220 => true,  // Celo
        369 => true,    // Pulsechain
        // Chains that do NOT support EIP-1559 (use legacy gas pricing)
        56 => false,    // BNB Chain
        61 => false,    // Ethereum Classic
        250 => false,   // Fantom Opera
        // Default to true for unknown chains (most modern chains support it)
        _ => true,
    }
}

/// Get recommended gas speed range for a chain
/// Returns (min_recommended, default, max_recommended)
pub fn recommended_gas_speed_range(chain_id: u64) -> (f32, f32, f32) {
    match chain_id {
        // Ethereum mainnet - volatile, be conservative
        1 => (0.9, 1.0, 1.5),
        // L2s - usually cheap and fast, don't need high multipliers
        10 | 8453 | 59144 | 100 => (0.9, 1.0, 1.2),
        // Polygon - cheap, can be more aggressive
        137 => (1.0, 1.1, 1.5),
        // BNB Chain - cheap, fixed pricing
        56 => (1.0, 1.0, 1.2),
        // Testnets - be generous to ensure inclusion
        11155111 | 560048 => (1.0, 1.1, 1.5),
        // Default
        _ => (0.9, 1.0, 1.5),
    }
}

/// Get a description of gas characteristics for a chain
pub fn chain_gas_description(chain_id: u64) -> &'static str {
    match chain_id {
        1 => "High fees, volatile pricing. Use Standard for normal priority.",
        10 | 8453 | 59144 | 100 => "Low fees. Standard speed is usually sufficient.",
        137 => "Very low fees. Can use higher speeds without significant cost.",
        56 => "Low fixed fees. Speed has minimal impact on cost.",
        11155111 | 560048 => "Testnet - gas is free but may need higher speed for inclusion.",
        369 => "Low fees similar to Ethereum pre-merge.",
        _ => "Standard speed (1.0x) recommended for most transactions.",
    }
}

/// Derivation path mode for HD wallets
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DerivationMode {
    /// Account-index: m/44'/60'/i'/0/0 - account number varies (Ledger Live, MetaMask default)
    #[serde(alias = "AccountBased")]  // Backwards compatibility with saved settings
    AccountIndex,
    /// Address-index: m/44'/60'/0'/0/i - address index varies, account fixed
    AddressIndex,
}

impl Default for DerivationMode {
    fn default() -> Self {
        DerivationMode::AccountIndex
    }
}

impl DerivationMode {
    /// Get the derivation path for this mode
    /// coin_type: BIP-44 coin type (60 for Ethereum, 714 for BNB, etc.)
    pub fn get_path(&self, index: u32, custom_account: u32, custom_address_index: u32, coin_type: u32) -> String {
        match self {
            DerivationMode::AccountIndex => {
                // Account number is the index, address index is fixed
                format!("m/44'/{}'/{}'/0/{}", coin_type, index, custom_address_index)
            }
            DerivationMode::AddressIndex => {
                // Account is fixed, address index is the index
                format!("m/44'/{}'/{}'/0/{}", coin_type, custom_account, index)
            }
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub rpc_url: String,
    pub chain_id: u64,
    pub gas_speed_multiplier: f32,  // Gas price multiplier (0.8=Slow, 1.0=Standard, 1.5=Fast, 2.0+=Aggressive)
    pub derivation_mode: DerivationMode,
    pub custom_account: u32,  // Used in AddressIndex mode - constant account number
    pub custom_address_index: u32,  // Used in AccountIndex mode - constant address index
    pub coin_type: u32,  // BIP-44 coin type (default 60 for Ethereum compatibility)
    pub export_directory: String,  // Directory for saving exported files
    // Overrides for custom networks
    pub native_token_override: Option<String>,
    pub label_override: Option<String>,
}

impl Config {
    pub fn new(rpc_url: String, chain_id: u64) -> Self {
        let gas_speed_multiplier = env::var("GAS_SPEED_MULTIPLIER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1.0); // Normal speed by default

        // Default export directory to user's documents or current directory
        let export_directory = env::var("USERPROFILE")
            .or_else(|_| env::var("HOME"))
            .ok()
            .map(|home| {
                let mut path = std::path::PathBuf::from(home);
                path.push("Documents");
                path.push("Beaug");
                path.to_string_lossy().to_string()
            })
            .unwrap_or_else(|| ".".to_string());

        Self {
            rpc_url,
            chain_id,
            gas_speed_multiplier,
            derivation_mode: DerivationMode::default(),
            custom_account: 0,
            custom_address_index: 0,
            coin_type: DEFAULT_COIN_TYPE,
            export_directory,
            native_token_override: None,
            label_override: None,
        }
    }

    pub fn from_network(network: &EvmNetwork) -> Self {
        Self::new(network.default_rpc.to_string(), network.chain_id)
    }

    /// Create config from a custom network
    pub fn from_custom_network(network: &crate::user_settings::CustomNetwork) -> Self {
        let mut config = Self::new(network.rpc_url.clone(), network.chain_id);
        config.native_token_override = Some(network.native_token.clone());
        config.label_override = Some(network.label.clone());
        config
    }
    
    /// Get the derivation path for a given index using the current mode
    pub fn get_derivation_path(&self, index: u32) -> String {
        self.derivation_mode.get_path(index, self.custom_account, self.custom_address_index, self.coin_type)
    }

    pub fn native_token(&self) -> &str {
        if let Some(ref token) = self.native_token_override {
            token.as_str()
        } else {
            find_network_by_chain_id(self.chain_id)
                .map(|n| n.native_token)
                .unwrap_or("ETH")
        }
    }

    pub fn network_label(&self) -> &str {
        if let Some(ref label) = self.label_override {
            label.as_str()
        } else {
            find_network_by_chain_id(self.chain_id)
                .map(|n| n.label)
                .unwrap_or("Unknown")
        }
    }

    pub async fn get_provider(&self) -> Result<Arc<Provider<Http>>> {
        let url = Url::parse(&self.rpc_url)?;
        let provider = Provider::<Http>::try_from(url.as_str())?;
        Ok(Arc::new(provider))
    }
}

impl Default for Config {
    fn default() -> Self {
        // Default to Sepolia testnet - GUI will load user settings and update
        if let Some(sepolia) = find_network_by_chain_id(11155111) {
            Self::from_network(sepolia)
        } else {
            Self::new("https://rpc.sepolia.org".to_string(), 11155111)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ==================== find_network_by_chain_id tests ====================

    #[test]
    fn test_find_network_by_chain_id_ethereum() {
        let network = find_network_by_chain_id(1);
        assert!(network.is_some());
        let network = network.unwrap();
        assert_eq!(network.label, "Ethereum");
        assert_eq!(network.native_token, "ETH");
    }

    #[test]
    fn test_find_network_by_chain_id_sepolia() {
        let network = find_network_by_chain_id(11155111);
        assert!(network.is_some());
        assert_eq!(network.unwrap().label, "Sepolia");
    }

    #[test]
    fn test_find_network_by_chain_id_not_found() {
        let network = find_network_by_chain_id(999999);
        assert!(network.is_none());
    }

    // ==================== find_network_index tests ====================

    #[test]
    fn test_find_network_index_ethereum() {
        let index = find_network_index(1);
        assert!(index.is_some());
        assert_eq!(index.unwrap(), 0); // Ethereum is first in the list
    }

    #[test]
    fn test_find_network_index_not_found() {
        let index = find_network_index(999999);
        assert!(index.is_none());
    }

    // ==================== is_builtin_chain_id tests ====================

    #[test]
    fn test_is_builtin_chain_id_true() {
        assert!(is_builtin_chain_id(1));       // Ethereum
        assert!(is_builtin_chain_id(137));     // Polygon
        assert!(is_builtin_chain_id(100));     // Gnosis Chain
    }

    #[test]
    fn test_is_builtin_chain_id_false() {
        assert!(!is_builtin_chain_id(999999));
        assert!(!is_builtin_chain_id(31337));  // Hardhat local
    }

    // ==================== get_slip44_coin_type tests ====================

    #[test]
    fn test_get_slip44_coin_type_ethereum() {
        assert_eq!(get_slip44_coin_type(1), 60);
    }

    #[test]
    fn test_get_slip44_coin_type_ethereum_classic() {
        assert_eq!(get_slip44_coin_type(61), 61);
    }

    #[test]
    fn test_get_slip44_coin_type_bnb() {
        assert_eq!(get_slip44_coin_type(56), 714);
    }

    #[test]
    fn test_get_slip44_coin_type_polygon() {
        assert_eq!(get_slip44_coin_type(137), 966);
    }

    #[test]
    fn test_get_slip44_coin_type_unknown_defaults_to_60() {
        assert_eq!(get_slip44_coin_type(999999), DEFAULT_COIN_TYPE);
        assert_eq!(get_slip44_coin_type(59144), DEFAULT_COIN_TYPE); // Linea uses default
    }

    // ==================== chain_supports_eip1559 tests ====================

    #[test]
    fn test_chain_supports_eip1559_ethereum_mainnet() {
        assert!(chain_supports_eip1559(1));
    }

    #[test]
    fn test_chain_supports_eip1559_testnets() {
        assert!(chain_supports_eip1559(11155111)); // Sepolia
        assert!(chain_supports_eip1559(560048));   // Hoodi
    }

    #[test]
    fn test_chain_supports_eip1559_l2s() {
        assert!(chain_supports_eip1559(10));     // Optimism
        assert!(chain_supports_eip1559(8453));   // Base
        assert!(chain_supports_eip1559(59144));  // Linea
        assert!(chain_supports_eip1559(100));    // Gnosis Chain
    }

    #[test]
    fn test_chain_supports_eip1559_legacy_chains() {
        assert!(!chain_supports_eip1559(56));   // BNB Chain
        assert!(!chain_supports_eip1559(61));   // Ethereum Classic
        assert!(!chain_supports_eip1559(250));  // Fantom
    }

    #[test]
    fn test_chain_supports_eip1559_unknown_defaults_true() {
        assert!(chain_supports_eip1559(999999));
    }

    // ==================== recommended_gas_speed_range tests ====================

    #[test]
    fn test_recommended_gas_speed_range_ethereum() {
        let (min, default, max) = recommended_gas_speed_range(1);
        assert_eq!(min, 0.9);
        assert_eq!(default, 1.0);
        assert_eq!(max, 1.5);
    }

    #[test]
    fn test_recommended_gas_speed_range_l2() {
        let (min, default, max) = recommended_gas_speed_range(59144); // Linea
        assert_eq!(min, 0.9);
        assert_eq!(default, 1.0);
        assert_eq!(max, 1.2);
    }

    #[test]
    fn test_recommended_gas_speed_range_testnet() {
        let (min, default, max) = recommended_gas_speed_range(11155111); // Sepolia
        assert_eq!(min, 1.0);
        assert_eq!(default, 1.1);
        assert_eq!(max, 1.5);
    }

    // ==================== chain_gas_description tests ====================

    #[test]
    fn test_chain_gas_description_ethereum() {
        let desc = chain_gas_description(1);
        assert!(desc.contains("High fees"));
    }

    #[test]
    fn test_chain_gas_description_testnet() {
        let desc = chain_gas_description(11155111);
        assert!(desc.contains("Testnet"));
    }

    #[test]
    fn test_chain_gas_description_unknown() {
        let desc = chain_gas_description(999999);
        assert!(desc.contains("Standard speed"));
    }

    // ==================== DerivationMode::get_path tests ====================

    #[test]
    fn test_derivation_mode_account_index() {
        let mode = DerivationMode::AccountIndex;
        // m/44'/60'/index'/0/custom_address_index
        let path = mode.get_path(5, 0, 0, 60);
        assert_eq!(path, "m/44'/60'/5'/0/0");
    }

    #[test]
    fn test_derivation_mode_account_index_with_custom_address() {
        let mode = DerivationMode::AccountIndex;
        let path = mode.get_path(3, 0, 2, 60);
        assert_eq!(path, "m/44'/60'/3'/0/2");
    }

    #[test]
    fn test_derivation_mode_address_index() {
        let mode = DerivationMode::AddressIndex;
        // m/44'/60'/custom_account'/0/index
        let path = mode.get_path(5, 0, 0, 60);
        assert_eq!(path, "m/44'/60'/0'/0/5");
    }

    #[test]
    fn test_derivation_mode_address_index_with_custom_account() {
        let mode = DerivationMode::AddressIndex;
        let path = mode.get_path(7, 2, 0, 60);
        assert_eq!(path, "m/44'/60'/2'/0/7");
    }

    #[test]
    fn test_derivation_mode_different_coin_type() {
        let mode = DerivationMode::AccountIndex;
        let path = mode.get_path(0, 0, 0, 714); // BNB coin type
        assert_eq!(path, "m/44'/714'/0'/0/0");
    }

    #[test]
    fn test_derivation_mode_default() {
        assert_eq!(DerivationMode::default(), DerivationMode::AccountIndex);
    }

    // ==================== Config tests ====================

    #[test]
    fn test_config_native_token_builtin() {
        let config = Config::new("https://ethereum-rpc.publicnode.com".to_string(), 1);
        assert_eq!(config.native_token(), "ETH");
    }

    #[test]
    fn test_config_native_token_polygon() {
        let config = Config::new("https://polygon-rpc.com".to_string(), 137);
        assert_eq!(config.native_token(), "POL");
    }

    #[test]
    fn test_config_native_token_override() {
        let mut config = Config::new("https://example.com".to_string(), 999999);
        config.native_token_override = Some("CUSTOM".to_string());
        assert_eq!(config.native_token(), "CUSTOM");
    }

    #[test]
    fn test_config_native_token_unknown_defaults_to_eth() {
        let config = Config::new("https://example.com".to_string(), 999999);
        assert_eq!(config.native_token(), "ETH");
    }

    #[test]
    fn test_config_network_label_builtin() {
        let config = Config::new("https://ethereum-rpc.publicnode.com".to_string(), 1);
        assert_eq!(config.network_label(), "Ethereum");
    }

    #[test]
    fn test_config_network_label_override() {
        let mut config = Config::new("https://example.com".to_string(), 999999);
        config.label_override = Some("My Custom Chain".to_string());
        assert_eq!(config.network_label(), "My Custom Chain");
    }

    #[test]
    fn test_config_network_label_unknown() {
        let config = Config::new("https://example.com".to_string(), 999999);
        assert_eq!(config.network_label(), "Unknown");
    }

    #[test]
    fn test_config_get_derivation_path() {
        let config = Config::new("https://ethereum-rpc.publicnode.com".to_string(), 1);
        let path = config.get_derivation_path(0);
        assert_eq!(path, "m/44'/60'/0'/0/0");
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.chain_id, 11155111); // Sepolia
        assert_eq!(config.derivation_mode, DerivationMode::AccountIndex);
        assert_eq!(config.coin_type, DEFAULT_COIN_TYPE);
    }

    #[test]
    fn test_config_from_network() {
        let network = find_network_by_chain_id(137).unwrap();
        let config = Config::from_network(network);
        assert_eq!(config.chain_id, 137);
        assert_eq!(config.rpc_url, "https://polygon-rpc.com");
    }
}