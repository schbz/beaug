//! Widget components for the GUI
//!
//! This module contains reusable UI widgets that can be embedded in views.
//!
//! ## Available Widgets
//!
//! - `TransactionView` - Displays a list of transactions with status and control buttons

mod transaction_view;

pub use transaction_view::TransactionView;
