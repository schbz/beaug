//! Notification system for the GUI
//!
//! Handles notification entries and operation state polling.

use super::async_job::AsyncJob;
use std::collections::VecDeque;

/// A notification entry with message and timestamp
#[derive(Clone)]
pub struct NotificationEntry {
    pub message: String,
    pub timestamp: chrono::DateTime<chrono::Local>,
}

impl NotificationEntry {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            timestamp: chrono::Local::now(),
        }
    }

    pub fn time_ago(&self) -> String {
        let now = chrono::Local::now();
        let duration = now.signed_duration_since(self.timestamp);
        if duration.num_seconds() < 60 {
            "just now".to_string()
        } else if duration.num_minutes() < 60 {
            format!("{}m ago", duration.num_minutes())
        } else if duration.num_hours() < 24 {
            format!("{}h ago", duration.num_hours())
        } else {
            self.timestamp.format("%m/%d %H:%M").to_string()
        }
    }
}

/// Trait for operation states that can be polled
pub trait OperationState {
    fn job_mut(&mut self) -> &mut Option<AsyncJob<()>>;
    fn status_mut(&mut self) -> &mut Option<String>;
}

/// Poll an operation state and update notifications on completion
pub fn poll_operation_state<T: OperationState>(
    state: &mut T,
    notifications: &mut VecDeque<NotificationEntry>,
) {
    if let Some(job) = state.job_mut() {
        if let Some(res) = job.poll() {
            match res {
                Ok(_) => {
                    *state.status_mut() = Some("[OK] Completed".into());
                    notifications.push_back(NotificationEntry::new(
                        "Operation completed successfully.",
                    ));
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    // Check if this is a Ledger-related error and add helpful message
                    let status_msg = if error_msg.contains("APDU")
                        || error_msg.contains("6a80")
                        || error_msg.contains("INVALID_DATA")
                    {
                        format!(
                            "[!!] Failed: {}\n\nMake sure \"Blind Signing\" is enabled in your Ledger Ethereum app settings.",
                            error_msg
                        )
                    } else if error_msg.contains("Ledger")
                        && (error_msg.contains("denied") || error_msg.contains("rejected"))
                    {
                        format!(
                            "[!!] Failed: {}\n\nTransaction was rejected on the Ledger device.",
                            error_msg
                        )
                    } else {
                        format!("[!!] Failed: {}", error_msg)
                    };
                    *state.status_mut() = Some(status_msg);
                    notifications.push_back(NotificationEntry::new(format!(
                        "Operation failed: {}",
                        error_msg
                    )));
                }
            }
            *state.job_mut() = None;
        }
    }
}
