//! Generic async job handling for GUI operations
//!
//! This module provides a simple way to poll background tasks from the GUI thread.

use anyhow::{anyhow, Result};
use std::sync::mpsc::{Receiver, TryRecvError};

/// Helper struct for async jobs - polls a background task
pub struct AsyncJob<T> {
    receiver: Option<Receiver<Result<T>>>,
}

impl<T> AsyncJob<T> {
    /// Create a new async job with the given receiver
    pub fn new(receiver: Receiver<Result<T>>) -> Self {
        Self {
            receiver: Some(receiver),
        }
    }

    /// Poll the job for completion
    /// Returns Some(result) if the job has completed, None if still running
    pub fn poll(&mut self) -> Option<Result<T>> {
        if let Some(rx) = &self.receiver {
            match rx.try_recv() {
                Ok(res) => {
                    self.receiver = None;
                    return Some(res);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.receiver = None;
                    return Some(Err(anyhow!("Worker task disconnected")));
                }
            }
        }
        None
    }

    /// Check if the job is still running
    pub fn is_running(&self) -> bool {
        self.receiver.is_some()
    }
}
