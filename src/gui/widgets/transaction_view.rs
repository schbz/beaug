//! Transaction view component for the GUI
//! Displays a list of transactions with individual status and control buttons

use crate::transaction_queue::{QueuedTransaction, TransactionQueue, TransactionStatus};
use eframe::egui::{self, Color32};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, TryRecvError};

/// State for the transaction view widget
pub struct TransactionView {
    /// The transaction queue
    queue: TransactionQueue,
    /// Active execution job for a single transaction
    active_job: Option<TransactionJob>,
    /// Last error message
    last_error: Option<String>,
    /// Flag indicating if re-randomize was requested
    rerandomize_requested: bool,
    /// Whether to show the re-randomize button (only for random splits)
    show_rerandomize: bool,
    /// Cached ledger ready status for use in nested render calls
    last_ledger_ready: bool,
    /// Cached native token symbol for use in nested render calls
    last_native_token: String,
    /// Chain ID for block explorer links
    chain_id: u64,
    /// Pending notifications to be collected by the GUI
    pending_notifications: VecDeque<String>,
    /// Track which transaction IDs we've already notified about to avoid duplicates
    notified_tx_ids: std::collections::HashSet<usize>,
}

struct TransactionJob {
    receiver: Receiver<Result<(), String>>,
}

impl TransactionView {
    /// Create a new transaction view (no re-randomize button)
    pub fn new(queue: TransactionQueue, chain_id: u64) -> Self {
        Self {
            queue,
            active_job: None,
            last_error: None,
            rerandomize_requested: false,
            show_rerandomize: false,
            last_ledger_ready: false,
            last_native_token: "ETH".to_string(),
            chain_id,
            pending_notifications: VecDeque::new(),
            notified_tx_ids: std::collections::HashSet::new(),
        }
    }

    /// Create a new transaction view that allows re-randomization (shows button)
    pub fn with_rerandomize(queue: TransactionQueue, chain_id: u64) -> Self {
        Self {
            queue,
            active_job: None,
            last_error: None,
            rerandomize_requested: false,
            show_rerandomize: true,
            last_ledger_ready: false,
            last_native_token: "ETH".to_string(),
            chain_id,
            pending_notifications: VecDeque::new(),
            notified_tx_ids: std::collections::HashSet::new(),
        }
    }

    /// Take all pending notifications (moves them out, leaving the queue empty)
    pub fn take_notifications(&mut self) -> Vec<String> {
        self.pending_notifications.drain(..).collect()
    }

    /// Update the chain ID (for when network changes)
    pub fn set_chain_id(&mut self, chain_id: u64) {
        self.chain_id = chain_id;
    }

    /// Render the transaction view
    /// 
    /// # Arguments
    /// * `ui` - The egui UI context
    /// * `ledger_ready` - Whether the ledger is connected and ready for signing
    /// * `ledger_warning` - Optional warning message to display when ledger is not ready
    /// * `native_token` - The native token symbol for the current network (e.g., "ETH", "PLS")
    pub fn show(&mut self, ui: &mut egui::Ui, ledger_ready: bool, ledger_warning: Option<&str>, native_token: &str) {
        // Cache values for nested render calls
        self.last_native_token = native_token.to_string();
        
        // Get current transactions
        let transactions = Self::block_on_async(self.queue.get_transactions());
        let stats = Self::block_on_async(self.queue.get_statistics());

        // Check for newly completed transactions and generate notifications
        for tx in &transactions {
            if self.notified_tx_ids.contains(&tx.id) {
                continue;
            }
            match &tx.status {
                TransactionStatus::Success { tx_hash, .. } => {
                    self.notified_tx_ids.insert(tx.id);
                    let short_hash = format!("{:?}", tx_hash);
                    let short_hash = if short_hash.len() > 18 {
                        format!("{}...{}", &short_hash[..10], &short_hash[short_hash.len()-6..])
                    } else {
                        short_hash
                    };
                    self.pending_notifications.push_back(format!(
                        "[OK] Tx #{} sent {} {} ({})",
                        tx.id + 1,
                        ethers::utils::format_ether(tx.transaction.value),
                        native_token,
                        short_hash
                    ));
                }
                TransactionStatus::Failed { error, .. } => {
                    self.notified_tx_ids.insert(tx.id);
                    let short_error = if error.len() > 50 {
                        format!("{}...", &error[..50])
                    } else {
                        error.clone()
                    };
                    self.pending_notifications.push_back(format!(
                        "[!!] Tx #{} failed: {}",
                        tx.id + 1,
                        short_error
                    ));
                }
                TransactionStatus::Skipped => {
                    self.notified_tx_ids.insert(tx.id);
                    self.pending_notifications.push_back(format!(
                        "[--] Tx #{} skipped",
                        tx.id + 1
                    ));
                }
                _ => {}
            }
        }

        // Header with statistics
        ui.group(|ui| {
            ui.horizontal(|ui| {
                
                // Compact statistics
                if stats.pending > 0 {
                    ui.colored_label(Color32::GRAY, format!("⏸ {} pending", stats.pending));
                }
                if stats.in_progress > 0 {
                    ui.colored_label(Color32::YELLOW, format!("⏳ {} in progress", stats.in_progress));
                }
                if stats.success > 0 {
                    ui.colored_label(Color32::GREEN, format!("✅ {} complete", stats.success));
                }
                if stats.failed > 0 {
                    ui.colored_label(Color32::RED, format!("❌ {} failed", stats.failed));
                }
                if stats.skipped > 0 {
                    ui.colored_label(Color32::DARK_GRAY, format!("⏭ {} skipped", stats.skipped));
                }
            });
            
            // Progress bar
            if stats.total > 0 {
                let progress = (stats.success + stats.failed + stats.skipped) as f32 / stats.total as f32;
                ui.add(egui::ProgressBar::new(progress).show_percentage());
            }
        });

        // Show ledger warning if not ready
        if !ledger_ready {
            if let Some(warning) = ledger_warning {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(warning).color(Color32::from_rgb(255, 170, 0)).size(12.0));
                });
            }
        }

        // Control buttons
        let sign_button_hover = if ledger_ready {
            "Sign all pending transactions"
        } else {
            "Connect and unlock your Ledger to sign transactions"
        };
        
        ui.horizontal(|ui| {
            let sign_button = egui::Button::new("Sign All Pending")
                .fill(egui::Color32::from_rgb(15, 15, 15))
                .stroke(egui::Stroke::new(3.0, egui::Color32::from_rgb(0, 221, 119)));
            
            if ui.add_enabled(ledger_ready && self.active_job.is_none(), sign_button)
                .on_hover_text(sign_button_hover)
                .clicked() {
                self.start_execute_all();
            }

            // Re-randomize button (only for random splits) - doesn't need ledger
            if self.show_rerandomize {
                if ui.add(egui::Button::new("[~] Re-randomize")
                    .fill(egui::Color32::from_rgb(15, 15, 15))
                    .stroke(egui::Stroke::new(3.0, egui::Color32::from_rgb(0, 221, 119))))
                    .clicked() {
                    self.rerandomize_requested = true;
                }
            }
        });
        
        // Store ledger_ready for use in render_transaction
        self.last_ledger_ready = ledger_ready;

        // Error display
        if let Some(error) = &self.last_error {
            ui.colored_label(Color32::RED, format!("⚠ {}", error));
        }

        ui.separator();

        // Check active job
        if let Some(job) = &mut self.active_job {
            match job.receiver.try_recv() {
                Ok(Ok(())) => {
                    self.active_job = None;
                    self.last_error = None;
                }
                Ok(Err(e)) => {
                    self.active_job = None;
                    self.last_error = Some(e);
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.active_job = None;
                    self.last_error = Some("Job disconnected".to_string());
                }
            }
        }

        // Transaction list
        egui::ScrollArea::vertical()
            .max_height(400.0)
            .show(ui, |ui| {
                for tx in transactions {
                    self.render_transaction(ui, tx);
                    ui.add_space(4.0); // Small spacing between transactions
                }
            });
    }

    /// Render a single transaction
    fn render_transaction(&mut self, ui: &mut egui::Ui, tx: QueuedTransaction) {
        ui.group(|ui| {
            // Header row with status and basic info
            ui.horizontal(|ui| {
                // Status indicator
                let (status_icon, status_color) = match &tx.status {
                    TransactionStatus::Pending => ("⏸", Color32::GRAY),
                    TransactionStatus::InProgress => ("⏳", Color32::YELLOW),
                    TransactionStatus::Success { .. } => ("✅", Color32::GREEN),
                    TransactionStatus::Failed { .. } => ("❌", Color32::RED),
                    TransactionStatus::Skipped => ("⏭", Color32::DARK_GRAY),
                };
                
                ui.colored_label(status_color, status_icon);
                ui.label(format!("Transaction #{}", tx.id + 1));
                ui.separator();
                
                // Show amount prominently
                ui.label(format!("{} {}", ethers::utils::format_ether(tx.transaction.value), self.last_native_token));
                
                    // Action buttons on the right
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let sign_hover = if self.last_ledger_ready {
                            "Sign this transaction"
                        } else {
                            "Connect and unlock your Ledger to sign"
                        };
                        
                        match &tx.status {
                            TransactionStatus::Pending => {
                                if self.active_job.is_none() {
                                    // Skip button doesn't need ledger
                                    if ui.add(egui::Button::new("Skip")
                                        .fill(egui::Color32::from_rgb(15, 15, 15))
                                        .stroke(egui::Stroke::new(2.0, egui::Color32::from_rgb(0, 221, 119))))
                                        .clicked() {
                                        // Handle skip inline since Result<()> doesn't implement Default
                                        let queue = self.queue.clone();
                                        let tx_id = tx.id;
                                        if let Ok(handle) = tokio::runtime::Handle::try_current() {
                                            tokio::task::block_in_place(|| {
                                                let _ = handle.block_on(queue.skip_transaction(tx_id));
                                            });
                                        } else if let Ok(rt) = tokio::runtime::Runtime::new() {
                                            let _ = rt.block_on(queue.skip_transaction(tx_id));
                                        }
                                    }
                                    // Sign button needs ledger
                                    let sign_button = egui::Button::new("Sign")
                                        .fill(egui::Color32::from_rgb(15, 15, 15))
                                        .stroke(egui::Stroke::new(3.0, egui::Color32::from_rgb(0, 221, 119)));
                                    if ui.add_enabled(self.last_ledger_ready, sign_button)
                                        .on_hover_text(sign_hover)
                                        .clicked() {
                                        self.start_transaction(tx.id);
                                    }
                                }
                            }
                            TransactionStatus::Failed { retryable: true, .. } => {
                                if self.active_job.is_none() {
                                    let retry_button = egui::Button::new("Retry")
                                        .fill(egui::Color32::from_rgb(15, 15, 15))
                                        .stroke(egui::Stroke::new(3.0, egui::Color32::from_rgb(0, 221, 119)));
                                    if ui.add_enabled(self.last_ledger_ready, retry_button)
                                        .on_hover_text(sign_hover)
                                        .clicked() {
                                        self.start_transaction(tx.id);
                                    }
                                }
                            }
                            _ => {}
                        }
                    });
            });
            
            // Details section
            ui.indent("tx_details", |ui| {
                // Destination on its own line
                ui.horizontal_wrapped(|ui| {
                    ui.label("To:");
                    
                    // Parse destination label in format "derivation_path → 0xAddress"
                    // or "external:0xAddress → 0xAddress" or legacy "Index X (0xAddress)"
                    let dest_label = &tx.destination_label;
                    
                    // Extract derivation path (before the arrow)
                    if let Some(arrow_pos) = dest_label.find(" → ") {
                        let path_part = &dest_label[..arrow_pos];
                        // Show a shortened version of the derivation path
                        let short_path = if path_part.starts_with("m/44'") {
                            // Extract the last segment (address index)
                            if let Some(last_slash) = path_part.rfind('/') {
                                format!("idx {}", &path_part[last_slash+1..])
                            } else {
                                path_part.to_string()
                            }
                        } else if path_part.starts_with("external:") {
                            "external".to_string()
                        } else {
                            path_part.to_string()
                        };
                        ui.label(&short_path);
                    } else if let Some(idx_start) = dest_label.find("Index ") {
                        // Legacy format: "Index X (0xAddress)"
                        if let Some(idx_end) = dest_label[idx_start..].find(" (") {
                            let index_part = &dest_label[idx_start..idx_start + idx_end];
                            ui.label(index_part);
                        }
                    }
                    
                    // Show shortened address
                    if let Some(addr_start) = dest_label.find("0x") {
                        // Find end of address (42 characters for full address)
                        let addr_end = (addr_start + 42).min(dest_label.len());
                        let full_addr = &dest_label[addr_start..addr_end];
                        if full_addr.len() >= 10 {
                            let short_addr = format!("{}...{}", &full_addr[..6], &full_addr[full_addr.len().saturating_sub(4)..]);
                            ui.label(short_addr);
                            
                            // Copy button for full address
                            if ui.add(egui::Button::new("📋").small())
                                .on_hover_text("Copy full address")
                                .clicked() {
                                ui.output_mut(|o| o.copied_text = full_addr.to_string());
                            }
                        } else {
                            ui.label(full_addr);
                        }
                    } else if !dest_label.is_empty() {
                        // Fallback: just show the label as-is
                        ui.label(dest_label);
                    }
                });
                
                // Status-specific details
                match &tx.status {
                    TransactionStatus::InProgress => {
                        ui.colored_label(Color32::YELLOW, "⏳ Waiting for Ledger confirmation...");
                    }
                    TransactionStatus::Success { tx_hash, block_number, gas_used } => {
                        // Transaction hash with copy and explorer buttons
                        ui.horizontal_wrapped(|ui| {
                            ui.label("Hash:");
                            let hash_str = format!("{:?}", tx_hash);
                            let short_hash = format!("{}...{}", &hash_str[..8], &hash_str[hash_str.len()-6..]);
                            ui.label(&short_hash);
                            if ui.add(egui::Button::new("📋").small())
                                .on_hover_text("Copy transaction hash")
                                .clicked() {
                                ui.output_mut(|o| o.copied_text = hash_str.clone());
                            }
                            // Block explorer link
                            if let Some(explorer_url) = crate::config::get_tx_explorer_url(self.chain_id, &hash_str) {
                                if ui.add(egui::Button::new("🔗").small())
                                    .on_hover_text("View on block explorer")
                                    .clicked() {
                                    if let Err(e) = open::that(&explorer_url) {
                                        tracing::warn!("Failed to open explorer URL: {}", e);
                                    }
                                }
                            }
                        });
                        
                        // Block and gas info
                        ui.horizontal(|ui| {
                            if let Some(block) = block_number {
                                ui.label(format!("Block: #{}", block));
                                ui.separator();
                            }
                            ui.label(format!("Gas used: {}", gas_used));
                        });
                    }
                    TransactionStatus::Failed { error, retryable } => {
                        // Error message (truncated if too long)
                        let error_display = if error.len() > 80 {
                            format!("{}...", &error[..80])
                        } else {
                            error.clone()
                        };
                        ui.colored_label(Color32::RED, format!("Error: {}", error_display));
                        
                        if !retryable {
                            ui.colored_label(Color32::DARK_RED, "❌ Not retryable");
                        }
                    }
                    TransactionStatus::Skipped => {
                        ui.colored_label(Color32::DARK_GRAY, "Transaction was skipped");
                    }
                    _ => {}
                }
            });
        });
    }

    /// Start executing a single transaction
    fn start_transaction(&mut self, id: usize) {
        let queue = self.queue.clone();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(async {
                    queue.execute_transaction(id).await.map_err(|e| e.to_string())
                }),
                Err(e) => Err(format!("Failed to create async runtime: {}", e)),
            };
            tx.send(result).ok();
        });

        self.active_job = Some(TransactionJob {
            receiver: rx,
        });
    }

    /// Start executing all transactions
    fn start_execute_all(&mut self) {
        let queue = self.queue.clone();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let result = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(async {
                    queue.execute_all().await.map(|_| ()).map_err(|e| e.to_string())
                }),
                Err(e) => Err(format!("Failed to create async runtime: {}", e)),
            };
            tx.send(result).ok();
        });

        self.active_job = Some(TransactionJob {
            receiver: rx,
        });
    }

    /// Get the transaction queue
    pub fn queue(&self) -> &TransactionQueue {
        &self.queue
    }

    /// Set a new transaction queue
    pub fn set_queue(&mut self, queue: TransactionQueue) {
        self.queue = queue;
        self.active_job = None;
        self.last_error = None;
        self.rerandomize_requested = false;
    }

    /// Check if re-randomize was requested and reset the flag
    pub fn take_rerandomize_request(&mut self) -> bool {
        let requested = self.rerandomize_requested;
        self.rerandomize_requested = false;
        requested
    }

    /// Helper to block on async operations
    /// Returns default value if runtime creation fails (logs error)
    fn block_on_async<T: Default>(fut: impl std::future::Future<Output = T>) -> T {
        // Try to use existing runtime handle, or create a new one
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            // We're in an async context, use block_in_place
            tokio::task::block_in_place(move || handle.block_on(fut))
        } else {
            // Create a new runtime
            match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(fut),
                Err(e) => {
                    tracing::error!("Failed to create Tokio runtime for UI operation: {}", e);
                    T::default()
                }
            }
        }
    }
}
