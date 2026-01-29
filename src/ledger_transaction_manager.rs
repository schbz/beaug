//! Ledger transaction manager for reliable hardware wallet operations.
//! Provides nonce management, retry logic, and transaction confirmation tracking.

use ethers::prelude::*;
use ethers::providers::{Http, Provider};
use anyhow::{anyhow, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::{error, info, warn};

/// Configuration for transaction manager behavior
#[derive(Clone, Debug)]
pub struct TransactionManagerConfig {
    pub inter_transaction_delay_ms: u64,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub wait_for_confirmation: bool,
    pub confirmation_timeout_secs: u64,
    pub derivation_mode: crate::config::DerivationMode,
    pub custom_account: u32,
    pub custom_address_index: u32,
    pub coin_type: u32,
    pub use_native_ledger: bool,
}

impl Default for TransactionManagerConfig {
    fn default() -> Self {
        Self {
            inter_transaction_delay_ms: 3000, // 3 seconds between transactions
            max_retries: 2,
            retry_delay_ms: 2000,
            wait_for_confirmation: true,
            confirmation_timeout_secs: 90,
            derivation_mode: crate::config::DerivationMode::default(),
            custom_account: 0,
            custom_address_index: 0,
            coin_type: crate::config::DEFAULT_COIN_TYPE,
            use_native_ledger: false,
        }
    }
}

/// Result of a transaction attempt
#[derive(Debug, Clone)]
pub enum TransactionResult {
    Success {
        tx_hash: TxHash,
        block_number: Option<u64>,
        gas_used: u64,
    },
    Failed {
        error: String,
        retryable: bool,
    },
}

/// Transaction to be executed
#[derive(Debug, Clone)]
pub struct PendingTransaction {
    pub to: Address,
    pub value: U256,
    pub gas_limit: u64,
    pub gas_price: U256,
    pub operation_name: String,
}

/// Professional transaction manager for Ledger operations using cast
pub struct LedgerTransactionManager {
    provider: Arc<Provider<Http>>,
    config: TransactionManagerConfig,
    chain_id: u64,
    source_address: Address,
    source_index: u32,
    rpc_url: String,
    current_nonce: Arc<Mutex<Option<u64>>>,
}

impl LedgerTransactionManager {
    /// Create a new transaction manager
    pub async fn new(
        provider: Arc<Provider<Http>>,
        config: TransactionManagerConfig,
        chain_id: u64,
        source_address: Address,
        source_index: u32,
        rpc_url: String,
    ) -> Result<Self> {
        // Verify cast is available only when using cast backend
        if !config.use_native_ledger {
            crate::ethers_ledger_signer::check_cast_available()?;
        }
        
        Ok(Self {
            provider,
            config,
            chain_id,
            source_address,
            source_index,
            rpc_url,
            current_nonce: Arc::new(Mutex::new(None)),
        })
    }

    /// Initialize the manager by fetching the current nonce
    pub async fn initialize(&self) -> Result<()> {
        let nonce = self.fetch_current_nonce().await?;
        *self.current_nonce.lock().await = Some(nonce);
        info!(
            "Transaction manager initialized with nonce {} for {:?}",
            nonce, self.source_address
        );
        Ok(())
    }

    /// Fetch the current nonce from the blockchain
    async fn fetch_current_nonce(&self) -> Result<u64> {
        let nonce = self.provider.get_transaction_count(self.source_address, None).await?;
        Ok(nonce.as_u64())
    }

    /// Get the next nonce, refreshing from blockchain if needed
    async fn get_next_nonce(&self) -> Result<u64> {
        let mut nonce_guard = self.current_nonce.lock().await;
        
        match *nonce_guard {
            Some(nonce) => {
                let refreshed_nonce = self.fetch_current_nonce().await?;
                let next_nonce = std::cmp::max(nonce, refreshed_nonce);
                *nonce_guard = Some(next_nonce + 1);
                Ok(next_nonce)
            }
            None => {
                let nonce = self.fetch_current_nonce().await?;
                *nonce_guard = Some(nonce + 1);
                Ok(nonce)
            }
        }
    }

    /// Execute a single transaction with retry logic
    pub async fn execute_transaction(
        &self,
        tx: &PendingTransaction,
    ) -> Result<TransactionResult> {
        let mut attempt = 0;
        let mut delay = self.config.retry_delay_ms;

        loop {
            attempt += 1;
            let nonce = self.get_next_nonce().await?;
            
            info!(
                "Executing transaction {} (attempt {}/{}) to {:?} with nonce {}",
                tx.operation_name, attempt, self.config.max_retries + 1, tx.to, nonce
            );

            match self.send_transaction_internal(tx, nonce).await {
                Ok(tx_hash) => {
                    info!("Transaction sent successfully: {:?}", tx_hash);

                    if self.config.wait_for_confirmation {
                        match self.wait_for_confirmation(tx_hash).await {
                            Ok((block_number, gas_used)) => {
                                return Ok(TransactionResult::Success {
                                    tx_hash,
                                    block_number,
                                    gas_used,
                                });
                            }
                            Err(e) => {
                                warn!("Transaction sent but confirmation failed: {}", e);
                                return Ok(TransactionResult::Success {
                                    tx_hash,
                                    block_number: None,
                                    gas_used: 0,
                                });
                            }
                        }
                    } else {
                        return Ok(TransactionResult::Success {
                            tx_hash,
                            block_number: None,
                            gas_used: 0,
                        });
                    }
                }
                Err(e) => {
                    let error_str = e.to_string();
                    let retryable = self.is_retryable_error(&error_str);

                    if attempt > self.config.max_retries || !retryable {
                        error!(
                            "Transaction failed after {} attempts: {}",
                            attempt, error_str
                        );
                        return Ok(TransactionResult::Failed {
                            error: error_str.clone(),
                            retryable,
                        });
                    }

                    warn!(
                        "Transaction attempt {} failed (retryable: {}): {}",
                        attempt, retryable, error_str
                    );

                    // Always wait before retrying to give Ledger/HID time to settle.
                    warn!("Retrying after {}ms delay...", delay);
                    sleep(Duration::from_millis(delay)).await;
                    delay *= 2;
                }
            }
        }
    }

    /// Internal method to send a transaction using the configured Ledger backend
    async fn send_transaction_internal(
        &self,
        tx: &PendingTransaction,
        nonce: u64,
    ) -> Result<TxHash> {
        use crate::ledger_dispatch::sign_and_send_transaction;

        // Use the dispatch layer to route to the appropriate backend
        sign_and_send_transaction(
            self.config.use_native_ledger,
            self.provider.clone(),
            &self.rpc_url,
            self.source_index,
            tx.to,
            tx.value,
            tx.gas_limit,
            tx.gas_price,
            nonce,
            self.chain_id,
            self.config.derivation_mode,
            self.config.custom_account,
            self.config.custom_address_index,
            self.config.coin_type,
        )
        .await
    }

    /// Wait for transaction confirmation
    async fn wait_for_confirmation(&self, tx_hash: TxHash) -> Result<(Option<u64>, u64)> {
        let mut attempts = 0;
        let max_attempts = (self.config.confirmation_timeout_secs * 2) as usize;
        
        loop {
            if let Ok(Some(receipt)) = self.provider.get_transaction_receipt(tx_hash).await {
                return Ok((
                    receipt.block_number.map(|n| n.as_u64()),
                    receipt.gas_used.map(|g| g.as_u64()).unwrap_or(0),
                ));
            }
            
            attempts += 1;
            if attempts >= max_attempts {
                return Err(anyhow!("Confirmation timeout after {} seconds", self.config.confirmation_timeout_secs));
            }
            
            sleep(Duration::from_millis(500)).await;
        }
    }

    /// Determine if an error is retryable
    fn is_retryable_error(&self, error: &str) -> bool {
        if error.contains("rejected") || error.contains("denied") || error.contains("Denied") {
            return false;
        }

        error.contains("timeout")
            || error.contains("network")
            || error.contains("connection")
            || error.contains("temporarily")
            || error.contains("rate limit")
            || error.contains("hidapi")
            || error.contains("Overlapped I/O operation is in progress")
            || error.contains("bad response")
    }

    /// Execute multiple transactions in sequence
    pub async fn execute_transaction_batch(
        &self,
        transactions: Vec<PendingTransaction>,
        progress_callback: Option<Box<dyn Fn(usize, usize, &TransactionResult) + Send + Sync>>,
    ) -> Vec<TransactionResult> {
        let total = transactions.len();
        let mut results = Vec::new();
        let mut success_count = 0;
        let mut failure_count = 0;

        info!("Starting batch execution of {} transactions", total);

        for (index, tx) in transactions.into_iter().enumerate() {
            if index > 0 {
                info!(
                    "Waiting {}ms before next transaction...",
                    self.config.inter_transaction_delay_ms
                );
                sleep(Duration::from_millis(self.config.inter_transaction_delay_ms)).await;
                sleep(Duration::from_millis(500)).await; // Extra delay for Ledger
            }

            match self.execute_transaction(&tx).await {
                Ok(result) => {
                    match &result {
                        TransactionResult::Success { .. } => {
                            success_count += 1;
                            info!("Transaction {}/{} succeeded", index + 1, total);
                        }
                        TransactionResult::Failed { .. } => {
                            failure_count += 1;
                            warn!("Transaction {}/{} failed", index + 1, total);
                        }
                    }

                    if let Some(ref callback) = progress_callback {
                        callback(index + 1, total, &result);
                    }

                    results.push(result);
                }
                Err(e) => {
                    error!("Transaction {}/{} error: {}", index + 1, total, e);
                    failure_count += 1;
                    results.push(TransactionResult::Failed {
                        error: e.to_string(),
                        retryable: false,
                    });
                }
            }
        }

        info!(
            "Batch execution complete: {} succeeded, {} failed out of {} total",
            success_count, failure_count, total
        );

        results
    }

    /// Refresh nonce from blockchain
    pub async fn refresh_nonce(&self) -> Result<u64> {
        let nonce = self.fetch_current_nonce().await?;
        *self.current_nonce.lock().await = Some(nonce);
        info!("Nonce refreshed to {}", nonce);
        Ok(nonce)
    }
}
