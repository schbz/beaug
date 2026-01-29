#![windows_subsystem = "windows"]

use anyhow::Result;
use beaug::{config::Config, gui, operation_log};
use tracing_subscriber;

fn main() -> Result<()> {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt::init();

    // Migrate operation log from old location (current dir) to new location (app data dir) if needed
    operation_log::migrate_log_if_needed();

    // Create default config - GUI will load user settings and update accordingly
    let config = Config::default();
    gui::launch(config)?;

    Ok(())
}
