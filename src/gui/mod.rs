//! GUI module for the Beaug application
//!
//! This module provides the graphical user interface built with egui/eframe.
//!
//! ## Module Structure
//!
//! - `app` - Main GuiApp struct, state types, and core application logic
//! - `async_job` - Generic async job polling for background tasks
//! - `theme` - Centralized theme and styling system (AppTheme)
//! - `helpers` - Utility functions for gas calculations, formatting, asset loading
//! - `notifications` - Notification system and operation state polling
//! - `views` - View rendering functions (dashboard, settings, balances, disperse, split)
//! - `widgets` - Reusable UI widgets (TransactionView)
//!
//! ## Usage
//!
//! ```no_run
//! use beaug::config::Config;
//! use beaug::gui;
//!
//! let config = Config::default();
//! gui::launch(config).expect("Failed to launch GUI");
//! ```
//!
//! ## Version
//!
//! The application version is sourced from `Cargo.toml` via `env!("CARGO_PKG_VERSION")`.
//! Both the title bar and About section display this version automatically.

mod app;
pub mod async_job;
pub mod helpers;
pub mod notifications;
pub mod theme;
pub mod views;
pub mod widgets;

// Re-export main public API
pub use app::{launch, GuiApp, GuiSection};

// Re-export commonly used types from submodules for convenience
pub use async_job::AsyncJob;
pub use helpers::{
    calculate_disperse_gas_limit, format_gwei, gas_speed_emoji, gas_speed_label, gas_speed_warning,
    load_icon, BEAUG_ICON_PNG, BEAUG_LOGO_WEBP,
};
pub use notifications::{NotificationEntry, OperationState};
pub use theme::{configure_style, AppTheme};
pub use widgets::TransactionView;