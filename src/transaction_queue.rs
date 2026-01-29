//! Transaction queue management for GUI-controlled execution
//! This module provides a queue of transactions that can be executed one by one with GUI control.

use crate::ledger_transaction_manager::{LedgerTransactionManager, PendingTransaction, TransactionResult};
use ethers::prelude::*;
use std::sync::Arc;
use tokio::sync::{Mutex, oneshot};
use anyhow::Result;

/// Status of a queued transaction
#[derive(Debug, Clone)]
pub enum TransactionStatus {
    /// Transaction is waiting to be executed
    Pending,
    /// Transaction is currently being executed (waiting for Ledger confirmation)
    InProgress,
    /// Transaction was executed successfully
    Success {
        tx_hash: TxHash,
        block_number: Option<u64>,
        gas_used: u64,
    },
    /// Transaction execution failed
    Failed {
        error: String,
        retryable: bool,
    },
    /// Transaction was skipped by user
    Skipped,
}

/// A transaction in the queue with its status and metadata
#[derive(Debug, Clone)]
pub struct QueuedTransaction {
    /// Unique ID for this transaction
    pub id: usize,
    /// The transaction details
    pub transaction: PendingTransaction,
    /// Current status
    pub status: TransactionStatus,
    /// Human-readable description
    pub description: String,
    /// Destination address label (e.g., "Index 5")
    pub destination_label: String,
}

/// Transaction queue that can be controlled from the GUI
#[derive(Clone)]
pub struct TransactionQueue {
    /// All queued transactions
    transactions: Arc<Mutex<Vec<QueuedTransaction>>>,
    /// The transaction manager for execution
    manager: Option<Arc<LedgerTransactionManager>>,
    _cancel_sender: Arc<Mutex<Option<oneshot::Sender<()>>>>,
    /// Custom delay between transactions (milliseconds)
    transaction_delay_ms: Arc<Mutex<u64>>,
}

impl TransactionQueue {
    /// Create a new empty transaction queue
    pub fn new() -> Self {
        Self {
            transactions: Arc::new(Mutex::new(Vec::new())),
            manager: None,
            _cancel_sender: Arc::new(Mutex::new(None)),
            transaction_delay_ms: Arc::new(Mutex::new(3000)), // Default 3 seconds
        }
    }
    
    /// Create a new transaction queue with custom delay
    pub fn with_delay(delay_ms: u64) -> Self {
        Self {
            transactions: Arc::new(Mutex::new(Vec::new())),
            manager: None,
            _cancel_sender: Arc::new(Mutex::new(None)),
            transaction_delay_ms: Arc::new(Mutex::new(delay_ms)),
        }
    }
    
    /// Set the transaction delay
    pub async fn set_delay(&self, delay_ms: u64) {
        let mut delay = self.transaction_delay_ms.lock().await;
        *delay = delay_ms;
    }
    
    /// Get the transaction delay
    pub async fn get_delay(&self) -> u64 {
        let delay = self.transaction_delay_ms.lock().await;
        *delay
    }

    /// Set the transaction manager for this queue
    pub fn set_manager(&mut self, manager: Arc<LedgerTransactionManager>) {
        self.manager = Some(manager);
    }

    /// Add transactions to the queue
    pub async fn add_transactions(&self, transactions: Vec<(PendingTransaction, String, String)>) {
        let mut queue = self.transactions.lock().await;
        let start_id = queue.len();
        
        for (idx, (tx, desc, dest_label)) in transactions.into_iter().enumerate() {
            queue.push(QueuedTransaction {
                id: start_id + idx,
                transaction: tx,
                status: TransactionStatus::Pending,
                description: desc,
                destination_label: dest_label,
            });
        }
    }

    /// Clear all transactions from the queue
    pub async fn clear(&self) {
        let mut queue = self.transactions.lock().await;
        queue.clear();
    }

    /// Get a copy of all transactions in the queue
    pub async fn get_transactions(&self) -> Vec<QueuedTransaction> {
        let queue = self.transactions.lock().await;
        queue.clone()
    }

    /// Get the status of a specific transaction
    pub async fn get_transaction_status(&self, id: usize) -> Option<TransactionStatus> {
        let queue = self.transactions.lock().await;
        queue.iter()
            .find(|tx| tx.id == id)
            .map(|tx| tx.status.clone())
    }

    /// Update the status of a transaction
    pub async fn update_status(&self, id: usize, status: TransactionStatus) {
        let mut queue = self.transactions.lock().await;
        if let Some(tx) = queue.iter_mut().find(|tx| tx.id == id) {
            tx.status = status;
        }
    }

    /// Update the value of a pending transaction
    pub async fn update_pending_transaction_value(&self, id: usize, new_value: U256) -> Result<()> {
        let mut queue = self.transactions.lock().await;
        if let Some(tx) = queue.iter_mut().find(|tx| tx.id == id) {
            match &tx.status {
                TransactionStatus::Pending => {
                    tx.transaction.value = new_value;
                    Ok(())
                }
                _ => Err(anyhow::anyhow!("Can only update values for pending transactions")),
            }
        } else {
            Err(anyhow::anyhow!("Transaction not found"))
        }
    }

    /// Execute a specific transaction by ID
    pub async fn execute_transaction(&self, id: usize) -> Result<()> {
        // Get the transaction
        let tx_to_execute = {
            let mut queue = self.transactions.lock().await;
            let tx = queue.iter_mut().find(|tx| tx.id == id)
                .ok_or_else(|| anyhow::anyhow!("Transaction not found"))?;
            
            // Check if it's in a valid state to execute
            match &tx.status {
                TransactionStatus::Pending | TransactionStatus::Failed { retryable: true, .. } => {
                    tx.status = TransactionStatus::InProgress;
                    tx.transaction.clone()
                }
                _ => return Err(anyhow::anyhow!("Transaction cannot be executed in current state")),
            }
        };

        // Execute the transaction
        let manager = self.manager.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Transaction manager not set"))?;

        match manager.execute_transaction(&tx_to_execute).await? {
            TransactionResult::Success { tx_hash, block_number, gas_used } => {
                self.update_status(id, TransactionStatus::Success {
                    tx_hash,
                    block_number,
                    gas_used,
                }).await;
                Ok(())
            }
            TransactionResult::Failed { error, retryable } => {
                self.update_status(id, TransactionStatus::Failed {
                    error: error.clone(),
                    retryable,
                }).await;
                Err(anyhow::anyhow!("Transaction failed: {}", error))
            }
        }
    }

    /// Execute all pending transactions in sequence
    pub async fn execute_all(&self) -> Result<Vec<(usize, TransactionStatus)>> {
        let _manager = self.manager.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Transaction manager not set"))?;

        let mut results = Vec::new();
        
        // Get all pending transactions
        let pending_ids: Vec<usize> = {
            let queue = self.transactions.lock().await;
            queue.iter()
                .filter(|tx| matches!(tx.status, TransactionStatus::Pending))
                .map(|tx| tx.id)
                .collect()
        };

        // Execute each one
        let last_id = pending_ids.last().copied();
        for id in pending_ids {
            match self.execute_transaction(id).await {
                Ok(_) => {
                    // Get status - if somehow missing, treat as unknown success
                    let status = self.get_transaction_status(id).await.unwrap_or_else(|| {
                        TransactionStatus::Success {
                            tx_hash: TxHash::zero(),
                            block_number: None,
                            gas_used: 0,
                        }
                    });
                    results.push((id, status));
                }
                Err(e) => {
                    let status = TransactionStatus::Failed {
                        error: e.to_string(),
                        retryable: false,
                    };
                    self.update_status(id, status.clone()).await;
                    results.push((id, status));
                }
            }
            
            // Add delay between transactions (except after the last one)
            if Some(id) != last_id {
                let delay_ms = self.get_delay().await;
                tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            }
        }

        Ok(results)
    }

    /// Skip a transaction
    pub async fn skip_transaction(&self, id: usize) -> Result<()> {
        let mut queue = self.transactions.lock().await;
        let tx = queue.iter_mut().find(|tx| tx.id == id)
            .ok_or_else(|| anyhow::anyhow!("Transaction not found"))?;
        
        match &tx.status {
            TransactionStatus::Pending => {
                tx.status = TransactionStatus::Skipped;
                Ok(())
            }
            _ => Err(anyhow::anyhow!("Can only skip pending transactions")),
        }
    }

    /// Get queue statistics
    pub async fn get_statistics(&self) -> QueueStatistics {
        let queue = self.transactions.lock().await;
        let mut stats = QueueStatistics::default();
        
        for tx in queue.iter() {
            stats.total += 1;
            match &tx.status {
                TransactionStatus::Pending => stats.pending += 1,
                TransactionStatus::InProgress => stats.in_progress += 1,
                TransactionStatus::Success { .. } => stats.success += 1,
                TransactionStatus::Failed { .. } => stats.failed += 1,
                TransactionStatus::Skipped => stats.skipped += 1,
            }
        }
        
        stats
    }
}

/// Statistics about the queue
#[derive(Debug, Clone, Default)]
pub struct QueueStatistics {
    pub total: usize,
    pub pending: usize,
    pub in_progress: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl QueueStatistics {
    /// Check if all transactions are complete (no pending or in-progress)
    pub fn is_complete(&self) -> bool {
        self.pending == 0 && self.in_progress == 0
    }

    /// Get a summary string
    pub fn summary(&self) -> String {
        format!(
            "Total: {} | Pending: {} | Success: {} | Failed: {} | Skipped: {}",
            self.total, self.pending, self.success, self.failed, self.skipped
        )
    }
}
