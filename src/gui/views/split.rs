//! Split operation views implementation
//!
//! This module provides the split operation panel rendering including:
//! - Source address selection
//! - Split parameters configuration (random or equal distribution)
//! - Transaction preview and execution
//! - Progress tracking and completion logging
//!
//! ## Implementation Notes
//!
//! The split view is one of the most complex views in the application, with
//! tight coupling to `GuiApp` internals including:
//! - `split_state()` - Access to split operation state
//! - `spawn_job()` - Background job management
//! - `TransactionView` widget integration
//! - Multiple async operations and polling
//!
//! Due to this complexity, the main implementations remain in `app.rs`:
//! - `GuiApp::view_split()` - Main split view
//! - `GuiApp::view_split_transactions()` - Transaction queue view
//! - `GuiApp::render_split_address_selection()` - Address selection UI
//! - `GuiApp::start_split_job()` - Job initialization
//! - `GuiApp::check_and_log_split_completion()` - Completion logging
//! - `GuiApp::rerandomize_transaction_amounts()` - Re-randomization

use eframe::egui;

use super::super::app::{GuiApp, SplitModeDescriptor, SplitSelector};

/// Renders the Split Random view
///
/// This is a convenience wrapper that delegates to `GuiApp::view_split()`.
pub fn view_split_random(app: &mut GuiApp, ui: &mut egui::Ui) {
    app.view_split(ui, SplitSelector::Random, SplitModeDescriptor::random());
}

/// Renders the Split Equal view
///
/// This is a convenience wrapper that delegates to `GuiApp::view_split()`.
pub fn view_split_equal(app: &mut GuiApp, ui: &mut egui::Ui) {
    app.view_split(ui, SplitSelector::Equal, SplitModeDescriptor::equal());
}
