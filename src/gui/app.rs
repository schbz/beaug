//! Main GUI application module
//!
//! Contains the GuiApp struct and all its implementations.

use crate::{
    balance::{self, BalanceScanRecord, FundedAddressScan},
    bulk_disperse,
    config::{Config, NetworkCategory, NETWORKS},
    gui::widgets::TransactionView,
    ledger_dispatch,
    ledger_ops::{self, LedgerStatus},
    split_operations,
    transaction_queue::{TransactionQueue, TransactionStatus},
    user_settings::CustomNetwork,
    utils,
};
use anyhow::{anyhow, Result};
use eframe::{egui, egui::RichText, App, Frame, NativeOptions};
use egui_extras;
use ethers::prelude::Middleware;
use std::collections::VecDeque;
use std::mem;
use std::sync::mpsc;
use std::thread;
use tokio::runtime::Builder;

// Import types from submodules
// Note: Some types are still defined locally in this file for tight coupling with methods.
// Future refactoring can move more types to submodules.
use super::async_job::AsyncJob;
use super::helpers::{
    calculate_disperse_gas_limit, gas_speed_emoji, gas_speed_label, gas_speed_warning,
    load_icon, BEAUG_LOGO_WEBP,
};
use super::notifications::{poll_operation_state, NotificationEntry, OperationState};
use super::theme::{configure_style, AppTheme};

/// GUI section enum for navigation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiSection {
    Dashboard,
    CheckBalances,
    SplitRandom,
    SplitEqual,
    BulkDisperse,
    Settings,
}

pub(crate) struct CheckBalancesState {
    pub(crate) start_index: u32,
    pub(crate) empty_target: u32,
    pub(crate) result: Option<balance::BalanceScanResult>,
    pub(crate) streaming_records: Vec<balance::BalanceScanRecord>,
    pub(crate) error: Option<String>,
    pub(crate) job: Option<AsyncJob<()>>,
    pub(crate) cancel_sender: Option<tokio::sync::oneshot::Sender<()>>,
    pub(crate) progress_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<balance::ScanProgress>>,
    pub(crate) show_parameters: bool, // Whether to show the parameters panel vs results
}

impl Default for CheckBalancesState {
    fn default() -> Self {
        Self {
            start_index: 0,
            empty_target: 5,
            result: None,
            streaming_records: Vec::new(),
            error: None,
            job: None,
            cancel_sender: None,
            progress_receiver: None,
            show_parameters: true,
        }
    }
}

impl CheckBalancesState {
    /// Create a new CheckBalancesState with values from user settings
    fn with_settings(settings: &crate::user_settings::UserSettings) -> Self {
        Self {
            start_index: settings.default_scan_start_index,
            empty_target: settings.default_scan_empty_streak,
            result: None,
            streaming_records: Vec::new(),
            error: None,
            job: None,
            cancel_sender: None,
            progress_receiver: None,
            show_parameters: true,
        }
    }

    /// Update scan parameters from user settings
    pub(crate) fn update_from_settings(&mut self, settings: &crate::user_settings::UserSettings) {
        self.start_index = settings.default_scan_start_index;
        self.empty_target = settings.default_scan_empty_streak;
    }
}

/// State for selecting a source address before running an operation
#[derive(Default)]
pub(crate) struct SourceSelectionState {
    /// Job scanning for funded addresses
    pub(crate) scan_job: Option<AsyncJob<FundedAddressScan>>,
    /// Streaming scan job with progress updates
    pub(crate) streaming_scan_job: Option<AsyncJob<()>>,
    /// Results of the scan - addresses with balances
    pub(crate) funded_addresses: Option<Vec<BalanceScanRecord>>,
    /// Empty addresses found during scanning (can be reused for split operations)
    pub(crate) empty_addresses: Option<Vec<BalanceScanRecord>>,
    /// Streaming results as they come in
    pub(crate) streaming_funded_addresses: Vec<BalanceScanRecord>,
    /// Streaming empty addresses found during scanning
    pub(crate) streaming_empty_addresses: Vec<BalanceScanRecord>,
    /// Error from scanning
    pub(crate) scan_error: Option<String>,
    /// The selected source index (set when user picks an address)
    pub(crate) selected_index: Option<usize>,
    /// Cancel sender for streaming scans
    pub(crate) cancel_sender: Option<tokio::sync::oneshot::Sender<()>>,
    /// Progress receiver for streaming scans
    pub(crate) progress_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<balance::FundedScanProgress>>,
}


impl SourceSelectionState {
    fn reset(&mut self) {
        self.scan_job = None;
        self.streaming_scan_job = None;
        self.funded_addresses = None;
        self.empty_addresses = None;
        self.streaming_funded_addresses.clear();
        self.streaming_empty_addresses.clear();
        self.scan_error = None;
        self.selected_index = None;
        self.cancel_sender = None;
        self.progress_receiver = None;
    }

    fn is_scanning(&self) -> bool {
        self.scan_job.as_ref().map(|j| j.is_running()).unwrap_or(false)
            || self.streaming_scan_job.as_ref().map(|j| j.is_running()).unwrap_or(false)
    }

    fn has_results(&self) -> bool {
        self.funded_addresses.is_some() || !self.streaming_funded_addresses.is_empty()
    }

    fn get_all_funded_addresses(&self) -> Vec<&BalanceScanRecord> {
        if let Some(ref addresses) = self.funded_addresses {
            addresses.iter().collect()
        } else {
            self.streaming_funded_addresses.iter().collect()
        }
    }

    fn get_all_empty_addresses(&self) -> Vec<&BalanceScanRecord> {
        if let Some(ref addresses) = self.empty_addresses {
            addresses.iter().collect()
        } else {
            self.streaming_empty_addresses.iter().collect()
        }
    }
}

pub(crate) struct SplitState {
    pub(crate) output_count: u32,
    pub(crate) recipient_addresses: String,
    pub(crate) gas_speed: Option<f32>,  // None = use global default, Some(value) = override
    pub(crate) source_index: String,
    pub(crate) remaining_balance: String,
    pub(crate) scan_start_index: u32,
    pub(crate) scan_empty_streak: u32,
    pub(crate) transaction_delay_ms: u64,
    pub(crate) transaction_view: Option<TransactionView>,
    pub(crate) status: Option<String>,
    pub(crate) job: Option<AsyncJob<()>>,
    pub(crate) prep_job: Option<AsyncJob<(TransactionQueue, usize)>>,
    pub(crate) prep_progress_receiver: Option<tokio::sync::mpsc::UnboundedReceiver<split_operations::PrepareProgress>>,
    pub(crate) source_selection: SourceSelectionState,
    // Parameters for re-randomization
    pub(crate) last_prep_params: Option<(SplitMode, u32, Option<f32>, Option<usize>, Option<Vec<String>>)>,
    pub(crate) rerandomize_job: Option<AsyncJob<()>>,
    // Track if operation was logged (to avoid duplicate logs)
    pub(crate) operation_logged: bool,
}

impl SplitState {
    /// Create a new SplitState with values from user settings
    fn with_settings(settings: &crate::user_settings::UserSettings) -> Self {
        // Convert default remaining balance from wei to ETH string
        let remaining_balance_str = if settings.default_remaining_balance > 0 {
            let remaining_eth = settings.default_remaining_balance as f64 / 1_000_000_000_000_000_000.0;
            format!("{}", remaining_eth)
        } else {
            String::new()
        };
        
        Self {
            output_count: settings.default_split_outputs,
            recipient_addresses: String::new(),
            gas_speed: None,  // Use global default from settings
            source_index: String::new(),
            remaining_balance: remaining_balance_str,
            scan_start_index: settings.default_scan_start_index,
            scan_empty_streak: settings.default_scan_empty_streak,
            transaction_delay_ms: 400,
            transaction_view: None,
            status: None,
            job: None,
            prep_job: None,
            prep_progress_receiver: None,
            source_selection: SourceSelectionState::default(),
            last_prep_params: None,
            rerandomize_job: None,
            operation_logged: false,
        }
    }

    /// Update scan parameters from user settings
    pub(crate) fn update_from_settings(&mut self, settings: &crate::user_settings::UserSettings) {
        self.output_count = settings.default_split_outputs;
        self.scan_start_index = settings.default_scan_start_index;
        self.scan_empty_streak = settings.default_scan_empty_streak;
        
        // Update remaining balance from settings (only if user hasn't customized it)
        if self.remaining_balance.trim().is_empty() && settings.default_remaining_balance > 0 {
            let remaining_eth = settings.default_remaining_balance as f64 / 1_000_000_000_000_000_000.0;
            self.remaining_balance = format!("{}", remaining_eth);
        }
    }
}

pub struct BulkDisperseState {
    pub recipients_input: String,
    pub disperse_contract_address: String,
    pub amount_input: String, // Amount of ETH to send
    pub remaining_balance: String, // Amount of ETH to keep on source address
    pub tip_amount: String, // Tip amount in ETH (added as regular recipient)
    pub source_index: u32,
    pub status: Option<String>,
    pub job: Option<AsyncJob<()>>,
    pub source_selection: SourceSelectionState,
    // Gas speed for transaction priority
    pub gas_speed: f32, // 0.8=Slow, 1.0=Standard, 1.5=Fast, 2.0+=Aggressive
    pub current_gas_price: Option<ethers::types::U256>,
    pub gas_price_job: Option<AsyncJob<ethers::types::U256>>,
    // Source balance tracking
    pub source_address: Option<String>,
    pub source_balance: Option<ethers::types::U256>,
    pub source_balance_job: Option<AsyncJob<(String, ethers::types::U256)>>,
    pub last_fetched_source_index: Option<u32>,
    // Contract validation
    pub contract_validation: Option<crate::disperse::ContractValidationStatus>,
    pub contract_validation_job: Option<AsyncJob<crate::disperse::ContractValidationStatus>>,
    pub last_validated_address: Option<String>,
}

impl Default for BulkDisperseState {
    fn default() -> Self {
        Self {
            recipients_input: String::new(),
            disperse_contract_address: "0xe7deB73d0661aA3732c971Ab3d583CFCa786e0d7".to_string(), // Beaug CREATE2 address
            amount_input: String::new(), // Will be auto-calculated
            remaining_balance: "0".to_string(), // Default: keep nothing on source
            tip_amount: String::new(), // No tip by default
            source_index: 0,
            status: None,
            job: None,
            source_selection: SourceSelectionState::default(),
            gas_speed: 1.0, // Normal speed by default
            current_gas_price: None,
            gas_price_job: None,
            // Source balance tracking
            source_address: None,
            source_balance: None,
            source_balance_job: None,
            last_fetched_source_index: None,
            // Contract validation
            contract_validation: None,
            contract_validation_job: None,
            last_validated_address: None,
        }
    }
}


pub(crate) struct BalanceViewState {
    pub(crate) index: u32,
    pub(crate) address: Option<String>,
    pub(crate) balance: Option<String>,
    pub(crate) job: Option<AsyncJob<(String, String)>>,
    pub(crate) error: Option<String>,
}

impl Default for BalanceViewState {
    fn default() -> Self {
        Self {
            index: 0,
            address: None,
            balance: None,
            job: None,
            error: None,
        }
    }
}

pub(crate) struct LogViewState {
    pub(crate) content: String,
    pub(crate) job: Option<AsyncJob<String>>,
    pub(crate) error: Option<String>,
    /// Flag to scroll to bottom on next render
    pub(crate) scroll_to_bottom: bool,
}

impl Default for LogViewState {
    fn default() -> Self {
        Self {
            content: "No logs yet. Run an operation to generate entries.".to_string(),
            job: None,
            error: None,
            scroll_to_bottom: true, // Start scrolled to bottom
        }
    }
}

impl OperationState for SplitState {
    fn job_mut(&mut self) -> &mut Option<AsyncJob<()>> {
        &mut self.job
    }

    fn status_mut(&mut self) -> &mut Option<String> {
        &mut self.status
    }
}

impl OperationState for BulkDisperseState {
    fn job_mut(&mut self) -> &mut Option<AsyncJob<()>> {
        &mut self.job
    }

    fn status_mut(&mut self) -> &mut Option<String> {
        &mut self.status
    }
}

/// Represents either a built-in network or a custom network selection
#[derive(Clone, Debug, PartialEq)]
pub enum NetworkSelection {
    /// Index into the static NETWORKS array
    Builtin(usize),
    /// Chain ID of a custom network
    Custom(u64),
}

impl NetworkSelection {
    /// Find the appropriate network selection for a chain ID
    fn from_chain_id(chain_id: u64, custom_networks: &[CustomNetwork]) -> Self {
        if let Some(idx) = crate::config::find_network_index(chain_id) {
            NetworkSelection::Builtin(idx)
        } else if custom_networks.iter().any(|n| n.chain_id == chain_id) {
            NetworkSelection::Custom(chain_id)
        } else {
            // Default to first built-in network
            NetworkSelection::Builtin(0)
        }
    }
}

/// State for the custom network form in settings
#[derive(Default)]
pub struct CustomNetworkFormState {
    pub label: String,
    pub chain_id: String,
    pub native_token: String,
    pub rpc_url: String,
    pub error: Option<String>,
    pub editing_chain_id: Option<u64>, // Some(chain_id) when editing existing network
}

impl CustomNetworkFormState {
    pub(crate) fn clear(&mut self) {
        self.label.clear();
        self.chain_id.clear();
        self.native_token.clear();
        self.rpc_url.clear();
        self.error = None;
        self.editing_chain_id = None;
    }

    pub(crate) fn populate_from(&mut self, network: &CustomNetwork) {
        self.label = network.label.clone();
        self.chain_id = network.chain_id.to_string();
        self.native_token = network.native_token.clone();
        self.rpc_url = network.rpc_url.clone();
        self.error = None;
        self.editing_chain_id = Some(network.chain_id);
    }
}

pub struct GuiApp {
    pub(crate) config: Config,
    pub(crate) user_settings: crate::user_settings::UserSettings,
    pub(crate) theme: AppTheme,
    pub(crate) logo_texture: Option<egui::TextureHandle>,
    pub(crate) section: GuiSection,
    pub(crate) previous_section: GuiSection,
    pub(crate) notifications: VecDeque<NotificationEntry>,
    pub(crate) show_notifications_popup: bool,
    pub(crate) notification_toast_visible: bool,
    pub(crate) notification_toast_close_time: Option<std::time::Instant>,
    pub(crate) last_notification_count: usize,
    pub(crate) check_state: CheckBalancesState,
    pub(crate) split_random: SplitState,
    pub(crate) split_equal: SplitState,
    pub(crate) bulk_disperse_state: BulkDisperseState,
    pub(crate) balance_view: BalanceViewState,
    pub(crate) log_view: LogViewState,
    // Network selection
    pub(crate) network_selection: NetworkSelection,
    pub(crate) custom_rpc: String,
    pub(crate) use_custom_rpc: bool,
    // Ledger status
    pub(crate) ledger_status: LedgerStatus,
    pub(crate) last_stable_ledger_status: LedgerStatus, // Status before "Checking" - used for change detection
    pub(crate) ledger_status_job: Option<AsyncJob<LedgerStatus>>,
    pub(crate) last_status_check: std::time::Instant,
    // Derivation config (temporary values for editing)
    pub(crate) config_derivation_mode: crate::config::DerivationMode,
    pub(crate) config_custom_account: String,
    pub(crate) config_custom_address_index: String,
    pub(crate) config_coin_type: String,
    pub(crate) use_custom_coin_type: bool,
    // Settings page editing state
    pub(crate) settings_pending_chain_id: u64,
    pub(crate) settings_pending_gas_speed: f32,
    pub(crate) settings_pending_scan_start_index: u32,
    pub(crate) settings_pending_split_outputs: u32,
    pub(crate) settings_pending_scan_empty_streak: u32,
    pub(crate) settings_pending_remaining_balance: u64,
    // Custom network form state
    pub(crate) custom_network_form: CustomNetworkFormState,
    // Network status indicator
    pub(crate) rpc_latency_ms: Option<u64>,
    pub(crate) rpc_status_job: Option<AsyncJob<u64>>,
    pub(crate) last_rpc_check: std::time::Instant,
}

impl GuiApp {
    fn new(config: Config, ctx: &egui::Context) -> Self {
        let theme = AppTheme::default();
        configure_style(ctx, &theme);

        // Load user settings
        let user_settings = crate::user_settings::UserSettings::load();

        // Find the appropriate network selection, preferring user settings over config
        let network_selection = NetworkSelection::from_chain_id(
            user_settings.selected_chain_id,
            &user_settings.custom_networks,
        );

        // Build the correct config based on the selected network
        let mut config = match &network_selection {
            NetworkSelection::Builtin(idx) => {
                let net = &NETWORKS[*idx];
                Config::from_network(net)
            }
            NetworkSelection::Custom(chain_id) => {
                if let Some(net) = user_settings.get_custom_network(*chain_id) {
                    Config::from_custom_network(net)
                } else {
                    config // fallback to passed config
                }
            }
        };
        
        // Apply saved coin type override if set
        if let Some(coin_type) = user_settings.coin_type_override {
            config.coin_type = coin_type;
        }

        let config_derivation_mode = config.derivation_mode;
        let config_custom_account = config.custom_account.to_string();
        let config_custom_address_index = config.custom_address_index.to_string();
        let config_coin_type = user_settings.coin_type_override
            .unwrap_or(crate::config::DEFAULT_COIN_TYPE)
            .to_string();
        let use_custom_coin_type = user_settings.coin_type_override.is_some();
        let settings_pending_chain_id = user_settings.selected_chain_id;
        let settings_pending_gas_speed = user_settings.default_gas_speed;
        let settings_pending_scan_start_index = user_settings.default_scan_start_index;
        let settings_pending_split_outputs = user_settings.default_split_outputs;
        let settings_pending_scan_empty_streak = user_settings.default_scan_empty_streak;
        let settings_pending_remaining_balance = user_settings.default_remaining_balance;

        // Create state objects with settings before moving user_settings
        let check_state = CheckBalancesState::with_settings(&user_settings);
        let split_random = SplitState::with_settings(&user_settings);
        let split_equal = SplitState::with_settings(&user_settings);

        Self {
            config,
            user_settings,
            theme,
            logo_texture: None,
            section: GuiSection::Dashboard,
            previous_section: GuiSection::Dashboard,
            notifications: VecDeque::with_capacity(20),
            show_notifications_popup: false,
            notification_toast_visible: false,
            notification_toast_close_time: None,
            last_notification_count: 0,
            check_state,
            split_random,
            split_equal,
            bulk_disperse_state: BulkDisperseState::default(),
            balance_view: BalanceViewState::default(),
            log_view: LogViewState::default(),
            network_selection,
            custom_rpc: String::new(),
            use_custom_rpc: false,
            ledger_status: LedgerStatus::Unknown("Not checked yet".to_string()),
            last_stable_ledger_status: LedgerStatus::Unknown("Not checked yet".to_string()),
            ledger_status_job: None,
            last_status_check: std::time::Instant::now(),
            config_derivation_mode,
            config_custom_account,
            config_custom_address_index,
            config_coin_type,
            use_custom_coin_type,
            settings_pending_chain_id,
            settings_pending_gas_speed,
            settings_pending_scan_start_index,
            settings_pending_split_outputs,
            settings_pending_scan_empty_streak,
            settings_pending_remaining_balance,
            custom_network_form: CustomNetworkFormState::default(),
            rpc_latency_ms: None,
            rpc_status_job: None,
            last_rpc_check: std::time::Instant::now(),
        }
    }

    /// Get the display info for the currently selected network
    pub(crate) fn selected_network_info(&self) -> (String, String, u64, String) {
        // Returns (label, native_token, chain_id, default_rpc)
        match &self.network_selection {
            NetworkSelection::Builtin(idx) => {
                let net = &NETWORKS[*idx];
                (net.label.to_string(), net.native_token.to_string(), net.chain_id, net.default_rpc.to_string())
            }
            NetworkSelection::Custom(chain_id) => {
                if let Some(net) = self.user_settings.get_custom_network(*chain_id) {
                    (net.label.clone(), net.native_token.clone(), net.chain_id, net.rpc_url.clone())
                } else {
                    // Fallback if custom network not found
                    ("Unknown".to_string(), "ETH".to_string(), *chain_id, String::new())
                }
            }
        }
    }

    pub(crate) fn apply_network_selection(&mut self) {
        let (label, native_token, chain_id, default_rpc) = self.selected_network_info();
        let rpc_url = if self.use_custom_rpc && !self.custom_rpc.trim().is_empty() {
            self.custom_rpc.trim().to_string()
        } else {
            default_rpc
        };
        self.config = Config::new(rpc_url, chain_id);
        self.config.label_override = Some(label);
        self.config.native_token_override = Some(native_token);
        // Clear stale data
        self.balance_view = BalanceViewState::default();
        self.check_state.result = None;
        self.check_state.error = None;
    }

    pub(crate) fn spawn_job<T, FutBuilder, Fut>(&self, builder: FutBuilder) -> AsyncJob<T>
    where
        T: Send + 'static,
        FutBuilder: FnOnce() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<T>> + 'static,
    {
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = match Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime.block_on(builder()),
                Err(e) => Err(anyhow::anyhow!("Failed to create async runtime: {}", e)),
            };
            let _ = tx.send(result);
        });
        AsyncJob::new(rx)
    }

    fn poll_jobs(&mut self) {
        // Poll streaming progress for balance scan
        let mut scan_completed = false;
        if let Some(receiver) = &mut self.check_state.progress_receiver {
            // Process all available progress updates
            while let Ok(progress) = receiver.try_recv() {
                match progress {
                    balance::ScanProgress::AddressFound(record) => {
                        self.check_state.streaming_records.push(record);
                    }
                    balance::ScanProgress::Completed(result) => {
                        // Log the balance scan operation
                        self.log_balance_scan(&result);
                        self.check_state.result = Some(result);
                        self.check_state.error = None;
                        scan_completed = true;
                        break;
                    }
                }
            }
        }
        
        // Clean up if scan completed
        if scan_completed {
            self.check_state.progress_receiver = None;
            self.check_state.cancel_sender = None;
            self.check_state.job = None;
        }
        
        // Poll the job to see if it's complete or errored
        if let Some(job) = &mut self.check_state.job {
            if let Some(res) = job.poll() {
                if let Err(e) = res {
                    self.check_state.error = Some(e.to_string());
                    self.notifications
                        .push_back(NotificationEntry::new(format!("Balance scan failed: {}", e)));
                }
                // Clean up on completion or error
                self.check_state.job = None;
                self.check_state.progress_receiver = None;
                self.check_state.cancel_sender = None;
            }
        }

        let mut notifications = mem::take(&mut self.notifications);
        poll_operation_state(&mut self.split_random, &mut notifications);
        poll_operation_state(&mut self.split_equal, &mut notifications);
        
        // Poll bulk disperse with logging for failures
        let bulk_disperse_had_job = self.bulk_disperse_state.job.is_some();
        poll_operation_state(&mut self.bulk_disperse_state, &mut notifications);
        // Log bulk disperse failure if job just completed with error
        if bulk_disperse_had_job && self.bulk_disperse_state.job.is_none() {
            if let Some(status) = &self.bulk_disperse_state.status {
                if status.starts_with("[!!]") {
                    // Log the failure
                    let chain_id = self.config.chain_id;
                    let network_label = self.config.network_label().to_string();
                    let _ = crate::operation_log::append_log(
                        "Beaug Bulk Disperse",
                        chain_id,
                        format!(
                            "Bulk disperse FAILED on {} (Chain ID: {})\nError: {}",
                            network_label,
                            chain_id,
                            status.replace("[!!] Failed: ", "")
                        ),
                    );
                }
            }
        }
        
        self.notifications = notifications;

        // Poll source selection scan jobs for split states
        Self::poll_source_selection(&mut self.split_random.source_selection);
        Self::poll_source_selection(&mut self.split_equal.source_selection);
        Self::poll_source_selection(&mut self.bulk_disperse_state.source_selection);

        if let Some(job) = &mut self.balance_view.job {
            if let Some(res) = job.poll() {
                match res {
                    Ok((addr, bal)) => {
                        self.balance_view.address = Some(addr);
                        self.balance_view.balance = Some(bal);
                        self.balance_view.error = None;
                    }
                    Err(e) => {
                        self.balance_view.error = Some(e.to_string());
                    }
                }
                self.balance_view.job = None;
            }
        }

        if let Some(job) = &mut self.log_view.job {
            if let Some(res) = job.poll() {
                match res {
                    Ok(content) => {
                        self.log_view.content = content;
                        self.log_view.error = None;
                        // Scroll to bottom when new content is loaded
                        self.log_view.scroll_to_bottom = true;
                    }
                    Err(e) => {
                        self.log_view.error = Some(e.to_string());
                    }
                }
                self.log_view.job = None;
            }
        }

        // Poll Ledger status job
        if let Some(job) = &mut self.ledger_status_job {
            if let Some(res) = job.poll() {
                let new_status = match res {
                    Ok(status) => status,
                    Err(_) => LedgerStatus::Unknown("Check failed".to_string()),
                };
                
                // Generate notification if status has meaningfully changed
                if let Some(notification) = self.get_ledger_status_change_notification(&new_status) {
                    self.notifications.push_back(NotificationEntry::new(notification));
                }
                
                // Update both current and stable status
                self.last_stable_ledger_status = new_status.clone();
                self.ledger_status = new_status;
                self.ledger_status_job = None;
            }
        }

        // Poll gas price job for bulk disperse
        if let Some(job) = &mut self.bulk_disperse_state.gas_price_job {
            if let Some(res) = job.poll() {
                match res {
                    Ok(gas_price) => {
                        self.bulk_disperse_state.current_gas_price = Some(gas_price);
                    }
                    Err(_) => {
                        self.bulk_disperse_state.current_gas_price = None;
                    }
                }
                self.bulk_disperse_state.gas_price_job = None;
            }
        }
        
        // Poll source balance job for bulk disperse
        if let Some(job) = &mut self.bulk_disperse_state.source_balance_job {
            if let Some(res) = job.poll() {
                match res {
                    Ok((address, balance)) => {
                        self.bulk_disperse_state.source_address = Some(address);
                        self.bulk_disperse_state.source_balance = Some(balance);
                    }
                    Err(_) => {
                        self.bulk_disperse_state.source_address = None;
                        self.bulk_disperse_state.source_balance = None;
                    }
                }
                self.bulk_disperse_state.source_balance_job = None;
            }
        }

        // Auto-refresh Ledger status based on user-configured interval
        let refresh_interval = self.user_settings.ledger_refresh_interval_secs;
        if refresh_interval > 0 {
            let elapsed = self.last_status_check.elapsed();
            if self.ledger_status_job.is_none() && elapsed.as_secs() >= refresh_interval {
                self.start_ledger_status_check();
            }
        }

        while self.notifications.len() > 50 {
            self.notifications.pop_front();
        }
    }

    pub(crate) fn load_csv_file(&self, path: &std::path::Path) -> Result<String, Box<dyn std::error::Error>> {
        use std::fs::File;
        use std::io::Read;

        // Read the file
        let mut file = File::open(path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        // Parse CSV
        let mut result_lines = Vec::new();
        let mut invalid_count = 0;

        // Check if this is a balance scanner format by looking at headers
        let mut rdr = csv::Reader::from_reader(contents.as_bytes());
        let mut is_balance_scanner_format = false;

        if let Ok(headers) = rdr.headers() {
            if headers.len() >= 5 {
                let header_check = headers.iter().map(|s| s.to_lowercase()).collect::<Vec<_>>();
                if header_check.len() >= 5 &&
                   (header_check[0].contains("path") || header_check[0] == "path") &&
                   (header_check[1].contains("address") || header_check[1] == "address") &&
                   (header_check[4].contains("status") || header_check[4] == "status") {
                    is_balance_scanner_format = true;
                }
            }
        }

        for (line_num, result) in rdr.records().enumerate() {
            match result {
                Ok(record) => {
                    if is_balance_scanner_format && record.len() >= 5 {
                        // Balance scanner format: Path,Address,Balance,Token,Status
                        let address = record.get(1).unwrap_or("").trim_matches('"').trim();
                        let status = record.get(4).unwrap_or("").trim_matches('"').trim();

                        // Only include EMPTY addresses (not funded) with valid format
                        if status == "Empty" && !address.is_empty() {
                            // Basic validation: should start with 0x and be 42 chars long
                            if address.starts_with("0x") && address.len() == 42 {
                                result_lines.push(address.to_string());
                            } else {
                                invalid_count += 1;
                            }
                        }
                    } else if record.len() >= 2 {
                        // Other CSV format: assume address,amount format
                        let address = record.get(0).unwrap_or("").trim_matches('"').trim();
                        let amount = record.get(1).unwrap_or("").trim_matches('"').trim();

                        if !address.is_empty() {
                            if address.starts_with("0x") && address.len() == 42 {
                                if amount.is_empty() {
                                    // Just address, will be used for equal distribution
                                    result_lines.push(address.to_string());
                                } else {
                                    // Address with amount
                                    result_lines.push(format!("{},{}", address, amount));
                                }
                            } else {
                                invalid_count += 1;
                            }
                        }
                    } else if record.len() == 1 {
                        // Single column: just addresses
                        let address = record.get(0).unwrap_or("").trim_matches('"').trim();
                        if !address.is_empty() && address.starts_with("0x") && address.len() == 42 {
                            result_lines.push(address.to_string());
                        } else {
                            invalid_count += 1;
                        }
                    }
                }
                Err(e) => {
                    return Err(format!("CSV parsing error at line {}: {}", line_num + 2, e).into());
                }
            }
        }

        // Warn about invalid addresses but don't fail completely
        if invalid_count > 0 {
            tracing::warn!("{} invalid addresses were skipped during CSV import", invalid_count);
        }

        // Join all valid entries with newlines for multi-line input
        Ok(result_lines.join("\n"))
    }

    fn start_ledger_status_check(&mut self) {
        let chain_id = self.config.chain_id;
        let use_native_ledger = self.user_settings.use_native_ledger;
        // Save current status before setting to Checking (for change detection)
        if !matches!(self.ledger_status, LedgerStatus::Checking) {
            self.last_stable_ledger_status = self.ledger_status.clone();
        }
        self.ledger_status = LedgerStatus::Checking;
        self.last_status_check = std::time::Instant::now();

        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = match Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => Ok(runtime.block_on(ledger_dispatch::check_ledger_status(use_native_ledger, chain_id))),
                Err(e) => {
                    tracing::error!("Failed to create async runtime for ledger check: {}", e);
                    Ok(LedgerStatus::Unknown(format!("Runtime error: {}", e)))
                }
            };
            let _ = tx.send(result);
        });
        self.ledger_status_job = Some(AsyncJob::new(rx));
    }

    /// Get a user-friendly message about the ledger status for display near action buttons.
    /// Returns None for Connected or Checking states - only shows actual connection problems.
    pub(crate) fn get_ledger_warning_message(&self) -> Option<String> {
        match &self.ledger_status {
            LedgerStatus::Connected { .. } => None,
            LedgerStatus::Checking => None, // Don't show message while checking
            LedgerStatus::Locked => Some("⚠ Ledger is locked or Ethereum app not open. Please unlock and open the Ethereum app.".to_string()),
            LedgerStatus::Disconnected => Some("⚠ Ledger not connected. Please connect your hardware wallet and open the Ethereum app.".to_string()),
            LedgerStatus::Unknown(msg) => Some(format!("⚠ Ledger status unknown: {}. Please ensure your hardware wallet is connected and unlocked.", msg)),
        }
    }
    
    /// Generate a notification message if the Ledger status has meaningfully changed.
    /// Returns None if no notification should be shown (status unchanged or not worth notifying).
    fn get_ledger_status_change_notification(&self, new_status: &LedgerStatus) -> Option<String> {
        // Skip notification for "Checking" state - it's transient
        if matches!(new_status, LedgerStatus::Checking) {
            return None;
        }
        
        // Compare against last stable status (before "Checking" was set)
        // This prevents notifications every refresh cycle when status hasn't really changed
        let previous_stable = &self.last_stable_ledger_status;
        
        // Check if status has meaningfully changed from the last stable state
        let status_changed = match (previous_stable, new_status) {
            // Same status category - no change
            (LedgerStatus::Connected { .. }, LedgerStatus::Connected { .. }) => false,
            (LedgerStatus::Locked, LedgerStatus::Locked) => false,
            (LedgerStatus::Disconnected, LedgerStatus::Disconnected) => false,
            (LedgerStatus::Checking, _) => true, // Was checking (shouldn't happen), now has result
            // For Unknown, only notify if the message actually changed
            (LedgerStatus::Unknown(old_msg), LedgerStatus::Unknown(new_msg)) => old_msg != new_msg,
            // Any other transition is a change
            _ => true,
        };
        
        if !status_changed {
            return None;
        }
        
        // Generate appropriate notification message
        match new_status {
            LedgerStatus::Connected { address } => {
                let addr_str = format!("{:?}", address);
                Some(format!(
                    "Ledger connected: {}...{}",
                    &addr_str[..8],
                    &addr_str[38..42]
                ))
            }
            LedgerStatus::Disconnected => Some("Ledger disconnected".to_string()),
            LedgerStatus::Locked => Some("Ledger locked or Ethereum app closed".to_string()),
            LedgerStatus::Unknown(msg) => Some(format!("Ledger status: {}", msg)),
            LedgerStatus::Checking => None, // Already handled above
        }
    }

    /// Render a ledger warning message in the UI if the ledger has a connection problem.
    /// Does not show anything during "Checking" state to avoid UI flicker.
    pub(crate) fn render_ledger_warning(&self, ui: &mut egui::Ui) {
        // Only show warnings for actual connection problems (not during checking)
        if let Some(warning) = self.get_ledger_warning_message() {
            ui.add_space(self.theme.spacing_xs);
            ui.horizontal(|ui| {
                ui.label(RichText::new(warning).color(self.theme.warning).size(12.0));
            });
        }
    }


    fn poll_source_selection(state: &mut SourceSelectionState) {
        // Handle traditional scan job
        if let Some(job) = &mut state.scan_job {
            if let Some(res) = job.poll() {
                match res {
                    Ok(scan) => {
                        state.funded_addresses = Some(scan.funded);
                        state.scan_error = None;
                    }
                    Err(e) => {
                        state.scan_error = Some(e.to_string());
                    }
                }
                state.scan_job = None;
            }
        }

        // Handle streaming scan job
        if let Some(job) = &mut state.streaming_scan_job {
            if let Some(res) = job.poll() {
                match res {
                    Ok(_) => {
                        // Streaming job completed successfully
                        // The results should already be in streaming_funded_addresses
                        if state.funded_addresses.is_none() {
                            state.funded_addresses = Some(state.streaming_funded_addresses.clone());
                        }
                        state.scan_error = None;
                    }
                    Err(e) => {
                        state.scan_error = Some(e.to_string());
                    }
                }
                state.streaming_scan_job = None;
                state.cancel_sender = None;
                state.progress_receiver = None;
            }
        }

        // Poll progress receiver for streaming updates
        let mut receiver_finished = false;
        if let Some(ref mut receiver) = state.progress_receiver {
            while let Ok(progress) = receiver.try_recv() {
                match progress {
                    balance::FundedScanProgress::AddressFound(record) => {
                        // Add addresses to appropriate streaming results
                        if record.balance.is_zero() {
                            state.streaming_empty_addresses.push(record);
                        } else {
                            state.streaming_funded_addresses.push(record);
                        }
                    }
                    balance::FundedScanProgress::Completed(scan) => {
                        // Final results - replace streaming results with complete results
                        state.funded_addresses = Some(scan.funded.clone());
                        state.empty_addresses = Some(scan.empty.clone());
                        state.streaming_funded_addresses.clear();
                        state.streaming_empty_addresses.clear();
                        receiver_finished = true;
                    }
                }
            }
        }

        if receiver_finished {
            state.progress_receiver = None;
            state.cancel_sender = None;
        }
    }

    /// Helper to block on async operations in the GUI context
    fn block_on_async<T>(fut: impl std::future::Future<Output = T>) -> T {
        // Try to use existing runtime handle, or create a new one
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            // We're in an async context, use block_in_place
            tokio::task::block_in_place(move || handle.block_on(fut))
        } else {
            // Create a new runtime - this is a critical operation for the UI
            let rt = tokio::runtime::Runtime::new()
                .expect("Failed to create Tokio runtime for async operation - this is a system-level failure");
            rt.block_on(fut)
        }
    }

    fn rerandomize_transaction_amounts(&mut self, selector: SplitSelector) {
        // Only allow re-randomization for random splits
        if let SplitSelector::Random = selector {
            // Check if any transactions have been processed - double-check for safety
            let has_processed_transactions = {
                let state = self.split_state(SplitSelector::Random);
                state.transaction_view.as_ref()
                    .and_then(|tv| {
                        let transactions = Self::block_on_async(tv.queue().get_transactions());
                        Some(transactions.iter().any(|tx| {
                            matches!(tx.status, TransactionStatus::Success { .. } | TransactionStatus::Skipped)
                        }))
                    })
                    .unwrap_or(false)
            };

            if has_processed_transactions {
                // Don't allow re-randomization if transactions have been processed
                return;
            }

            // Extract all needed values first to avoid borrowing conflicts
            let gas_price = self.bulk_disperse_state.current_gas_price.unwrap_or_default();
            let config_clone = self.config.clone();

            // Get the transaction view and queue
            let state = self.split_state(SplitSelector::Random);
            if let Some(tx_view) = &state.transaction_view {
                let queue = tx_view.queue().clone();

                // Extract remaining parameters
                let (gas_speed, source_idx, config_chain_id) = {
                    if let Some(ref prep_params) = state.last_prep_params {
                        let (_, _, speed, src_idx, _) = prep_params;
                        (speed.unwrap_or(config_clone.gas_speed_multiplier), *src_idx, config_clone.chain_id)
                    } else {
                        (config_clone.gas_speed_multiplier, Some(0), config_clone.chain_id)
                    }
                };

                // Create a job to re-randomize amounts for pending transactions
                let rerandomize_job = self.spawn_job({
                    let gas_price_clone = gas_price;
                    let gas_speed_clone = gas_speed;
                    let source_idx_clone = source_idx;
                    let config_chain_id_clone = config_chain_id;
                    move || {
                        let config = config_clone;
                        let gas_price = gas_price_clone;
                        let _gas_speed = gas_speed_clone; // Unused but kept for future use
                        let source_idx = source_idx_clone;
                        let config_chain_id = config_chain_id_clone;
                        async move {
                    use rand::Rng;

                    // Get current transactions
                    let transactions = queue.get_transactions().await;

                    // Find pending transactions and calculate totals
                    let mut pending_tx_ids = Vec::new();
                    let mut total_sent = ethers::types::U256::zero();

                    for tx in &transactions {
                        match &tx.status {
                            TransactionStatus::Pending => {
                                pending_tx_ids.push(tx.id);
                            }
                            TransactionStatus::Success { .. } => {
                                total_sent += tx.transaction.value;
                            }
                            _ => {}
                        }
                    }

                    if pending_tx_ids.is_empty() {
                        return Ok::<(), anyhow::Error>(());
                    }

                    // Get source balance to calculate remaining
                    let provider = config.get_provider().await?;
                    let source_addr = ledger_ops::get_ledger_address_with_config(
                        config_chain_id,
                        source_idx.unwrap_or(0) as u32,
                        Some(&config),
                    ).await?;
                    let source_balance = provider.get_balance(source_addr, None).await?;

                    // Calculate remaining balance available for pending transactions
                    let gas_limit = 25000u64;
                    let tx_fee = gas_price * ethers::types::U256::from(gas_limit);
                    let gas_reserve = tx_fee * ethers::types::U256::from(2);

                    let remaining_balance = source_balance
                        .saturating_sub(total_sent)
                        .saturating_sub(gas_reserve);

                    // Min transfer is 5x base tx fee (gas_speed is already applied to tx_fee)
                    let min_transfer_amount = tx_fee * ethers::types::U256::from(5u64);
                    let num_pending = pending_tx_ids.len() as u64;

                    // Ensure minimum amounts for all pending transactions
                    let total_min_required = min_transfer_amount * ethers::types::U256::from(num_pending);

                    if remaining_balance < total_min_required {
                        return Ok::<(), anyhow::Error>(()); // Not enough balance
                    }

                    let available_for_distribution = remaining_balance - total_min_required;

                    // Generate new random amounts that sum to the remaining balance
                    let mut rng = rand::thread_rng();
                    let mut new_amounts = Vec::new();
                    let mut remaining_to_distribute = available_for_distribution;

                    // Distribute amounts - give minimum + random share to all but last
                    for i in 0..(num_pending - 1) {
                        let max_for_this_tx = remaining_to_distribute / ethers::types::U256::from(num_pending - i);
                        let amount_u128: u128 = max_for_this_tx.try_into().unwrap_or(u128::MAX);
                        let random_ratio: f64 = rng.gen_range(0.0..1.0);
                        let random_amount = (amount_u128 as f64 * random_ratio) as u128;
                        let amount = ethers::types::U256::from(random_amount) + min_transfer_amount;

                        new_amounts.push(amount);
                        remaining_to_distribute = remaining_to_distribute.saturating_sub(amount - min_transfer_amount);
                    }

                    // Give the rest to the last transaction
                    new_amounts.push(remaining_to_distribute + min_transfer_amount);

                    // Update the pending transactions with new amounts
                    for (i, tx_id) in pending_tx_ids.iter().enumerate() {
                        if i < new_amounts.len() {
                            queue.update_pending_transaction_value(*tx_id, new_amounts[i]).await?;
                        }
                    }

                    Ok::<(), anyhow::Error>(())
                        }
                    }
                });

                // Store the job for polling
                let state = self.split_state(SplitSelector::Random);
                state.rerandomize_job = Some(rerandomize_job);
                state.status = Some("Re-randomizing transaction amounts...".to_string());
            }
        }
    }

    pub(crate) fn auto_calculate_amount(&mut self) {
        if self.bulk_disperse_state.recipients_input.trim().is_empty() {
            self.notifications.push_back(NotificationEntry::new("[!!] Please enter recipients first"));
            return;
        }
        
        // Need both gas price and source balance for accurate calculation
        let gas_price = match self.bulk_disperse_state.current_gas_price {
            Some(gp) => gp,
            None => {
                self.notifications.push_back(NotificationEntry::new("[!!] Waiting for gas price..."));
                return;
            }
        };
        
        let source_balance = match self.bulk_disperse_state.source_balance {
            Some(bal) => bal,
            None => {
                self.notifications.push_back(NotificationEntry::new("[!!] Please fetch source balance first (click Refresh)"));
                return;
            }
        };

        match bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input) {
            Ok(disperse_type) => {
                let recipient_count = match &disperse_type {
                    bulk_disperse::BulkDisperseType::Equal(addresses) => addresses.len(),
                    bulk_disperse::BulkDisperseType::Mixed(recipients) => recipients.len(),
                };

                if recipient_count == 0 {
                    self.notifications.push_back(NotificationEntry::new("[!!] No valid recipients found"));
                    return;
                }

                // Get tip amount if specified (sent to tip recipient)
                let tip_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.tip_amount)
                    .unwrap_or(ethers::types::U256::zero());

                // Get remaining balance to keep on source address
                let remaining_balance_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.remaining_balance)
                    .unwrap_or(ethers::types::U256::zero());

                // Calculate gas cost (with speed multiplier)
                // Using updated formula: 150k base + 120k per recipient
                // Note: Add 1 to recipient count if there's a tip (tip recipient added by backend)
                let total_recipients = if tip_wei.is_zero() { recipient_count } else { recipient_count + 1 };
                let gas_limit = calculate_disperse_gas_limit(total_recipients);
                let speed = self.bulk_disperse_state.gas_speed;
                let adjusted_gas_price = gas_price * ethers::types::U256::from((speed * 100.0) as u64) / 100;
                let estimated_gas_cost = adjusted_gas_price * ethers::types::U256::from(gas_limit);
                // Add 5% buffer for gas price fluctuations between calculation and execution
                let estimated_gas_cost_buffered = estimated_gas_cost * 105u64 / 100u64;

                // Calculate available amount: source_balance - remaining - gas_cost (with buffer)
                let reserved = remaining_balance_wei + estimated_gas_cost_buffered;
                
                let native_token = self.config.native_token();
                if source_balance <= reserved {
                    self.notifications.push_back(NotificationEntry::new(format!(
                        "[XX] Insufficient balance! Need more than {} {} (gas+buffer) + {} {} (reserve)",
                        utils::format_ether(estimated_gas_cost_buffered), native_token,
                        utils::format_ether(remaining_balance_wei), native_token
                    )));
                    return;
                }

                let available = source_balance - reserved;

                // For mixed distribution, use specified amounts + tip (backend uses this total)
                // For equal distribution, use available minus tip (backend adds tip back)
                let amount_to_send = match &disperse_type {
                    bulk_disperse::BulkDisperseType::Mixed(recipients) => {
                        let total_specified: ethers::types::U256 = recipients.iter()
                            .map(|(_, amount)| *amount)
                            .fold(ethers::types::U256::zero(), |acc, x| acc + x);
                        
                        // Total needed = specified amounts + tip
                        let total_needed = total_specified + tip_wei;
                        if available < total_needed {
                            self.notifications.push_back(NotificationEntry::new(format!(
                                "[XX] Available {} {} < {} {} needed (specified amounts + tip)",
                                utils::format_ether(available), native_token,
                                utils::format_ether(total_needed), native_token
                            )));
                            self.bulk_disperse_state.amount_input = utils::format_ether(total_needed);
                            return;
                        }
                        // Return specified amounts + tip (backend expects total including tip for mixed)
                        total_needed
                    }
                    bulk_disperse::BulkDisperseType::Equal(_) => {
                        // For equal: subtract tip from available (backend adds it back)
                        if available > tip_wei { available - tip_wei } else { ethers::types::U256::zero() }
                    }
                };
                
                self.bulk_disperse_state.amount_input = utils::format_ether(amount_to_send);
                
                // Show breakdown
                match &disperse_type {
                    bulk_disperse::BulkDisperseType::Mixed(recipients) => {
                        let total_specified: ethers::types::U256 = recipients.iter()
                            .map(|(_, amount)| *amount)
                            .fold(ethers::types::U256::zero(), |acc, x| acc + x);
                        self.notifications.push_back(NotificationEntry::new(format!(
                            "[OK] Mixed distribution: {} {} to {} recipients",
                            utils::format_ether(total_specified), native_token,
                            recipient_count
                        )));
                    }
                    bulk_disperse::BulkDisperseType::Equal(_) => {
                        self.notifications.push_back(NotificationEntry::new(format!(
                            "[OK] Calculated: {} {} available ({} {} source - {} {} gas+buffer - {} {} reserve)",
                            utils::format_ether(amount_to_send), native_token,
                            utils::format_ether(source_balance), native_token,
                            utils::format_ether(estimated_gas_cost_buffered), native_token,
                            utils::format_ether(remaining_balance_wei), native_token
                        )));
                    }
                }
                
                if tip_wei > ethers::types::U256::zero() {
                    self.notifications.push_back(NotificationEntry::new(format!(
                        "[i] {} {} tip included",
                        utils::format_ether(tip_wei), native_token
                    )));
                }
            }
            Err(e) => {
                self.notifications.push_back(NotificationEntry::new(format!("[XX] Parse error: {}", e)));
            }
        }
    }

    /// Silent version of auto_calculate_amount - updates amount without notifications
    /// Used for automatic recalculation when gas speed, tip, or keep on source changes
    pub(crate) fn auto_calculate_amount_silent(&mut self) {
        if self.bulk_disperse_state.recipients_input.trim().is_empty() {
            return;
        }
        
        let gas_price = match self.bulk_disperse_state.current_gas_price {
            Some(gp) => gp,
            None => return,
        };
        
        let source_balance = match self.bulk_disperse_state.source_balance {
            Some(bal) => bal,
            None => return,
        };

        if let Ok(disperse_type) = bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input) {
            let recipient_count = match &disperse_type {
                bulk_disperse::BulkDisperseType::Equal(addresses) => addresses.len(),
                bulk_disperse::BulkDisperseType::Mixed(recipients) => recipients.len(),
            };

            if recipient_count == 0 {
                return;
            }

            // Get remaining balance to keep on source address
            let remaining_balance_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.remaining_balance)
                .unwrap_or(ethers::types::U256::zero());

            // Get tip amount
            let tip_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.tip_amount)
                .unwrap_or(ethers::types::U256::zero());

            // Calculate gas cost (with speed multiplier)
            // Note: Add 1 to recipient count if there's a tip (tip recipient added by backend)
            let total_recipients = if tip_wei.is_zero() { recipient_count } else { recipient_count + 1 };
            let gas_limit = calculate_disperse_gas_limit(total_recipients);
            let speed = self.bulk_disperse_state.gas_speed;
            let adjusted_gas_price = gas_price * ethers::types::U256::from((speed * 100.0) as u64) / 100;
            let estimated_gas_cost = adjusted_gas_price * ethers::types::U256::from(gas_limit);
            // Add 5% buffer for gas price fluctuations between calculation and execution
            let estimated_gas_cost_buffered = estimated_gas_cost * 105u64 / 100u64;

            // Calculate available amount: source_balance - remaining - gas_cost (with buffer)
            let reserved = remaining_balance_wei + estimated_gas_cost_buffered;
            
            if source_balance > reserved {
                // For mixed distribution, use specified amounts + tip (backend expects total)
                // For equal distribution, use available minus tip (backend adds tip back)
                let amount_to_send = match &disperse_type {
                    bulk_disperse::BulkDisperseType::Mixed(recipients) => {
                        let total_specified: ethers::types::U256 = recipients.iter()
                            .map(|(_, amount)| *amount)
                            .fold(ethers::types::U256::zero(), |acc, x| acc + x);
                        // Include tip in total for mixed distribution
                        total_specified + tip_wei
                    }
                    bulk_disperse::BulkDisperseType::Equal(_) => {
                        // For equal: available minus tip (backend adds tip back)
                        let available = source_balance - reserved;
                        if available > tip_wei { available - tip_wei } else { ethers::types::U256::zero() }
                    }
                };
                self.bulk_disperse_state.amount_input = utils::format_ether(amount_to_send);
            }
        }
    }

    // view_settings() and validate_and_save_custom_network() moved to views/settings.rs
    // view_bulk_disperse() moved to views/disperse.rs
    // view_check_balances() moved to views/balances.rs

    fn parse_optional_usize(input: &str) -> Option<usize> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            None
        } else {
            trimmed.parse().ok()
        }
    }

    pub(crate) fn parse_optional_eth_to_wei(input: &str) -> Option<ethers::types::U256> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            None
        } else {
            // Use parse_ether for string parsing (handles large values and preserves precision)
            ethers::utils::parse_ether(trimmed).ok()
        }
    }

    fn split_state(&mut self, selector: SplitSelector) -> &mut SplitState {
        match selector {
            SplitSelector::Random => &mut self.split_random,
            SplitSelector::Equal => &mut self.split_equal,
        }
    }

    pub(crate) fn append_split_recipient(&mut self, selector: SplitSelector, address: &str) {
        let state = self.split_state(selector);
        let recipients = &mut state.recipient_addresses;

        if recipients.trim().is_empty() {
            *recipients = address.to_string();
            return;
        }

        if recipients.trim_end().ends_with(',') {
            if !recipients.ends_with(' ') {
                recipients.push(' ');
            }
        } else {
            recipients.push_str(", ");
        }

        recipients.push_str(address);
    }

    pub(crate) fn append_bulk_disperse_recipient(&mut self, address: &str) {
        let recipients = &mut self.bulk_disperse_state.recipients_input;

        if recipients.trim().is_empty() {
            *recipients = address.to_string();
            return;
        }

        if !recipients.ends_with('\n') {
            recipients.push('\n');
        }

        recipients.push_str(address);
    }


    pub(crate) fn view_split(
        &mut self,
        ui: &mut egui::Ui,
        selector: SplitSelector,
        mode: SplitModeDescriptor,
    ) {
        // Check if we have a transaction view - if so, show only that
        let has_transaction_view = {
            let state = self.split_state(selector);
            state.transaction_view.is_some()
        };

        if has_transaction_view {
            // Show full-screen transaction view
            self.view_split_transactions(ui, selector, mode);
            return;
        }

        // Section header with appropriate icon based on mode
        let icon = match selector {
            SplitSelector::Random => "[~]",
            SplitSelector::Equal => "[=]",
        };
        self.render_section_header(ui, icon, &mode.title.to_uppercase());
        ui.add_space(self.theme.spacing_md);

        // Collect state info first to avoid borrow issues
        let (running, scanning, has_results, manual_source_idx) = {
            let state = self.split_state(selector);
            (
                state.job.as_ref().map(|j| j.is_running()).unwrap_or(false),
                state.source_selection.is_scanning(),
                state.source_selection.has_results(),
                Self::parse_optional_usize(&state.source_index),
            )
        };

        // Parse recipient addresses to determine output count
        let parsed_addresses = {
            let state = self.split_state(selector);
            if !state.recipient_addresses.trim().is_empty() {
                let addresses: Vec<String> = state.recipient_addresses
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                let valid_addresses: Vec<String> = addresses
                    .into_iter()
                    .filter(|addr| addr.starts_with("0x") && addr.len() == 42)
                    .collect();

                if valid_addresses.is_empty() { None } else { Some(valid_addresses) }
            } else {
                None
            }
        };

        // Cache theme colors before mutable borrows
        let accent_green = self.theme.accent_green;
        let text_primary = self.theme.text_primary;
        let text_secondary = self.theme.text_secondary;
        let spacing_md = self.theme.spacing_md;
        let spacing_sm = self.theme.spacing_sm;

        // Parameters panel
        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("Split Parameters").strong().color(text_primary));
            ui.add_space(spacing_sm);

            // Outputs row
            ui.horizontal(|ui| {
                ui.label("Outputs:");
                let state = self.split_state(selector);
                if let Some(ref addresses) = parsed_addresses {
                    ui.label(RichText::new(format!("{} (from addresses)", addresses.len())).color(accent_green));
                    state.output_count = addresses.len() as u32;
                } else {
                    ui.add(
                        egui::DragValue::new(&mut state.output_count)
                            .clamp_range(1..=200)
                            .speed(1),
                    );
                }
            });
            ui.add_space(spacing_sm);

            // Recipients row
            ui.horizontal(|ui| {
                ui.label("Recipients:");
                let state = self.split_state(selector);
                ui.add(egui::TextEdit::singleline(&mut state.recipient_addresses)
                    .desired_width(300.0)
                    .hint_text("comma-separated addresses"));
            });
            ui.add_space(spacing_sm);

            // Gas speed row - slider with labels
            // Cache the default gas speed before mutable borrow
            let default_gas_speed = self.user_settings.default_gas_speed;
            
            ui.horizontal(|ui| {
                ui.label("Gas speed:");
                let state = self.split_state(selector);
                
                // Determine current value (use global default if None)
                let current_speed = state.gas_speed.unwrap_or(default_gas_speed);
                let speed_label = gas_speed_label(current_speed);
                let speed_emoji = gas_speed_emoji(current_speed);
                ui.label(RichText::new(format!("{} {:.1}x ({})", speed_emoji, current_speed, speed_label)).color(accent_green));
            });
            
            ui.horizontal(|ui| {
                ui.label(RichText::new("Slow").small().color(text_secondary));
                let state = self.split_state(selector);
                
                // Use a local variable for the slider, then update state
                let mut speed_value = state.gas_speed.unwrap_or(default_gas_speed);
                if ui.add(egui::Slider::new(&mut speed_value, 0.8..=2.5)
                    .show_value(false)
                    .step_by(0.1)).changed() {
                    state.gas_speed = Some(speed_value);
                }
                ui.label(RichText::new("Aggressive").small().color(text_secondary));
                
                // Reset button to use global default
                if state.gas_speed.is_some() {
                    if ui.small_button("Reset").clicked() {
                        state.gas_speed = None;
                    }
                }
            });
            
            // Show warning for extreme values
            {
                let state = self.split_state(selector);
                let current_speed = state.gas_speed.unwrap_or(default_gas_speed);
                if let Some(warning) = gas_speed_warning(current_speed) {
                    ui.colored_label(egui::Color32::from_rgb(255, 170, 0), warning);
                }
            }
            ui.add_space(spacing_sm);

            // Remaining balance row
            let native_token = self.config.native_token().to_string();
            ui.horizontal(|ui| {
                ui.label("Keep on source:");
                ui.add(egui::TextEdit::singleline(&mut self.split_state(selector).remaining_balance)
                    .desired_width(120.0)
                    .hint_text(format!("0.0 {}", native_token)));
                ui.label(RichText::new(format!("({} amount to keep)", native_token)).small().color(text_secondary));
            });

            // Validate remaining balance input
            {
                let state = self.split_state(selector);
                if !state.remaining_balance.trim().is_empty() {
                    if Self::parse_optional_eth_to_wei(&state.remaining_balance).is_none() {
                        ui.colored_label(egui::Color32::YELLOW, "[!!] Invalid remaining balance format (use decimal numbers like 0.1 or 1.5)");
                    }
                }
            }
            ui.add_space(spacing_sm);

            // Source index row
            ui.horizontal(|ui| {
                ui.label("Source index:");
                let state = self.split_state(selector);
                ui.add(egui::TextEdit::singleline(&mut state.source_index)
                    .desired_width(80.0)
                    .hint_text("auto"));
                ui.label(RichText::new("(leave empty to scan)").small().color(text_secondary));
            });
        });
        
        ui.add_space(spacing_md);


        // Scan parameters (collapsible)
        ui.collapsing("Scan Settings", |ui| {
            ui.horizontal(|ui| {
                ui.label("Start scanning from index:");
                let state = self.split_state(selector);
                ui.add(
                    egui::DragValue::new(&mut state.scan_start_index)
                        .clamp_range(0..=10_000)
                        .speed(1),
                );
            });
            ui.horizontal(|ui| {
                ui.label("Stop after consecutive empties:");
                let state = self.split_state(selector);
                ui.add(
                    egui::DragValue::new(&mut state.scan_empty_streak)
                        .clamp_range(1..=50)
                        .speed(1),
                );
            });
        });

        ui.add_space(self.theme.spacing_sm);

        // Main action button / status
        if running {
            ui.label("[..] Signing transactions... confirm on Ledger if prompted.");
        } else if scanning {
            ui.horizontal(|ui| {
                ui.label("[..] Scanning for funded addresses... keep Ledger unlocked.");
                // Cancel button
                if ui.button("[X] Cancel").clicked() {
                    let state = self.split_state(selector);
                    if let Some(sender) = state.source_selection.cancel_sender.take() {
                        let _ = sender.send(());
                    }
                }
            });
            // Show streaming results during scanning
            if has_results {
                ui.add_space(self.theme.spacing_sm);
                self.render_split_address_selection(ui, selector, mode);
            }

            // Show destination (empty) addresses as they are found
            let empty_addresses = {
                let state = self.split_state(selector);
                state.source_selection.get_all_empty_addresses().into_iter().cloned().collect::<Vec<_>>()
            };

            if !empty_addresses.is_empty() {
                ui.add_space(self.theme.spacing_sm);
                ui.label(
                    RichText::new(format!("Destination addresses found: {}", empty_addresses.len()))
                        .strong()
                        .color(egui::Color32::from_rgb(150, 150, 200)),
                );
                egui::ScrollArea::vertical()
                    .id_source("empty_addresses_scroll")
                    .max_height(100.0)
                    .show(ui, |ui| {
                        for record in &empty_addresses {
                            ui.label(format!(
                                "{} → {:?}",
                                record.derivation_path, record.address
                            ));
                        }
                    });
            }
        } else if has_results && manual_source_idx.is_none() {
            // Show address selection UI (only if no manual source specified)
            self.render_split_address_selection(ui, selector, mode);
        } else {
            // Check if transactions are already displayed
            let has_transaction_view = {
                let state = self.split_state(selector);
                state.transaction_view.is_some()
            };

            if !has_transaction_view {
                // Check ledger status for the button
                // Use is_usable() to allow operations during status checks
                let ledger_ready = self.ledger_status.is_usable();
                let button_hover = if ledger_ready {
                    format!("Start {} operation", mode.title)
                } else {
                    self.get_ledger_warning_message().unwrap_or_else(|| "Ledger not ready".to_string())
                };
                
                // Show run button (disabled if ledger not ready)
                let button_response = ui.add_enabled(
                    ledger_ready, 
                    mode.themed_button(&self.theme)
                ).on_hover_text(&button_hover);
                
                // Show ledger warning below button if not ready
                if !ledger_ready {
                    self.render_ledger_warning(ui);
                }
                
                if button_response.clicked() {
                    // Extract values first to avoid borrow issues
                    let (source_idx, outputs, gas_speed, remaining_balance, start_idx, empty_streak) = {
                        let state = self.split_state(selector);
                        // Convert U256 to u64 for split operations (capped at u64::MAX)
                        let remaining_balance_u64 = Self::parse_optional_eth_to_wei(&state.remaining_balance)
                            .and_then(|u| u.try_into().ok());
                        (
                            Self::parse_optional_usize(&state.source_index),
                            state.output_count,
                            state.gas_speed,
                            remaining_balance_u64,
                            state.scan_start_index,
                            state.scan_empty_streak,
                        )
                    };

                if let Some(idx) = source_idx {
                    // Direct execution with specified index - parse recipient addresses
                    let recipient_addresses = {
                        let state = self.split_state(selector);
                        if !state.recipient_addresses.trim().is_empty() {
                            let addresses: Vec<String> = state.recipient_addresses
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty() && s.starts_with("0x") && s.len() == 42)
                                .collect();
                            if addresses.is_empty() { None } else { Some(addresses) }
                        } else {
                            None
                        }
                    };
                    self.start_split_job(selector, mode.kind, outputs, gas_speed, Some(idx), recipient_addresses, remaining_balance);
                } else {
                    // Start scanning for funded addresses
                    // Reset any existing transaction view and source selection state first
                    {
                        let state = self.split_state(selector);
                        state.transaction_view = None;
                        state.source_selection.reset();
                        state.status = None;
                    }

                    let config = self.config.clone();
                    let use_native_ledger = self.user_settings.use_native_ledger;
                    // Create channels for progress and cancellation
                    let (progress_sender, progress_receiver) = tokio::sync::mpsc::unbounded_channel();
                    let (cancel_sender, cancel_receiver) = tokio::sync::oneshot::channel();

                    // Store receivers and cancel sender
                    {
                        let state = self.split_state(selector);
                        state.source_selection.progress_receiver = Some(progress_receiver);
                        state.source_selection.cancel_sender = Some(cancel_sender);
                    }

                    // Start the streaming scan
                    let streaming_scan_job = self.spawn_job(move || async move {
                        balance::scan_for_funded_addresses_streaming(
                            config,
                            start_idx,
                            empty_streak,
                            progress_sender,
                            cancel_receiver,
                            use_native_ledger,
                        ).await
                    });

                    let state = self.split_state(selector);
                    state.source_selection.streaming_scan_job = Some(streaming_scan_job);
                }
                }
            }
        }

        // Show scan error if any
        {
            let state = self.split_state(selector);
            if let Some(err) = &state.source_selection.scan_error {
                ui.colored_label(egui::Color32::LIGHT_RED, format!("Scan error: {}", err));
            }
        }

        // Status
        {
            let spacing_xs = self.theme.spacing_xs;
            let state = self.split_state(selector);
            if let Some(status) = &state.status {
                ui.add_space(spacing_xs);
                ui.label(status);
            }
        }

        // Poll preparation progress and job
        {
            // Poll progress receiver for real-time updates
            let progress_update = {
                let state = self.split_state(selector);
                if let Some(ref mut receiver) = state.prep_progress_receiver {
                    receiver.try_recv().ok()
                } else {
                    None
                }
            };

            // Update status based on progress
            if let Some(progress) = progress_update {
                let status_msg = match progress {
                    split_operations::PrepareProgress::CheckingPreFound { current, total, found_empty } => {
                        format!("◐ Verifying pre-found address {}/{} (empty: {})", current, total, found_empty)
                    }
                    split_operations::PrepareProgress::ScanningIndex { index, found_empty, needed } => {
                        format!("◐ Scanning index {} (found {}/{} empty)", index, found_empty, needed)
                    }
                    split_operations::PrepareProgress::BuildingTransactions { current, total } => {
                        format!("◐ Building transaction {}/{}", current, total)
                    }
                    split_operations::PrepareProgress::Complete { total_transactions } => {
                        format!("[OK] Prepared {} transactions", total_transactions)
                    }
                };
                let state = self.split_state(selector);
                state.status = Some(status_msg);
            }

            let poll_result = {
                let state = self.split_state(selector);
                if let Some(job) = &mut state.prep_job {
                    job.poll()
                } else {
                    None
                }
            };

            if let Some(res) = poll_result {
                match res {
                    Ok((queue, total)) => {
                        let chain_id = self.config.chain_id;
                        let state = self.split_state(selector);
                        let transaction_view = if let SplitSelector::Random = selector {
                            TransactionView::with_rerandomize(queue, chain_id)
                        } else {
                            TransactionView::new(queue, chain_id)
                        };
                        state.transaction_view = Some(transaction_view);
                        state.status = Some(format!("[OK] Prepared {} transactions. Ready to sign!", total));
                        state.prep_job = None;
                        state.prep_progress_receiver = None;
                        self.notifications.push_back(NotificationEntry::new(format!("Prepared {} transactions", total)));
                    }
                    Err(e) => {
                        let state = self.split_state(selector);
                        state.status = Some(format!("[!!] Failed to prepare: {}", e));
                        state.prep_job = None;
                        state.prep_progress_receiver = None;
                        self.notifications.push_back(NotificationEntry::new(format!("Preparation failed: {}", e)));
                    }
                }
            }
        }

        // Poll rerandomize job
        {
            let poll_result = {
                let state = self.split_state(selector);
                if let Some(job) = &mut state.rerandomize_job {
                    job.poll()
                } else {
                    None
                }
            };

            if let Some(res) = poll_result {
                match res {
                    Ok(()) => {
                        let state = self.split_state(selector);
                        state.status = Some("[OK] Transaction amounts re-randomized!".to_string());
                        state.rerandomize_job = None;
                        self.notifications.push_back(NotificationEntry::new("Transaction amounts re-randomized"));
                    }
                    Err(e) => {
                        let state = self.split_state(selector);
                        state.status = Some(format!("[!!] Failed to re-randomize: {}", e));
                        state.rerandomize_job = None;
                        self.notifications.push_back(NotificationEntry::new(format!("Re-randomization failed: {}", e)));
                    }
                }
            }
        }
    }

    fn render_split_address_selection(
        &mut self,
        ui: &mut egui::Ui,
        selector: SplitSelector,
        mode: SplitModeDescriptor,
    ) {
        let (_, native_token, _, _) = self.selected_network_info();
        let mut selected_idx: Option<usize> = None;
        // Use is_usable() to allow operations during status checks
        let ledger_ready = self.ledger_status.is_usable();

        ui.label(
            RichText::new("Select source address:")
                .strong()
                .color(egui::Color32::from_rgb(100, 200, 150)),
        );
        ui.add_space(self.theme.spacing_xs);
        
        // Show ledger warning if not ready
        if !ledger_ready {
            self.render_ledger_warning(ui);
            ui.add_space(self.theme.spacing_xs);
        }

        // Get all funded addresses (streaming or completed)
        let funded_addresses = {
            let state = self.split_state(selector);
            state.source_selection.get_all_funded_addresses().into_iter().cloned().collect::<Vec<_>>()
        };

        if funded_addresses.is_empty() {
            ui.colored_label(
                egui::Color32::YELLOW,
                "No funded addresses found. Please fund an address first.",
            );
            if ui.button("Cancel").clicked() {
                let state = self.split_state(selector);
                state.source_selection.reset();
            }
        } else {
            let button_hover = if ledger_ready {
                format!("Start {} from this address", mode.title)
            } else {
                self.get_ledger_warning_message().unwrap_or_else(|| "Ledger not ready".to_string())
            };
            
            // Copy all addresses button
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} funded addresses:", funded_addresses.len())).small().color(self.theme.text_secondary));
                if ui.add(egui::Button::new("📋 Copy All").small())
                    .on_hover_text("Copy all addresses to clipboard")
                    .clicked() 
                {
                    let addresses: Vec<String> = funded_addresses
                        .iter()
                        .map(|r| format!("{:?}", r.address))
                        .collect();
                    ui.output_mut(|o| o.copied_text = addresses.join("\n"));
                    self.notifications.push_back(NotificationEntry::new(format!("[OK] {} addresses copied to clipboard", funded_addresses.len())));
                }
            });
            
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    for record in &funded_addresses {
                        ui.horizontal(|ui| {
                            let addr_short = format!(
                                "{:?}",
                                record.address
                            );
                            let balance_str = utils::format_ether(record.balance);
                            ui.label(format!(
                                "{} → {} — {} {}",
                                record.derivation_path, addr_short, balance_str, native_token
                            ));
                            if ui.add_enabled(ledger_ready, mode.themed_button(&self.theme))
                                .on_hover_text(&button_hover)
                                .clicked()
                            {
                                selected_idx = Some(record.index as usize);
                            }
                        });
                    }
                });

            ui.add_space(self.theme.spacing_xs);
            if ui.button("Cancel").clicked() {
                let state = self.split_state(selector);
                state.source_selection.reset();
            }
        }

        // Handle selection
        if let Some(idx) = selected_idx {
            let state = self.split_state(selector);
            let outputs = state.output_count;
            let gas_speed = state.gas_speed;
            // Convert U256 to u64 for split operations (capped at u64::MAX)
            let remaining_balance = Self::parse_optional_eth_to_wei(&state.remaining_balance)
                .and_then(|u| u.try_into().ok());

            // Parse recipient addresses
            let recipient_addresses = if !state.recipient_addresses.trim().is_empty() {
                let addresses: Vec<String> = state.recipient_addresses
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty() && s.starts_with("0x") && s.len() == 42)
                    .collect();
                if addresses.is_empty() { None } else { Some(addresses) }
            } else {
                None
            };

            state.source_selection.reset();
            self.start_split_job(selector, mode.kind, outputs, gas_speed, Some(idx), recipient_addresses, remaining_balance);
        }
    }

    /// Full-screen transaction view for split operations
    fn view_split_transactions(
        &mut self,
        ui: &mut egui::Ui,
        selector: SplitSelector,
        mode: SplitModeDescriptor,
    ) {
        // Check if all transactions are complete to change button text
        let all_complete = {
            let state = self.split_state(selector);
            if let Some(tx_view) = &state.transaction_view {
                let stats = Self::block_on_async(tx_view.queue().get_statistics());
                stats.is_complete() && stats.total > 0
            } else {
                false
            }
        };
        
        // Header with back button
        ui.horizontal(|ui| {
            let icon = match selector {
                SplitSelector::Random => "[~]",
                SplitSelector::Equal => "[=]",
            };
            ui.heading(RichText::new(format!("{} {} - Transaction Queue", icon, mode.title.to_uppercase())));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Back/Cancel button - changes to "New Transaction" when complete
                let button_text = if all_complete { "[←] New Transaction" } else { "[←] Cancel" };
                if ui.add(self.theme.button_warning(button_text)).clicked() {
                    let state = self.split_state(selector);
                    state.transaction_view = None;
                    state.source_selection.reset();
                    state.status = None;
                }

            });
        });

        ui.add_space(self.theme.spacing_sm);

        // Status line
        {
            let state = self.split_state(selector);
            if let Some(status) = &state.status {
                ui.label(RichText::new(status).color(self.theme.text_secondary));
            }
        }

        ui.separator();

        // Full-height transaction view
        // Get ledger status before mutable borrow
        // Use is_usable() to allow operations during status checks
        let ledger_ready = self.ledger_status.is_usable();
        let ledger_warning = self.get_ledger_warning_message();
        let native_token = self.config.native_token().to_string();
        let mut needs_rerandomize = false;
        let mut tx_notifications = Vec::new();
        {
            let state = self.split_state(selector);
            if let Some(tx_view) = &mut state.transaction_view {
                // Use remaining available space for transaction list
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        tx_view.show(ui, ledger_ready, ledger_warning.as_deref(), &native_token);
                    });

                // Check if re-randomize was requested
                if tx_view.take_rerandomize_request() {
                    needs_rerandomize = true;
                }
                
                // Collect any transaction notifications
                tx_notifications = tx_view.take_notifications();
            }
        }
        // Add transaction notifications to the main notification queue
        for notification in tx_notifications {
            self.notifications.push_back(NotificationEntry::new(notification));
        }
        if needs_rerandomize {
            self.rerandomize_transaction_amounts(selector);
        }

        // Poll preparation progress and job (for any ongoing preparations)
        {
            let progress_update = {
                let state = self.split_state(selector);
                if let Some(ref mut receiver) = state.prep_progress_receiver {
                    receiver.try_recv().ok()
                } else {
                    None
                }
            };

            if let Some(progress) = progress_update {
                let status_msg = match progress {
                    split_operations::PrepareProgress::CheckingPreFound { current, total, found_empty } => {
                        format!("◐ Verifying pre-found address {}/{} (empty: {})", current, total, found_empty)
                    }
                    split_operations::PrepareProgress::ScanningIndex { index, found_empty, needed } => {
                        format!("◐ Scanning index {} (found {}/{} empty)", index, found_empty, needed)
                    }
                    split_operations::PrepareProgress::BuildingTransactions { current, total } => {
                        format!("◐ Building transaction {}/{}", current, total)
                    }
                    split_operations::PrepareProgress::Complete { total_transactions } => {
                        format!("[OK] Prepared {} transactions", total_transactions)
                    }
                };
                let state = self.split_state(selector);
                state.status = Some(status_msg);
            }

            let poll_result = {
                let state = self.split_state(selector);
                if let Some(job) = &mut state.prep_job {
                    job.poll()
                } else {
                    None
                }
            };

            if let Some(res) = poll_result {
                match res {
                    Ok((queue, total)) => {
                        let chain_id = self.config.chain_id;
                        let state = self.split_state(selector);
                        let transaction_view = if let SplitSelector::Random = selector {
                            TransactionView::with_rerandomize(queue, chain_id)
                        } else {
                            TransactionView::new(queue, chain_id)
                        };
                        state.transaction_view = Some(transaction_view);
                        state.status = Some(format!("[OK] Prepared {} transactions. Ready to sign!", total));
                        state.prep_job = None;
                        state.prep_progress_receiver = None;
                        state.operation_logged = false; // Reset logging flag for new operation
                        self.notifications.push_back(NotificationEntry::new(format!("Prepared {} transactions", total)));
                    }
                    Err(e) => {
                        let state = self.split_state(selector);
                        state.status = Some(format!("[!!] Failed to prepare: {}", e));
                        state.prep_job = None;
                        state.prep_progress_receiver = None;
                        self.notifications.push_back(NotificationEntry::new(format!("Preparation failed: {}", e)));
                    }
                }
            }
        }

        // Check if operation is complete and log it
        self.check_and_log_split_completion(selector, &mode);
    }

    /// Check if a split operation is complete and log it to the operation log
    fn check_and_log_split_completion(&mut self, selector: SplitSelector, mode: &SplitModeDescriptor) {
        let chain_id = self.config.chain_id;
        let native_token = self.config.native_token().to_string();
        let network_label = self.config.network_label().to_string();

        let state = self.split_state(selector);
        
        // Skip if already logged or no transaction view
        if state.operation_logged {
            return;
        }

        let tx_view = match &state.transaction_view {
            Some(tv) => tv,
            None => return,
        };

        // Get queue statistics
        let stats = Self::block_on_async(tx_view.queue().get_statistics());
        
        // Check if operation is complete (no pending or in-progress)
        if !stats.is_complete() || stats.total == 0 {
            return;
        }

        // Get all transactions for detailed logging
        let transactions = Self::block_on_async(tx_view.queue().get_transactions());
        
        // Build transaction details
        let mut tx_details = Vec::new();
        let mut total_sent = ethers::types::U256::zero();
        
        for tx in &transactions {
            let status_str = match &tx.status {
                TransactionStatus::Success { tx_hash, .. } => {
                    total_sent += tx.transaction.value;
                    format!("✓ {:?}", tx_hash)
                }
                TransactionStatus::Failed { error, .. } => format!("✗ Failed: {}", error),
                TransactionStatus::Skipped => "⏭ Skipped".to_string(),
                _ => "?".to_string(),
            };
            
            tx_details.push(format!(
                "  {}. {:?} → {} {} [{}]",
                tx.id + 1,
                tx.transaction.to,
                utils::format_ether(tx.transaction.value),
                native_token,
                status_str
            ));
        }

        let operation_name = match selector {
            SplitSelector::Random => "Beaug Split Random",
            SplitSelector::Equal => "Beaug Split Equal",
        };

        let details = format!(
            "{} on {} (Chain ID: {})\n\
             Summary: {} total, {} success, {} failed, {} skipped\n\
             Total distributed: {} {}\n\
             Transactions:\n{}",
            mode.title,
            network_label,
            chain_id,
            stats.total,
            stats.success,
            stats.failed,
            stats.skipped,
            utils::format_ether(total_sent),
            native_token,
            tx_details.join("\n")
        );

        // Log the operation
        if let Err(e) = crate::operation_log::append_log(operation_name, chain_id, &details) {
            tracing::warn!("Failed to log split operation: {}", e);
        }

        // Mark as logged
        let state = self.split_state(selector);
        state.operation_logged = true;
        state.status = Some(format!("[OK] Complete! {} success, {} failed, {} skipped", stats.success, stats.failed, stats.skipped));
    }

    /// Log a balance scan operation to the operation log
    fn log_balance_scan(&self, result: &balance::BalanceScanResult) {
        let chain_id = self.config.chain_id;
        let native_token = self.config.native_token();
        let network_label = self.config.network_label();

        let funded_count = result.records.iter().filter(|r| !r.balance.is_zero()).count();
        let empty_count = result.records.iter().filter(|r| r.balance.is_zero()).count();

        // Calculate total balance
        let total_balance: ethers::types::U256 = result.records
            .iter()
            .map(|r| r.balance)
            .fold(ethers::types::U256::zero(), |acc, b| acc + b);

        // Build address details
        let address_lines: Vec<String> = result.records
            .iter()
            .map(|r| {
                let status = if r.balance.is_zero() { "⚪" } else { "🟢" };
                format!(
                    "  {} {} → {:?} - {} {}",
                    status,
                    r.derivation_path,
                    r.address,
                    utils::format_ether(r.balance),
                    native_token
                )
            })
            .collect();

        let details = format!(
            "Beaug Balance Scan on {} (Chain ID: {})\n\
             Scanned {} addresses: {} funded, {} empty\n\
             Total balance found: {} {}\n\
             Met target: {} | Cancelled: {}\n\
             Addresses:\n{}",
            network_label,
            chain_id,
            result.records.len(),
            funded_count,
            empty_count,
            utils::format_ether(total_balance),
            native_token,
            result.met_target,
            result.cancelled,
            address_lines.join("\n")
        );

        if let Err(e) = crate::operation_log::append_log("Beaug Balance Scan", chain_id, &details) {
            tracing::warn!("Failed to log balance scan: {}", e);
        }
    }

    fn start_split_job(
        &mut self,
        selector: SplitSelector,
        split_mode: SplitMode,
        outputs: u32,
        gas_speed: Option<f32>,
        source_idx: Option<usize>,
        recipient_addresses: Option<Vec<String>>,
        remaining_balance: Option<u64>,
    ) {
        let config = self.config.clone();
        let delay_ms = self.split_state(selector).transaction_delay_ms;
        let use_native_ledger = self.user_settings.use_native_ledger;

        // Store parameters for re-randomization
        let state = self.split_state(selector);
        state.last_prep_params = Some((split_mode, outputs, gas_speed, source_idx, recipient_addresses.clone()));

        // Get any pre-found empty addresses from the source selection scan
        let pre_found_empty_addresses = if recipient_addresses.is_none() {
            // Only use pre-found empty addresses if we're not using custom recipient addresses
            let empty_addrs = state.source_selection.get_all_empty_addresses();
            if empty_addrs.is_empty() {
                None
            } else {
                Some(empty_addrs.into_iter().cloned().collect())
            }
        } else {
            None
        };

        // Get the scan start index from state
        let scan_start_index = state.scan_start_index;

        // Create a progress channel for real-time updates
        let (progress_sender, progress_receiver) = tokio::sync::mpsc::unbounded_channel();

        // Create a job to prepare transactions
        let prep_job = self.spawn_job(move || async move {
            // Prepare transactions
            let (tx_list, manager_arc) = split_operations::prepare_split_transactions(
                config.clone(),
                outputs,
                gas_speed,
                match split_mode {
                    SplitMode::Random => split_operations::SplitMode::Random,
                    SplitMode::Equal => split_operations::SplitMode::Equal,
                },
                source_idx,
                recipient_addresses,
                pre_found_empty_addresses,
                scan_start_index,
                Some(progress_sender),
                remaining_balance,
                use_native_ledger,
            )
            .await?;

            // Create transaction queue with custom delay
            let mut queue = TransactionQueue::with_delay(delay_ms);
            
            // Note: The delay is managed by the queue itself
            // The manager uses its own configuration which could be updated in the future
            
            queue.set_manager(manager_arc);
            queue.add_transactions(tx_list.clone()).await;

            Ok::<(TransactionQueue, usize), anyhow::Error>((queue, tx_list.len()))
        });

        let state = self.split_state(selector);
        state.status = Some("Preparing transactions...".into());
        state.prep_job = Some(prep_job);
        state.prep_progress_receiver = Some(progress_receiver);
        state.transaction_view = None; // Clear any previous view
    }


}

impl App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        self.poll_jobs();

        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            // Add vertical padding above the logo
            ui.add_space(10.0);

            ui.horizontal_wrapped(|ui| {
                // Display the Beaug WebP logo
                let logo_size = egui::vec2(120.0, 28.0); // Adjust size as needed

                // Load texture if not already loaded
                if self.logo_texture.is_none() {
                    match egui_extras::image::load_image_bytes(BEAUG_LOGO_WEBP) {
                        Ok(image) => {
                            tracing::debug!("WebP logo loaded successfully, size: {}x{}", image.width(), image.height());
                            self.logo_texture = Some(ctx.load_texture("beaug_logo", image, Default::default()));
                        }
                        Err(e) => {
                            tracing::warn!("Failed to load WebP logo: {}", e);
                        }
                    }
                }

                // Show the logo if texture is available
                if let Some(texture) = &self.logo_texture {
                    let image = egui::Image::new(texture)
                        .fit_to_exact_size(logo_size);
                    ui.add(image);
                } else {
                    // Fallback text if image fails to load
                    ui.heading(
                        RichText::new("🔗 Beaug")
                            .size(28.0)
                            .color(egui::Color32::from_rgb(173, 216, 230)),
                    );
                }
                // Version number next to logo (uses Cargo.toml version)
                ui.label(RichText::new(format!("v{}", env!("CARGO_PKG_VERSION"))).size(12.0).color(self.theme.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Network selector dropdown (rightmost)
                    let (current_label, current_token, _, _) = self.selected_network_info();
                    let display_text = format!("{} ({})", current_label, current_token);
                    let mut changed = false;
                    let mut new_selection = self.network_selection.clone();
                    egui::ComboBox::from_id_source("network_selector")
                        .selected_text(&display_text)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            ui.set_min_width(240.0);
                            let mut last_category: Option<NetworkCategory> = None;
                            
                            // Built-in networks
                            for (idx, network) in NETWORKS.iter().enumerate() {
                                // Insert section header when category changes
                                if last_category != Some(network.category) {
                                    if last_category.is_some() {
                                        ui.separator();
                                    }
                                    let header = match network.category {
                                        NetworkCategory::EthereumMainnet => "── Ethereum ──",
                                        NetworkCategory::EthereumTestnet => "── Ethereum Testnets ──",
                                        NetworkCategory::L2Mainnet => "── L2 Networks ──",
                                        NetworkCategory::OtherMainnet => "── Other Chains ──",
                                        NetworkCategory::L2Testnet => "── L2 Testnets ──",
                                    };
                                    ui.label(
                                        RichText::new(header)
                                            .color(egui::Color32::from_rgb(120, 140, 180))
                                            .small(),
                                    );
                                    last_category = Some(network.category);
                                }
                                let label = format!(
                                    "{} · {} · #{}",
                                    network.label, network.native_token, network.chain_id
                                );
                                let is_selected = self.network_selection == NetworkSelection::Builtin(idx);
                                if ui.selectable_label(is_selected, &label).clicked() {
                                    new_selection = NetworkSelection::Builtin(idx);
                                    changed = true;
                                }
                            }
                            
                            // Custom networks section
                            if !self.user_settings.custom_networks.is_empty() {
                                ui.separator();
                                ui.label(
                                    RichText::new("── Custom Networks ──")
                                        .color(egui::Color32::from_rgb(180, 140, 200))
                                        .small(),
                                );
                                for net in &self.user_settings.custom_networks {
                                    let label = format!(
                                        "{} · {} · #{}",
                                        net.label, net.native_token, net.chain_id
                                    );
                                    let is_selected = self.network_selection == NetworkSelection::Custom(net.chain_id);
                                    if ui.selectable_label(is_selected, &label).clicked() {
                                        new_selection = NetworkSelection::Custom(net.chain_id);
                                        changed = true;
                                    }
                                }
                            }
                        });
                    if changed {
                        let (label, _, _, _) = match &new_selection {
                            NetworkSelection::Builtin(idx) => {
                                let net = &NETWORKS[*idx];
                                (net.label.to_string(), net.native_token.to_string(), net.chain_id, net.default_rpc.to_string())
                            }
                            NetworkSelection::Custom(chain_id) => {
                                if let Some(net) = self.user_settings.get_custom_network(*chain_id) {
                                    (net.label.clone(), net.native_token.clone(), net.chain_id, net.rpc_url.clone())
                                } else {
                                    ("Unknown".to_string(), "ETH".to_string(), *chain_id, String::new())
                                }
                            }
                        };
                        self.network_selection = new_selection;
                        self.apply_network_selection();
                        self.notifications
                            .push_back(NotificationEntry::new(format!("Switched to {}", label)));
                    }

                    ui.add_space(self.theme.spacing_md);
                    
                    // Derivation path display
                    ui.allocate_ui_with_layout(
                        egui::vec2(150.0, 28.0),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(35, 45, 60))
                                .rounding(4.0)
                                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let path_pattern = match self.config.derivation_mode {
                                            crate::config::DerivationMode::AccountIndex => {
                                                format!("m/44'/{}'/i'/0/{}", self.config.coin_type, self.config.custom_address_index)
                                            }
                                            crate::config::DerivationMode::AddressIndex => {
                                                format!("m/44'/{}'/{}'/0/i", self.config.coin_type, self.config.custom_account)
                                            }
                                        };
                                        ui.monospace(
                                            RichText::new(&path_pattern)
                                                .color(egui::Color32::from_rgb(180, 200, 230))
                                                .size(12.0),
                                        );
                                    });
                                });
                        },
                    );
                    
                    ui.add_space(self.theme.spacing_sm);

                    // Ledger status indicator - fixed width, subtle design
                    // Icon color changes based on status, text stays consistent
                    let (status_icon, status_color, status_hover) = match &self.ledger_status {
                        LedgerStatus::Connected { address } => {
                            let addr_str = format!("{:?}", address);
                            let short_addr = format!("{}...{}", &addr_str[..6], &addr_str[38..42]);
                            ("●", egui::Color32::from_rgb(50, 205, 50), format!("Connected: {}", short_addr))
                        }
                        LedgerStatus::Locked => ("●", egui::Color32::from_rgb(255, 193, 7), "Ledger locked or Ethereum app not open".to_string()),
                        LedgerStatus::Disconnected => ("●", egui::Color32::from_rgb(220, 53, 69), "Ledger not connected".to_string()),
                        LedgerStatus::Checking => ("◐", egui::Color32::from_rgb(100, 149, 237), "Checking Ledger status...".to_string()),
                        LedgerStatus::Unknown(msg) => ("●", egui::Color32::from_rgb(150, 150, 150), format!("Unknown: {}", msg)),
                    };

                    ui.allocate_ui_with_layout(
                        egui::vec2(100.0, 28.0),  // Fixed narrow width
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            egui::Frame::none()
                                .fill(egui::Color32::from_rgb(25, 35, 50))
                                .rounding(4.0)
                                .inner_margin(egui::Margin::symmetric(8.0, 4.0))
                                .show(ui, |ui| {
                                    ui.set_min_width(80.0);
                                    ui.horizontal(|ui| {
                                        // Refresh button (subtle, icon only when checking)
                                        let refresh_icon = if self.ledger_status_job.is_some() { "↻" } else { "[R]" };
                                        if ui
                                            .add_enabled(
                                                self.ledger_status_job.is_none(),
                                                egui::Button::new(
                                                    egui::RichText::new(refresh_icon)
                                                        .color(self.theme.text_primary)
                                                        .size(11.0)
                                                )
                                                    .fill(self.theme.secondary)
                                                    .stroke(egui::Stroke::new(1.0, self.theme.surface_active))
                                                    .small(),
                                            )
                                            .on_hover_text("Refresh Ledger status")
                                            .clicked()
                                        {
                                            self.start_ledger_status_check();
                                        }
                                        
                                        // Status icon (color indicates state) + fixed label
                                        ui.label(
                                            RichText::new(status_icon)
                                                .color(status_color)
                                                .size(14.0),
                                        ).on_hover_text(&status_hover);
                                        
                                        ui.label(
                                            RichText::new("Ledger")
                                                .color(self.theme.text_secondary)
                                                .size(12.0),
                                        ).on_hover_text(&status_hover);
                                    });
                                });
                        },
                    );

                });
            });
        });

        // Check for new notifications and trigger toast
        let current_notification_count = self.notifications.len();
        if current_notification_count > self.last_notification_count {
            // New notification arrived - show toast
            self.notification_toast_visible = true;
            self.notification_toast_close_time = Some(std::time::Instant::now() + std::time::Duration::from_secs(5));
        }
        self.last_notification_count = current_notification_count;

        // Auto-close toast after timeout
        if let Some(close_time) = self.notification_toast_close_time {
            if std::time::Instant::now() >= close_time {
                self.notification_toast_visible = false;
                self.notification_toast_close_time = None;
            }
        }

        // Notification toast/icon overlay - top right corner below title bar
        let notification_count = self.notifications.len();
        let has_notifications = notification_count > 0;
        let latest_notification = self.notifications.back().map(|n| n.message.clone());

        egui::Area::new(egui::Id::new("notification_overlay"))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-10.0, -10.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(35, 45, 60))
                    .rounding(6.0)
                    .stroke(egui::Stroke::new(1.0, self.theme.primary))
                    .inner_margin(egui::Margin::symmetric(8.0, 6.0))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // ASCII-style notification icon
                            let icon_color = if has_notifications {
                                self.theme.accent_green
                            } else {
                                self.theme.text_secondary
                            };

                            // Click on icon to toggle history
                            if ui.add(
                                egui::Button::new(
                                    RichText::new("[!]")
                                        .size(14.0)
                                        .color(icon_color)
                                        .strong()
                                )
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                            ).on_hover_text("Click to view notification history").clicked() {
                                self.show_notifications_popup = !self.show_notifications_popup;
                            }

                            // Show expanded notification if toast is visible
                            if self.notification_toast_visible {
                                if let Some(ref msg) = latest_notification {
                                    ui.add_space(4.0);
                                    // Truncate long messages
                                    let display_text = if msg.len() > 40 {
                                        format!("{}...", &msg[..40])
                                    } else {
                                        msg.clone()
                                    };
                                    ui.label(
                                        RichText::new(&display_text)
                                            .size(12.0)
                                            .color(self.theme.text_primary)
                                    );
                                }
                            } else if has_notifications {
                                // Show notification count badge when collapsed
                                ui.add_space(2.0);
                                ui.label(
                                    RichText::new(format!("{}", notification_count))
                                        .size(10.0)
                                        .color(self.theme.accent_orange)
                                );
                            }
                        });
                    });
            });

        // Notification history popup window
        if self.show_notifications_popup {
            egui::Window::new("[#] Notification History")
                .collapsible(false)
                .resizable(true)
                .default_width(450.0)
                .default_height(350.0)
                .anchor(egui::Align2::RIGHT_BOTTOM, [-10.0, -50.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} notifications", self.notifications.len())).color(self.theme.text_secondary));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(
                                egui::Button::new(RichText::new("[X] Close").color(self.theme.text_primary))
                                    .fill(self.theme.secondary)
                            ).clicked() {
                                self.show_notifications_popup = false;
                            }
                            if ui.add(
                                egui::Button::new(RichText::new("[C] Clear").color(self.theme.text_primary))
                                    .fill(self.theme.secondary)
                            ).clicked() {
                                self.notifications.clear();
                            }
                        });
                    });
                    ui.add_space(self.theme.spacing_xs);
                    ui.label(RichText::new("-".repeat(50)).size(10.0).color(self.theme.primary));
                    ui.add_space(self.theme.spacing_xs);

                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .max_height(280.0)
                        .show(ui, |ui| {
                            if self.notifications.is_empty() {
                                ui.label(RichText::new("No notifications yet.").color(self.theme.text_secondary));
                            } else {
                                for notification in self.notifications.iter().rev() {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!("[{}]", notification.time_ago()))
                                                .size(11.0)
                                                .color(self.theme.text_secondary)
                                        );
                                        ui.label(
                                            RichText::new(&notification.message)
                                                .size(12.0)
                                                .color(self.theme.text_primary)
                                        );
                                    });
                                    ui.add_space(3.0);
                                }
                            }
                        });
                });
        }

        egui::SidePanel::left("nav")
            .resizable(false)
            .default_width(180.0)
            .frame(egui::Frame::none()
                .fill(self.theme.surface)
                .stroke(egui::Stroke::new(1.0, self.theme.primary)))
            .show(ctx, |ui| {
                ui.add_space(self.theme.spacing_md);
                
                ui.horizontal(|ui| {
                    ui.add_space(self.theme.spacing_xs);
                    ui.label(RichText::new("-".repeat(22)).size(10.0).color(self.theme.primary));
                });
                ui.add_space(self.theme.spacing_sm);

                let nav_items = [
                    (GuiSection::Dashboard, "[H] Dashboard"),
                    (GuiSection::CheckBalances, "[?] Scan Addresses"),
                    (GuiSection::SplitRandom, "[~] Split Random"),
                    (GuiSection::SplitEqual, "[=] Split Even"),
                    (GuiSection::BulkDisperse, "[$] Bulk Disperse"),
                    (GuiSection::Settings, "[*] Settings"),
                ];

                for (section, label) in nav_items {
                    let selected = self.section == section;
                    
                    // Create a custom nav button with left accent border
                    ui.horizontal(|ui| {
                        // Left accent indicator for selected item
                        if selected {
                            ui.add_space(2.0);
                            let (rect, _) = ui.allocate_exact_size(
                                egui::vec2(3.0, 20.0),
                                egui::Sense::hover()
                            );
                            ui.painter().rect_filled(rect, 0.0, self.theme.primary);
                            ui.add_space(4.0);
                        } else {
                            ui.add_space(9.0);
                        }
                        
                        let text_color = if selected { self.theme.text_primary } else { self.theme.text_secondary };
                        let response = ui.add(
                            egui::Button::new(RichText::new(label).size(13.0).color(text_color))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .sense(egui::Sense::click())
                        );
                        
                        if response.clicked() {
                            self.previous_section = self.section;
                            self.section = section;
                            // Auto-refresh logs when entering Dashboard
                            if section == GuiSection::Dashboard
                                && (self.previous_section != GuiSection::Dashboard
                                    || self.log_view.content == "No logs yet. Run an operation to generate entries.")
                            {
                                self.refresh_logs();
                            }
                            // Always scroll to bottom when entering Dashboard
                            if section == GuiSection::Dashboard {
                                self.log_view.scroll_to_bottom = true;
                            }
                        }
                    });
                    ui.add_space(self.theme.spacing_xs);
                }

                // Add separator before Settings (last item - move it visually)
                ui.add_space(self.theme.spacing_lg);
                ui.horizontal(|ui| {
                    ui.add_space(self.theme.spacing_xs);
                    ui.label(RichText::new("-".repeat(22)).size(10.0).color(self.theme.surface_active));
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(self.theme.spacing_md);
            egui::ScrollArea::vertical().show(ui, |ui| {
                match self.section {
                    GuiSection::Dashboard => self.view_dashboard(ui),
                    GuiSection::CheckBalances => super::views::view_check_balances(self, ui),
                    GuiSection::SplitRandom => {
                        self.view_split(ui, SplitSelector::Random, SplitModeDescriptor::random())
                    }
                    GuiSection::SplitEqual => {
                        self.view_split(ui, SplitSelector::Equal, SplitModeDescriptor::equal())
                    }
                    GuiSection::BulkDisperse => self.view_bulk_disperse(ui),
                    GuiSection::Settings => self.view_settings(ui),
                }
            });
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(100));
    }
}

// configure_style is imported from super::theme

#[derive(Clone, Copy)]
pub(crate) enum SplitMode {
    Random,
    Equal,
}

#[derive(Clone, Copy)]
pub(crate) enum SplitSelector {
    Random,
    Equal,
}

pub(crate) struct SplitModeDescriptor {
    pub(crate) title: &'static str,
    pub(crate) button_color: egui::Color32,
    pub(crate) kind: SplitMode,
}

impl SplitModeDescriptor {
    /// Create a themed button for this split mode
    pub(crate) fn themed_button(&self, theme: &AppTheme) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(self.title)
                .color(theme.text_primary) // Explicit text color for readability
                .strong()
        )
            .fill(theme.surface) // Use dark surface background
            .stroke(egui::Stroke::new(3.0, self.button_color)) // Bright colored border
            .min_size(theme.button_medium)
    }
}

impl SplitModeDescriptor {
    pub(crate) fn random() -> Self {
        Self {
            title: "Split Funds – Random amounts",
            button_color: egui::Color32::from_rgb(0, 80, 40), // Much darker green for readability
            kind: SplitMode::Random,
        }
    }

    pub(crate) fn equal() -> Self {
        Self {
            title: "Split Funds – Equal amounts",
            button_color: egui::Color32::from_rgb(0, 80, 40), // Much darker green for consistency
            kind: SplitMode::Equal,
        }
    }
}

pub fn launch(mut config: Config) -> Result<()> {
    // Load user settings and apply them to the config
    let user_settings = crate::user_settings::UserSettings::load();

    // Use user settings for default network if available
    if let Some(network) = crate::config::find_network_by_chain_id(user_settings.selected_chain_id) {
        config = Config::from_network(network);
    }

    // Apply user settings gas speed
    config.gas_speed_multiplier = user_settings.default_gas_speed;

    let app_creator = move |cc: &eframe::CreationContext<'_>| {
        Box::new(GuiApp::new(config.clone(), &cc.egui_ctx)) as Box<dyn App>
    };

    // Build viewport with window icon
    // Use maximized as default for first launch, but persistence will restore previous state
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1100.0, 720.0]) // Default size for first launch or if restored from maximized
        .with_maximized(true); // Start maximized on first launch
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }

    let native_options = NativeOptions {
        viewport,
        // Enable window state persistence (size, position, maximized state)
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native("Beaug - Batch EVM Allocation Utility GUI", native_options, Box::new(app_creator))
        .map_err(|e| anyhow!("Failed to start GUI: {}", e))
}
