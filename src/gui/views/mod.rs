//! View modules for the GUI
//!
//! This module organizes the different view implementations of the application.
//! Each submodule contains the rendering logic for a specific view/screen.
//!
//! ## Module Structure
//!
//! - `dashboard` - Main dashboard with network status, logs, and about section
//! - `settings` - Application configuration and network settings
//! - `split` - Split operation (random and equal distribution)
//! - `disperse` - Bulk disperse operation
//! - `balances` - Balance scanning and viewing
//!
//! ## Implementation Notes
//!
//! Each view module exports a main view function that takes `&mut GuiApp` and `&mut egui::Ui`.
//! These functions are called from the main `App::update` method in `app.rs`.

pub mod balances;
pub mod dashboard;
pub mod disperse;
pub mod settings;
pub mod split;

// Re-export main view functions for convenient access
pub use balances::view_check_balances;
pub use split::{view_split_equal, view_split_random};
