use ethers::types::U256;
use anyhow::{anyhow, Result};

pub fn format_ether(wei: U256) -> String {
    ethers::utils::format_units(wei, "ether").unwrap_or_else(|_| "0.0".to_string())
}

/// Convert ETH amount (as f64) to Wei
/// 
/// # Errors
/// Returns an error if the conversion fails (e.g., negative values, overflow)
/// 
/// # Note
/// For user input, prefer `parse_eth_str_to_wei` which avoids float precision issues
pub fn eth_to_wei(eth: f64) -> Result<U256> {
    if eth < 0.0 {
        return Err(anyhow!("ETH amount cannot be negative: {}", eth));
    }
    ethers::utils::parse_units(eth, "ether")
        .map(|pu| pu.into())
        .map_err(|e| anyhow!("Failed to convert {} ETH to wei: {}", eth, e))
}

/// Parse a string representing ETH to Wei
/// 
/// This is preferred over `eth_to_wei(f64)` for user input as it:
/// - Handles decimal strings directly without f64 precision loss
/// - Validates input format properly
/// 
/// # Errors
/// Returns an error if the string is not a valid decimal number or conversion fails
pub fn parse_eth_str_to_wei(input: &str) -> Result<U256> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("ETH amount cannot be empty"));
    }
    
    // Parse as decimal string directly to avoid float precision issues
    ethers::utils::parse_ether(trimmed)
        .map_err(|e| anyhow!("Invalid ETH amount '{}': {}", trimmed, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_ether_zero() {
        let wei = U256::zero();
        let result = format_ether(wei);
        assert_eq!(result, "0.000000000000000000");
    }

    #[test]
    fn test_format_ether_one_eth() {
        // 1 ETH = 10^18 wei
        let wei = U256::from(10u64.pow(18));
        let result = format_ether(wei);
        assert_eq!(result, "1.000000000000000000");
    }

    #[test]
    fn test_format_ether_fractional() {
        // 0.5 ETH = 5 * 10^17 wei
        let wei = U256::from(5u64) * U256::from(10u64.pow(17));
        let result = format_ether(wei);
        assert_eq!(result, "0.500000000000000000");
    }

    // ==================== eth_to_wei tests ====================

    #[test]
    fn test_eth_to_wei_zero() {
        let result = eth_to_wei(0.0).unwrap();
        assert_eq!(result, U256::zero());
    }

    #[test]
    fn test_eth_to_wei_one_eth() {
        let result = eth_to_wei(1.0).unwrap();
        let expected = U256::from(10u64.pow(18));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_eth_to_wei_fractional() {
        let result = eth_to_wei(0.5).unwrap();
        let expected = U256::from(5u64) * U256::from(10u64.pow(17));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_eth_to_wei_large_value() {
        let result = eth_to_wei(100.0).unwrap();
        let expected = U256::from(100u64) * U256::from(10u64.pow(18));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_eth_to_wei_negative_fails() {
        let result = eth_to_wei(-1.0);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("negative"));
    }

    // ==================== parse_eth_str_to_wei tests ====================

    #[test]
    fn test_parse_eth_str_to_wei_zero() {
        let result = parse_eth_str_to_wei("0").unwrap();
        assert_eq!(result, U256::zero());
    }

    #[test]
    fn test_parse_eth_str_to_wei_one_eth() {
        let result = parse_eth_str_to_wei("1").unwrap();
        let expected = U256::from(10u64.pow(18));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_eth_str_to_wei_fractional() {
        let result = parse_eth_str_to_wei("0.5").unwrap();
        let expected = U256::from(5u64) * U256::from(10u64.pow(17));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_eth_str_to_wei_with_whitespace() {
        let result = parse_eth_str_to_wei("  1.5  ").unwrap();
        let expected = U256::from(15u64) * U256::from(10u64.pow(17));
        assert_eq!(result, expected);
    }

    #[test]
    fn test_parse_eth_str_to_wei_empty_fails() {
        let result = parse_eth_str_to_wei("");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn test_parse_eth_str_to_wei_invalid_fails() {
        let result = parse_eth_str_to_wei("abc");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_eth_str_to_wei_high_precision() {
        // Test that string parsing preserves precision better than f64
        let result = parse_eth_str_to_wei("0.123456789012345678").unwrap();
        // Should get exactly 123456789012345678 wei
        let expected = U256::from(123456789012345678u64);
        assert_eq!(result, expected);
    }
}
