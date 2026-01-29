use anyhow::Result;
use chrono::Utc;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Log file name
const OPERATION_LOG_FILE: &str = "operation_log.txt";

/// Old log file location (in current/working directory) for migration
const OLD_LOG_FILE: &str = "operation_log.txt";

/// Get the directory where app data is stored (same as settings)
fn app_data_dir() -> PathBuf {
    if let Some(config_dir) = dirs::config_dir() {
        let app_dir = config_dir.join("beaug");
        if !app_dir.exists() {
            let _ = fs::create_dir_all(&app_dir);
        }
        app_dir
    } else {
        // Fall back to current directory
        PathBuf::from(".")
    }
}

/// Get the full path to the operation log file
fn log_path() -> PathBuf {
    app_data_dir().join(OPERATION_LOG_FILE)
}

/// Get the full path to the operation log file as a string for display
pub fn log_file_path() -> String {
    log_path().display().to_string()
}

/// Migrate old log file from current directory to the new app data location
/// This is called once at startup to move any existing log to the new location
pub fn migrate_log_if_needed() {
    let old_path = PathBuf::from(OLD_LOG_FILE);
    let new_path = log_path();
    
    // Only migrate if:
    // 1. Old file exists in current directory
    // 2. New location is different from old location
    // 3. New file doesn't already exist (or we'll append to it)
    if old_path.exists() && old_path.canonicalize().ok() != new_path.canonicalize().ok() {
        // Read old content
        if let Ok(old_content) = fs::read_to_string(&old_path) {
            // Append old content to new file (in case new file has some entries)
            if let Ok(mut file) = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&new_path)
            {
                // Add migration marker
                let _ = writeln!(file, "\n--- Migrated from {} ---\n", old_path.display());
                let _ = write!(file, "{}", old_content);
                
                // Remove old file after successful migration
                let _ = fs::remove_file(&old_path);
                
                tracing::info!(
                    "Migrated operation log from {:?} to {:?}",
                    old_path,
                    new_path
                );
            }
        }
    }
}

/// Append a structured log entry describing a user-requested operation.
pub fn append_log(operation: &str, chain_id: u64, details: impl AsRef<str>) -> Result<()> {
    let path = log_path();
    
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            fs::create_dir_all(parent)?;
        }
    }
    
    let timestamp = Utc::now().to_rfc3339();
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;

    writeln!(
        file,
        "[{}] chain_id={} operation={}",
        timestamp, chain_id, operation
    )?;

    let body = details.as_ref();
    if body.trim().is_empty() {
        writeln!(file, "  (no additional details)")?;
    } else {
        for line in body.lines() {
            if line.trim().is_empty() {
                writeln!(file)?;
            } else {
                writeln!(file, "  {}", line)?;
            }
        }
    }

    writeln!(file)?;
    Ok(())
}

/// Read the entire log file content
pub fn read_log() -> Result<String> {
    let path = log_path();
    if path.exists() {
        Ok(fs::read_to_string(&path)?)
    } else {
        Ok(String::new())
    }
}
