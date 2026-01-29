//! Helper functions and constants for the GUI
//!
//! Contains utility functions for gas calculations, formatting, and icon loading.

use eframe::egui;

// Load the logo at compile time
pub static BEAUG_LOGO_WEBP: &[u8] = include_bytes!("../beaug1.png");

// Load the window icon at compile time
pub static BEAUG_ICON_PNG: &[u8] = include_bytes!("../beaug-icon.png");

/// Helper to get a label for gas speed values (industry-standard terminology)
pub fn gas_speed_label(speed: f32) -> &'static str {
    if speed < 0.9 {
        "Slow"
    } else if speed < 1.2 {
        "Standard"
    } else if speed < 1.8 {
        "Fast"
    } else {
        "Aggressive"
    }
}

/// Helper to get an emoji indicator for gas speed
pub fn gas_speed_emoji(speed: f32) -> &'static str {
    if speed < 0.9 {
        "🐢"
    } else if speed < 1.2 {
        "⚡"
    } else if speed < 1.8 {
        "🚀"
    } else {
        "⚡⚡"
    }
}

/// Get a warning message for extreme gas speed values, if any
pub fn gas_speed_warning(speed: f32) -> Option<&'static str> {
    if speed < 0.85 {
        Some("⚠ Very low: Transaction may be stuck for hours or days")
    } else if speed > 2.0 {
        Some("⚠ Very high: You may significantly overpay for gas")
    } else {
        None
    }
}

/// Format gas price in Gwei from wei
pub fn format_gwei(wei: ethers::types::U256) -> String {
    let gwei = ethers::utils::format_units(wei, "gwei").unwrap_or_else(|_| "?".to_string());
    // Trim to reasonable precision
    if let Some(dot_pos) = gwei.find('.') {
        let decimals = gwei.len() - dot_pos - 1;
        if decimals > 2 {
            format!("{:.2}", gwei.parse::<f64>().unwrap_or(0.0))
        } else {
            gwei
        }
    } else {
        gwei
    }
}

/// Calculate gas limit for disperse contract calls
/// Uses a formula based on recipient count with a 10% safety buffer
pub fn calculate_disperse_gas_limit(recipient_count: usize) -> u64 {
    // Base gas: 150k for function call overhead
    // Per recipient: 120k (transfer + storage writes + events)
    // Safety buffer: 10%
    // Max cap: 15M (typical block gas limit)
    let base_gas_limit = 150_000u64 + 120_000u64 * recipient_count as u64;
    let with_buffer = (base_gas_limit as f64 * 1.10) as u64;
    with_buffer.min(15_000_000u64)
}

/// Load the application icon for the window
pub fn load_icon() -> Option<egui::IconData> {
    let img = image::load_from_memory(BEAUG_ICON_PNG).ok()?.into_rgba8();
    let (width, height) = img.dimensions();
    Some(egui::IconData {
        rgba: img.into_raw(),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethers::types::U256;

    // ==================== gas_speed_label tests ====================

    #[test]
    fn test_gas_speed_label_slow() {
        assert_eq!(gas_speed_label(0.5), "Slow");
        assert_eq!(gas_speed_label(0.89), "Slow");
    }

    #[test]
    fn test_gas_speed_label_standard() {
        assert_eq!(gas_speed_label(0.9), "Standard");
        assert_eq!(gas_speed_label(1.0), "Standard");
        assert_eq!(gas_speed_label(1.19), "Standard");
    }

    #[test]
    fn test_gas_speed_label_fast() {
        assert_eq!(gas_speed_label(1.2), "Fast");
        assert_eq!(gas_speed_label(1.5), "Fast");
        assert_eq!(gas_speed_label(1.79), "Fast");
    }

    #[test]
    fn test_gas_speed_label_aggressive() {
        assert_eq!(gas_speed_label(1.8), "Aggressive");
        assert_eq!(gas_speed_label(2.5), "Aggressive");
    }

    // ==================== gas_speed_emoji tests ====================

    #[test]
    fn test_gas_speed_emoji_slow() {
        assert_eq!(gas_speed_emoji(0.5), "🐢");
        assert_eq!(gas_speed_emoji(0.89), "🐢");
    }

    #[test]
    fn test_gas_speed_emoji_standard() {
        assert_eq!(gas_speed_emoji(0.9), "⚡");
        assert_eq!(gas_speed_emoji(1.0), "⚡");
        assert_eq!(gas_speed_emoji(1.19), "⚡");
    }

    #[test]
    fn test_gas_speed_emoji_fast() {
        assert_eq!(gas_speed_emoji(1.2), "🚀");
        assert_eq!(gas_speed_emoji(1.5), "🚀");
        assert_eq!(gas_speed_emoji(1.79), "🚀");
    }

    #[test]
    fn test_gas_speed_emoji_aggressive() {
        assert_eq!(gas_speed_emoji(1.8), "⚡⚡");
        assert_eq!(gas_speed_emoji(2.5), "⚡⚡");
    }

    // ==================== gas_speed_warning tests ====================

    #[test]
    fn test_gas_speed_warning_very_low() {
        assert!(gas_speed_warning(0.84).is_some());
        assert!(gas_speed_warning(0.84).unwrap().contains("Very low"));
    }

    #[test]
    fn test_gas_speed_warning_no_warning_low_boundary() {
        assert!(gas_speed_warning(0.85).is_none());
        assert!(gas_speed_warning(1.0).is_none());
    }

    #[test]
    fn test_gas_speed_warning_no_warning_high_boundary() {
        assert!(gas_speed_warning(2.0).is_none());
    }

    #[test]
    fn test_gas_speed_warning_very_high() {
        assert!(gas_speed_warning(2.01).is_some());
        assert!(gas_speed_warning(2.01).unwrap().contains("Very high"));
    }

    // ==================== format_gwei tests ====================

    #[test]
    fn test_format_gwei_zero() {
        let result = format_gwei(U256::zero());
        // format_units returns "0.000000000" which has >2 decimals, so trimmed to 2
        assert_eq!(result, "0.00");
    }

    #[test]
    fn test_format_gwei_one_gwei() {
        // 1 Gwei = 10^9 wei
        let one_gwei = U256::from(10u64.pow(9));
        let result = format_gwei(one_gwei);
        assert_eq!(result, "1.00");
    }

    #[test]
    fn test_format_gwei_whole_number() {
        // 50 Gwei
        let fifty_gwei = U256::from(50u64) * U256::from(10u64.pow(9));
        let result = format_gwei(fifty_gwei);
        assert_eq!(result, "50.00");
    }

    #[test]
    fn test_format_gwei_precision_trimming() {
        // 1.123456789 Gwei should be trimmed to 2 decimals
        let wei = U256::from(1_123_456_789u64);
        let result = format_gwei(wei);
        assert_eq!(result, "1.12");
    }

    #[test]
    fn test_format_gwei_fractional() {
        // 1.5 Gwei = 1_500_000_000 wei, will be formatted and trimmed to 2 decimals
        let wei = U256::from(1_500_000_000u64);
        let result = format_gwei(wei);
        assert_eq!(result, "1.50");
    }

    // ==================== calculate_disperse_gas_limit tests ====================

    #[test]
    fn test_calculate_disperse_gas_limit_one_recipient() {
        // Base: 150k + 120k * 1 = 270k, with 10% buffer = 297k
        let result = calculate_disperse_gas_limit(1);
        assert_eq!(result, 297_000);
    }

    #[test]
    fn test_calculate_disperse_gas_limit_ten_recipients() {
        // Base: 150k + 120k * 10 = 1.35M, with 10% buffer = 1.485M
        let result = calculate_disperse_gas_limit(10);
        assert_eq!(result, 1_485_000);
    }

    #[test]
    fn test_calculate_disperse_gas_limit_fifty_recipients() {
        // Base: 150k + 120k * 50 = 6.15M, with 10% buffer = 6.765M
        let result = calculate_disperse_gas_limit(50);
        assert_eq!(result, 6_765_000);
    }

    #[test]
    fn test_calculate_disperse_gas_limit_cap_at_max() {
        // 200 recipients would be: 150k + 120k * 200 = 24.15M, with buffer = 26.565M
        // Should be capped at 15M
        let result = calculate_disperse_gas_limit(200);
        assert_eq!(result, 15_000_000);
    }

    #[test]
    fn test_calculate_disperse_gas_limit_zero_recipients() {
        // Edge case: 0 recipients
        // Base: 150k + 0 = 150k, with 10% buffer = 165k
        let result = calculate_disperse_gas_limit(0);
        assert_eq!(result, 165_000);
    }
}
