//! Ledger hardware wallet operations via Foundry's cast CLI.
//! Provides status checking, address derivation, and device connection management.

use ethers::prelude::*;
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::time::sleep;
use tracing::{error, info, warn};

#[cfg(windows)]
use std::os::windows::process::CommandExt;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;

const CAST_TIMEOUT_SECS: u64 = 30;

/// Cached path to the cast executable
static CAST_PATH: OnceLock<String> = OnceLock::new();

/// Find the cast executable, checking common installation paths if not in PATH.
/// The result is cached for subsequent calls.
pub fn get_cast_path() -> &'static str {
    CAST_PATH.get_or_init(|| {
        // First, try if "cast" is available in PATH
        if is_cast_available("cast") {
            info!("Found cast in PATH");
            return "cast".to_string();
        }

        // Check common Foundry installation paths
        let candidate_paths = get_foundry_candidate_paths();
        
        for path in candidate_paths {
            let cast_path = path.join(if cfg!(windows) { "cast.exe" } else { "cast" });
            if cast_path.exists() {
                let path_str = cast_path.to_string_lossy().to_string();
                if is_cast_available(&path_str) {
                    info!("Found cast at: {}", path_str);
                    return path_str;
                }
            }
        }

        // Fall back to "cast" and let the error propagate later
        warn!("cast not found in PATH or common locations, falling back to 'cast'");
        "cast".to_string()
    })
}

/// Get common Foundry installation paths to check
fn get_foundry_candidate_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // Check user home directory locations
    if let Some(home) = dirs::home_dir() {
        // Standard Foundry installation: ~/.foundry/bin
        paths.push(home.join(".foundry").join("bin"));
        
        // Alternative: ~/foundry/bin
        paths.push(home.join("foundry").join("bin"));
    }

    // On Windows, also check common program locations
    #[cfg(windows)]
    {
        if let Some(local_app_data) = dirs::data_local_dir() {
            paths.push(local_app_data.join("foundry").join("bin"));
            paths.push(local_app_data.join(".foundry").join("bin"));
        }
        if let Some(program_files) = std::env::var_os("ProgramFiles") {
            paths.push(PathBuf::from(program_files).join("foundry").join("bin"));
        }
    }

    // Check directory where the current executable is located
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            paths.push(exe_dir.to_path_buf());
        }
    }

    paths
}

/// Check if cast is available at the given path
fn is_cast_available(cast_path: &str) -> bool {
    let mut cmd = std::process::Command::new(cast_path);
    cmd.arg("--version");
    
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    
    cmd.output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Status of the Ledger device connection
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerStatus {
    Connected { address: Address },
    Locked,
    Disconnected,
    Checking,
    Unknown(String),
}

impl LedgerStatus {
    /// Returns true only if definitely connected
    pub fn is_ready(&self) -> bool {
        matches!(self, LedgerStatus::Connected { .. })
    }
    
    /// Returns true if connected OR currently checking (allows operations to proceed during status checks)
    pub fn is_usable(&self) -> bool {
        matches!(self, LedgerStatus::Connected { .. } | LedgerStatus::Checking)
    }
    
    /// Returns true if there's a known connection problem (not checking, not connected)
    pub fn has_problem(&self) -> bool {
        matches!(self, LedgerStatus::Locked | LedgerStatus::Disconnected | LedgerStatus::Unknown(_))
    }

    pub fn display_text(&self) -> String {
        match self {
            LedgerStatus::Connected { address } => {
                let addr_str = format!("{:?}", address);
                format!(
                    "🟢 Connected: {}...{}",
                    &addr_str[..8],
                    &addr_str[38..42]
                )
            }
            LedgerStatus::Locked => "🟡 Locked / App Closed".to_string(),
            LedgerStatus::Disconnected => "🔴 Not Connected".to_string(),
            LedgerStatus::Checking => "⏳ Checking...".to_string(),
            LedgerStatus::Unknown(msg) => format!("⚪ {}", msg),
        }
    }

    pub fn color(&self) -> (u8, u8, u8) {
        match self {
            LedgerStatus::Connected { .. } => (50, 205, 50),
            LedgerStatus::Locked => (255, 193, 7),
            LedgerStatus::Disconnected => (220, 53, 69),
            LedgerStatus::Checking => (100, 149, 237),
            LedgerStatus::Unknown(_) => (150, 150, 150),
        }
    }
}

/// Get an address from the Ledger at a specific derivation index using cast
async fn get_address_via_cast(index: u32, config: Option<&crate::config::Config>) -> Result<Address, String> {
    let hd_path = if let Some(cfg) = config {
        cfg.get_derivation_path(index)
    } else {
        // Default to account-based for backward compatibility
        format!("m/44'/60'/{}'/0/0", index)
    };

    // Serialize Ledger/HID access across the entire process.
    let _lock = crate::ledger_lock::ledger_lock().lock().await;

    let mut command = Command::new(get_cast_path());
    command
        .arg("wallet")
        .arg("address")
        .arg("--ledger")
        .arg("--hd-path")
        .arg(&hd_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Hide console window on Windows
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);

    let mut child = command
        .spawn()
        .map_err(|e| format!("Failed to run cast: {}", e))?;

    let mut child_stdout = child
        .stdout
        .take()
        .ok_or_else(|| "Failed to capture cast stdout".to_string())?;
    let mut child_stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Failed to capture cast stderr".to_string())?;

    let stdout_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        child_stdout
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("Failed reading cast stdout: {}", e))?;
        Ok::<Vec<u8>, String>(buf)
    });

    let stderr_task = tokio::spawn(async move {
        let mut buf = Vec::new();
        child_stderr
            .read_to_end(&mut buf)
            .await
            .map_err(|e| format!("Failed reading cast stderr: {}", e))?;
        Ok::<Vec<u8>, String>(buf)
    });

    let status = match tokio::time::timeout(Duration::from_secs(CAST_TIMEOUT_SECS), child.wait()).await {
        Ok(res) => res.map_err(|e| format!("Failed to run cast: {}", e))?,
        Err(_) => {
            let _ = child.kill().await;
            return Err(format!(
                "Timed out waiting for Ledger response (cast wallet address, {}s).",
                CAST_TIMEOUT_SECS
            ));
        }
    };

    let stdout = stdout_task
        .await
        .map_err(|e| format!("stdout task join error: {}", e))??;
    let stderr = stderr_task
        .await
        .map_err(|e| format!("stderr task join error: {}", e))??;

    if status.success() {
        let stdout = String::from_utf8_lossy(&stdout);
        let addr_str = stdout.trim();
        addr_str
            .parse::<Address>()
            .map_err(|e| format!("Failed to parse address: {}", e))
    } else {
        let stderr = String::from_utf8_lossy(&stderr);
        Err(stderr.to_string())
    }
}

/// Check the Ledger connection status
pub async fn check_ledger_status(_chain_id: u64) -> LedgerStatus {
    // First check if cast is available using our path finder
    let cast_path = get_cast_path();
    let mut cmd = std::process::Command::new(cast_path);
    cmd.arg("--version");
    
    // Hide console window on Windows
    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);
    
    let cast_available = match cmd.output() {
        Ok(output) => output.status.success(),
        Err(_) => false,
    };

    if !cast_available {
        return LedgerStatus::Unknown("cast not found - install Foundry (https://getfoundry.sh)".to_string());
    }
    
    match get_address_via_cast(0, None).await {
        Ok(address) => LedgerStatus::Connected { address },
        Err(err_str) => {
            if err_str.contains("6983")
                || err_str.contains("6985")
                || err_str.contains("locked")
                || err_str.contains("Denied")
                || err_str.contains("not open")
            {
                LedgerStatus::Locked
            } else if err_str.contains("No device found")
                || err_str.contains("hidapi")
                || err_str.contains("DeviceNotFound")
            {
                LedgerStatus::Disconnected
            } else {
                LedgerStatus::Unknown(err_str.chars().take(40).collect())
            }
        }
    }
}

/// Get a Ledger address at the given derivation index
pub async fn get_ledger_address(chain_id: u64, index: u32) -> Result<Address> {
    get_ledger_address_with_config(chain_id, index, None).await
}

pub async fn get_ledger_address_with_config(chain_id: u64, index: u32, config: Option<&crate::config::Config>) -> Result<Address> {
    get_ledger_address_with_retry_config(chain_id, index, config).await
}

pub async fn get_ledger_address_with_retry(_chain_id: u64, index: u32) -> Result<Address> {
    get_ledger_address_with_retry_config(_chain_id, index, None).await
}

pub async fn get_ledger_address_with_retry_config(_chain_id: u64, index: u32, config: Option<&crate::config::Config>) -> Result<Address> {
    // IMPORTANT: Do not block on stdin here (this code runs in both CLI and GUI).
    // Instead, do a small bounded retry for transient HID errors, then return a
    // clear error for the caller/UI to handle.
    const MAX_ATTEMPTS: usize = 5;

    for attempt in 1..=MAX_ATTEMPTS {
        info!("Getting address from Ledger at index {} (attempt {}/{})...", index, attempt, MAX_ATTEMPTS);

        match get_address_via_cast(index, config).await {
            Ok(address) => return Ok(address),
            Err(err_str) => {
                let locked = err_str.contains("locked")
                    || err_str.contains("6983")
                    || err_str.contains("6985")
                    || err_str.contains("Denied")
                    || err_str.contains("not open");

                let disconnected = err_str.contains("No device found")
                    || err_str.contains("hidapi")
                    || err_str.contains("DeviceNotFound");

                // Common on Windows if another Ledger call is still unwinding.
                let transient_hid_busy = err_str.contains("Overlapped I/O operation is in progress");

                if locked {
                    return Err(anyhow!("Ledger is locked or Ethereum app is not open."));
                }

                if disconnected {
                    return Err(anyhow!(
                        "Ledger device not found. Please ensure it's connected and unlocked."
                    ));
                }

                if transient_hid_busy && attempt < MAX_ATTEMPTS {
                    sleep(Duration::from_millis(300 * attempt as u64)).await;
                    continue;
                }

                error!("Ledger error: {}", err_str);
                return Err(anyhow!("Ledger error: {}", err_str));
            }
        }
    }

    Err(anyhow!("Failed to read Ledger address after {} attempts.", MAX_ATTEMPTS))
}
