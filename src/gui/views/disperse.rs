//! Bulk disperse view implementation
//!
//! Contains the bulk disperse panel rendering including:
//! - Contract address validation
//! - Source address selection and balance display
//! - Recipient list management with parsing
//! - Amount calculation (auto or manual)
//! - Gas estimation and execution

use crate::bulk_disperse;
use crate::gui::app::GuiApp;
use crate::gui::helpers::{calculate_disperse_gas_limit, format_gwei, gas_speed_emoji, gas_speed_label, gas_speed_warning};
use crate::gui::notifications::NotificationEntry;
use crate::ledger_ops;
use crate::utils;
use eframe::egui::{self, RichText};
use ethers::prelude::Middleware;

impl GuiApp {
    /// Render the bulk disperse view
    pub(crate) fn view_bulk_disperse(&mut self, ui: &mut egui::Ui) {
        // Section header
        self.render_section_header(ui, "[$]", "BULK DISPERSE");
        ui.add_space(self.theme.spacing_md);

        // Fetch gas price if not available or job not running
        if self.bulk_disperse_state.current_gas_price.is_none() && self.bulk_disperse_state.gas_price_job.is_none() {
            let config = self.config.clone();
            self.bulk_disperse_state.gas_price_job = Some(self.spawn_job(move || async move {
                let provider = config.get_provider().await?;
                provider.get_gas_price().await.map_err(anyhow::Error::from)
            }));
        }

        // Disperse Contract Address with Validation
        self.render_disperse_contract_address(ui);
        
        ui.add_space(self.theme.spacing_sm);

        // Recipients Input - Large text box for pasting
        self.render_recipients_input(ui);

        // Show parsed preview if there's input
        self.render_recipients_preview(ui);
        
        ui.add_space(self.theme.spacing_md);

        // Source Address Selection with balance display
        self.render_source_address_selection(ui);
        
        ui.add_space(self.theme.spacing_sm);

        // Gas and amount configuration
        self.render_gas_and_amount_config(ui);

        // Calculation summary
        self.render_calculation_summary(ui);
        
        ui.add_space(self.theme.spacing_sm);

        // Amount input and execute button
        self.render_execute_section(ui);
    }

    fn render_disperse_contract_address(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Disperse Contract Address:");
            let address_changed = ui.text_edit_singleline(&mut self.bulk_disperse_state.disperse_contract_address).changed();
            if ui.button("Use Default").clicked() {
                self.bulk_disperse_state.disperse_contract_address = crate::disperse::MAIN_BEAUG_ADDRESS.to_string();
                self.bulk_disperse_state.last_validated_address = None;
                self.bulk_disperse_state.contract_validation = None;
            }
            
            if address_changed {
                self.bulk_disperse_state.last_validated_address = None;
                self.bulk_disperse_state.contract_validation = None;
                self.bulk_disperse_state.contract_validation_job = None;
            }
        });
        
        // Contract validation logic
        let current_address = self.bulk_disperse_state.disperse_contract_address.trim().to_string();
        let already_validated = self.bulk_disperse_state.last_validated_address.as_ref() == Some(&current_address);
        let job_running = self.bulk_disperse_state.contract_validation_job.is_some();
        
        if !already_validated && !job_running {
            if current_address.is_empty() {
                self.bulk_disperse_state.contract_validation = None;
                self.bulk_disperse_state.last_validated_address = Some(current_address.clone());
            } else if !current_address.starts_with("0x") {
                self.bulk_disperse_state.contract_validation = Some(
                    crate::disperse::ContractValidationStatus::Error("Address must start with 0x".to_string())
                );
                self.bulk_disperse_state.last_validated_address = Some(current_address.clone());
            } else if current_address.len() != 42 {
                self.bulk_disperse_state.contract_validation = Some(
                    crate::disperse::ContractValidationStatus::Error(
                        format!("Invalid address length ({}/42 chars)", current_address.len())
                    )
                );
                self.bulk_disperse_state.last_validated_address = Some(current_address.clone());
            } else if let Ok(contract_addr) = current_address.parse::<ethers::types::Address>() {
                let config = self.config.clone();
                let addr_clone = current_address.clone();
                self.bulk_disperse_state.contract_validation = Some(crate::disperse::ContractValidationStatus::Checking);
                self.bulk_disperse_state.contract_validation_job = Some(self.spawn_job(move || async move {
                    let provider = config.get_provider().await?;
                    Ok(crate::disperse::validate_contract(provider, config.chain_id, contract_addr).await)
                }));
                self.bulk_disperse_state.last_validated_address = Some(addr_clone);
            } else {
                self.bulk_disperse_state.contract_validation = Some(
                    crate::disperse::ContractValidationStatus::Error("Invalid address format".to_string())
                );
                self.bulk_disperse_state.last_validated_address = Some(current_address.clone());
            }
        }
        
        // Poll validation job
        if let Some(job) = &mut self.bulk_disperse_state.contract_validation_job {
            if let Some(result) = job.poll() {
                match result {
                    Ok(status) => {
                        self.bulk_disperse_state.contract_validation = Some(status);
                    }
                    Err(e) => {
                        self.bulk_disperse_state.contract_validation = Some(
                            crate::disperse::ContractValidationStatus::Error(e.to_string())
                        );
                    }
                }
                self.bulk_disperse_state.contract_validation_job = None;
            }
        }
        
        // Display validation status
        ui.horizontal(|ui| {
            ui.label(RichText::new("The smart contract address that will receive and disperse the funds.").italics().size(11.0));
            
            if let Some(ref status) = self.bulk_disperse_state.contract_validation {
                ui.add_space(8.0);
                let (icon, color, text) = match status {
                    crate::disperse::ContractValidationStatus::MainBeaugRegistry => {
                        ("★", egui::Color32::from_rgb(0, 220, 120), status.display_text())
                    }
                    crate::disperse::ContractValidationStatus::RegisteredAndCompatible => {
                        ("✓", egui::Color32::from_rgb(0, 200, 100), status.display_text())
                    }
                    crate::disperse::ContractValidationStatus::RegisteredButIncompatible => {
                        ("⚠", egui::Color32::from_rgb(255, 180, 0), status.display_text())
                    }
                    crate::disperse::ContractValidationStatus::CompatibleButUnregistered => {
                        ("ℹ", egui::Color32::from_rgb(100, 150, 255), status.display_text())
                    }
                    crate::disperse::ContractValidationStatus::Unknown => {
                        ("✗", egui::Color32::from_rgb(255, 80, 80), "Unknown contract - use with caution")
                    }
                    crate::disperse::ContractValidationStatus::Checking => {
                        ("⋯", egui::Color32::from_rgb(150, 150, 150), status.display_text())
                    }
                    crate::disperse::ContractValidationStatus::Error(_) => {
                        ("!", egui::Color32::from_rgb(255, 100, 100), status.display_text())
                    }
                };
                ui.label(RichText::new(format!("{} {}", icon, text)).color(color).size(11.0));
            }
        });
    }

    fn render_recipients_input(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Recipients and Amounts:").strong());
        ui.add_space(self.theme.spacing_xs);

        self.theme.frame_panel().show(ui, |ui| {
            ui.add({
                let native_token = self.config.native_token().to_lowercase();
                egui::TextEdit::multiline(&mut self.bulk_disperse_state.recipients_input)
                    .desired_rows(10)
                    .desired_width(f32::INFINITY)
                    .hint_text(format!("Paste your recipient list here...\n\n[#] EQUAL DISTRIBUTION (just addresses):\n0x742d35Cc6634C0532925a3b844Bc454e4438f44e\n0x742d35Cc6634C0532925a3b844Bc454e4438f44f\n0x742d35Cc6634C0532925a3b844Bc454e4438f44g\n\n[$] SPECIFIC AMOUNTS:\n0x742d35Cc6634C0532925a3b844Bc454e4438f44e,0.1\n0x742d35Cc6634C0532925a3b844Bc454e4438f44f,0.05\n0x742d35Cc6634C0532925a3b844Bc454e4438f44g,0.25\n\nOr use space-separated: address amount_in_{}", native_token))
            });
        });

        ui.add_space(self.theme.spacing_sm);
        ui.label(RichText::new(format!("Format: addresses only (equal split) OR address,amount_in_{} per line", self.config.native_token().to_lowercase())).italics().size(11.0));

        // Optional CSV upload
        ui.add_space(self.theme.spacing_sm);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Alternative:").small());
            if ui.button("📄 Load from CSV File").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("CSV files", &["csv"])
                    .pick_file()
                {
                    match self.load_csv_file(&path) {
                        Ok(recipient_data) => {
                            if !recipient_data.is_empty() {
                                self.bulk_disperse_state.recipients_input = recipient_data.clone();
                                let line_count = recipient_data.lines().count();
                                self.notifications.push_back(NotificationEntry::new(format!("[OK] Loaded {} recipients from CSV file", line_count)));
                            } else {
                                self.notifications.push_back(NotificationEntry::new("[!!] No valid recipients found in CSV file"));
                            }
                        }
                        Err(e) => {
                            self.notifications.push_back(NotificationEntry::new(format!("[XX] Failed to load CSV file: {}", e)));
                        }
                    }
                }
            }
        });
    }

    fn render_recipients_preview(&mut self, ui: &mut egui::Ui) {
        if self.bulk_disperse_state.recipients_input.trim().is_empty() {
            return;
        }

        ui.add_space(self.theme.spacing_md);
        ui.label(RichText::new("[#] Parsed Recipients Preview:").strong());

        match bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input) {
            Ok(disperse_type) => {
                match disperse_type {
                    bulk_disperse::BulkDisperseType::Equal(ref addresses) => {
                        if addresses.is_empty() {
                            ui.colored_label(egui::Color32::YELLOW, "No recipients found. Check your input format.");
                        } else {
                            ui.colored_label(
                                egui::Color32::GREEN,
                                format!("[OK] Equal distribution: {} recipients", addresses.len())
                            );
                            ui.label("[$] Amount will be split evenly among all recipients");
                        }
                    }
                    bulk_disperse::BulkDisperseType::Mixed(ref recipients) => {
                        if recipients.is_empty() {
                            ui.colored_label(egui::Color32::YELLOW, "No recipients found. Check your input format.");
                        } else {
                            let total_amount: ethers::types::U256 = recipients.iter().map(|(_, amount)| *amount).fold(ethers::types::U256::zero(), |acc, x| acc + x);
                            ui.colored_label(
                                egui::Color32::GREEN,
                                format!("[OK] Mixed distribution: {} recipients, total amount: {} {}", recipients.len(), utils::format_ether(total_amount), self.config.native_token())
                            );
                            ui.label("[$] Specified amounts will be distributed");
                        }
                    }
                }

                // Amount validation
                let native_token = self.config.native_token();
                let amount_str = self.bulk_disperse_state.amount_input.trim();
                if !amount_str.is_empty() {
                    match utils::parse_eth_str_to_wei(amount_str) {
                        Ok(amount_wei) => {
                            if amount_wei.is_zero() {
                                ui.colored_label(egui::Color32::RED, "[XX] Amount must be greater than 0");
                            } else {
                                match disperse_type {
                                    bulk_disperse::BulkDisperseType::Equal(_) => {
                                        ui.colored_label(
                                            egui::Color32::GREEN,
                                            format!("[OK] Amount to send: {} {} (will be split evenly)", amount_str, native_token)
                                        );
                                    }
                                    bulk_disperse::BulkDisperseType::Mixed(ref recipients) => {
                                        let total_specified: ethers::types::U256 = recipients.iter().map(|(_, amount)| *amount).fold(ethers::types::U256::zero(), |acc, x| acc + x);
                                        let display_tip_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.tip_amount)
                                            .unwrap_or(ethers::types::U256::zero());
                                        let total_required = total_specified + display_tip_wei;
                                        if amount_wei >= total_required {
                                            ui.colored_label(
                                                egui::Color32::GREEN,
                                                format!("[OK] Amount sufficient: {} {} >= {} {} required", amount_str, native_token, utils::format_ether(total_required), native_token)
                                            );
                                        } else {
                                            ui.colored_label(
                                                egui::Color32::RED,
                                                format!("[XX] Amount insufficient: {} {} < {} {} required", amount_str, native_token, utils::format_ether(total_required), native_token)
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        Err(_) => {
                            ui.colored_label(egui::Color32::RED, "[XX] Invalid amount format");
                        }
                    }
                } else {
                    ui.colored_label(egui::Color32::YELLOW, "[!!] Please enter amount to send");
                }

                // Show recipients preview
                let native_token = self.config.native_token();
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(20, 30, 45))
                    .rounding(4.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(120.0)
                            .show(ui, |ui| {
                                match disperse_type {
                                    bulk_disperse::BulkDisperseType::Equal(ref addresses) => {
                                        for (i, addr) in addresses.iter().enumerate() {
                                            if i >= 10 {
                                                ui.label(format!("... and {} more recipients", addresses.len() - 10));
                                                break;
                                            }
                                            ui.label(format!("{}. {:?} (equal share)", i + 1, addr));
                                        }
                                    }
                                    bulk_disperse::BulkDisperseType::Mixed(ref recipients) => {
                                        for (i, (addr, amount)) in recipients.iter().enumerate() {
                                            if i >= 10 {
                                                ui.label(format!("... and {} more recipients", recipients.len() - 10));
                                                break;
                                            }
                                            ui.label(format!("{}. {:?} → {} {}", i + 1, addr, utils::format_ether(*amount), native_token));
                                        }
                                    }
                                }
                            });
                    });
            }
            Err(e) => {
                ui.colored_label(egui::Color32::RED, format!("[XX] Parse error: {}", e));
            }
        }
    }

    fn render_source_address_selection(&mut self, ui: &mut egui::Ui) {
        let ledger_ready_for_fetch = self.ledger_status.is_usable();
        
        ui.horizontal(|ui| {
            ui.label("Source Address Index:");
            ui.add(
                egui::DragValue::new(&mut self.bulk_disperse_state.source_index)
                    .clamp_range(0..=1000)
            );
            
            let should_fetch = ledger_ready_for_fetch
                && self.bulk_disperse_state.last_fetched_source_index != Some(self.bulk_disperse_state.source_index)
                && self.bulk_disperse_state.source_balance_job.is_none();
            
            let refresh_hover = if ledger_ready_for_fetch {
                "Fetch source address and balance from Ledger"
            } else {
                "Connect and unlock your Ledger to fetch address"
            };
            let refresh_clicked = ui.add_enabled(
                ledger_ready_for_fetch, 
                egui::Button::new("🔄 Refresh")
            ).on_hover_text(refresh_hover).clicked();
            
            if should_fetch || refresh_clicked {
                let config = self.config.clone();
                let index = self.bulk_disperse_state.source_index;
                self.bulk_disperse_state.last_fetched_source_index = Some(index);
                self.bulk_disperse_state.source_balance_job = Some(self.spawn_job(move || async move {
                    let provider = config.get_provider().await?;
                    let address = ledger_ops::get_ledger_address_with_config(config.chain_id, index, Some(&config)).await?;
                    let balance = provider.get_balance(address, None).await?;
                    Ok((format!("{:?}", address), balance))
                }));
            }
        });
        
        // Display source address and balance
        if self.bulk_disperse_state.source_balance_job.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Fetching address and balance from Ledger...").italics().color(self.theme.text_secondary));
            });
        } else if let (Some(addr), Some(balance)) = (&self.bulk_disperse_state.source_address, self.bulk_disperse_state.source_balance) {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Address: {}", addr)).monospace().size(11.0));
            });
            let balance_eth = utils::format_ether(balance);
            let balance_color = if balance > ethers::types::U256::zero() { 
                self.theme.accent_green 
            } else { 
                self.theme.warning 
            };
            ui.horizontal(|ui| {
                ui.label("Available Balance:");
                ui.label(RichText::new(format!("{} {}", balance_eth, self.config.native_token())).strong().color(balance_color));
            });
        } else if !ledger_ready_for_fetch {
            self.render_ledger_warning(ui);
        } else {
            ui.label(RichText::new("Click Refresh to fetch source address and balance").italics().size(11.0).color(self.theme.text_secondary));
        }
    }

    fn render_gas_and_amount_config(&mut self, ui: &mut egui::Ui) {
        let old_gas_speed = self.bulk_disperse_state.gas_speed;
        let old_tip = self.bulk_disperse_state.tip_amount.clone();
        let old_keep_on_source = self.bulk_disperse_state.remaining_balance.clone();

        let is_mixed_mode = if !self.bulk_disperse_state.recipients_input.trim().is_empty() {
            matches!(
                bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input),
                Ok(bulk_disperse::BulkDisperseType::Mixed(_))
            )
        } else {
            false
        };

        // Gas Speed Selection
        ui.horizontal(|ui| {
            ui.label("Gas Speed:");
            let speed_label = gas_speed_label(self.bulk_disperse_state.gas_speed);
            let speed_emoji = gas_speed_emoji(self.bulk_disperse_state.gas_speed);
            ui.label(RichText::new(format!("{} {:.1}x ({})", speed_emoji, self.bulk_disperse_state.gas_speed, speed_label)).color(self.theme.accent_green));
        });
        
        ui.horizontal(|ui| {
            ui.label(RichText::new("Slow").small().color(self.theme.text_secondary));
            ui.add(egui::Slider::new(&mut self.bulk_disperse_state.gas_speed, 0.8..=2.5)
                .show_value(false)
                .step_by(0.1));
            ui.label(RichText::new("Aggressive").small().color(self.theme.text_secondary));
        });
        
        if let Some(warning) = gas_speed_warning(self.bulk_disperse_state.gas_speed) {
            ui.colored_label(self.theme.warning, warning);
        }
        ui.add_space(self.theme.spacing_xs);

        // Tip Amount Field
        let native_token = self.config.native_token();
        ui.horizontal(|ui| {
            ui.label(format!("Tip ({}):", native_token));
            ui.add(egui::TextEdit::singleline(&mut self.bulk_disperse_state.tip_amount)
                .desired_width(100.0)
                .hint_text("0.0"));
            ui.label(RichText::new("(added as recipient)").small().color(self.theme.text_secondary));
        });
        
        if !self.bulk_disperse_state.tip_amount.trim().is_empty() {
            if Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.tip_amount).is_none() {
                ui.colored_label(egui::Color32::YELLOW, "[!] Invalid format (use decimal numbers)");
            } else {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Tip Recipient:").color(self.theme.text_secondary));
                    ui.label(RichText::new(crate::disperse::BEAUG_OWNER_ADDRESS).monospace().color(self.theme.accent_green));
                });
            }
        }
        ui.add_space(self.theme.spacing_xs);

        // Keep on Source
        if !is_mixed_mode {
            ui.horizontal(|ui| {
                ui.label(format!("Keep on Source ({}):", self.config.native_token()));
                ui.add(
                    egui::TextEdit::singleline(&mut self.bulk_disperse_state.remaining_balance)
                        .desired_width(100.0)
                        .hint_text("0")
                );
                ui.label(RichText::new("(reserve on source address)").small().color(self.theme.text_secondary));
            });

            if !self.bulk_disperse_state.remaining_balance.trim().is_empty() {
                if Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.remaining_balance).is_none() {
                    ui.colored_label(egui::Color32::YELLOW, "[!] Invalid format (use decimal numbers)");
                }
            }
        } else {
            self.bulk_disperse_state.remaining_balance = "0".to_string();
        }
        ui.add_space(self.theme.spacing_sm);

        // Auto-recalculate
        let gas_speed_changed = (self.bulk_disperse_state.gas_speed - old_gas_speed).abs() > 0.01;
        let tip_changed = self.bulk_disperse_state.tip_amount != old_tip;
        let keep_on_source_changed = self.bulk_disperse_state.remaining_balance != old_keep_on_source;
        
        if (gas_speed_changed || tip_changed || keep_on_source_changed) 
            && self.bulk_disperse_state.source_balance.is_some()
            && self.bulk_disperse_state.current_gas_price.is_some()
            && !self.bulk_disperse_state.recipients_input.trim().is_empty()
        {
            self.auto_calculate_amount_silent();
        }
    }

    fn render_calculation_summary(&mut self, ui: &mut egui::Ui) {
        let (recipient_count, is_mixed_distribution, mixed_total) = if !self.bulk_disperse_state.recipients_input.trim().is_empty() {
            match bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input) {
                Ok(bulk_disperse::BulkDisperseType::Equal(addrs)) => (addrs.len(), false, ethers::types::U256::zero()),
                Ok(bulk_disperse::BulkDisperseType::Mixed(recips)) => {
                    let total: ethers::types::U256 = recips.iter().map(|(_, amt)| *amt).fold(ethers::types::U256::zero(), |acc, x| acc + x);
                    (recips.len(), true, total)
                },
                Err(_) => (0, false, ethers::types::U256::zero()),
            }
        } else {
            (0, false, ethers::types::U256::zero())
        };

        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("📊 Calculation Summary").strong());
            ui.add_space(4.0);
            
            if let (Some(gas_price), Some(source_balance)) = (
                self.bulk_disperse_state.current_gas_price,
                self.bulk_disperse_state.source_balance
            ) {
                let speed = self.bulk_disperse_state.gas_speed;
                
                let tip_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.tip_amount)
                    .unwrap_or(ethers::types::U256::zero());
                
                let total_recipients = if tip_wei.is_zero() { recipient_count } else { recipient_count + 1 };
                let gas_limit = if total_recipients > 0 { calculate_disperse_gas_limit(total_recipients) } else { 150_000u64 };
                let fee_wei = gas_price * ethers::types::U256::from(gas_limit);
                let adjusted_fee_wei = fee_wei * ethers::types::U256::from((speed * 100.0) as u64) / 100;
                let adjusted_gas_price = gas_price * ethers::types::U256::from((speed * 100.0) as u64) / 100;
                
                let keep_on_source_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.remaining_balance)
                    .unwrap_or(ethers::types::U256::zero());
                
                let reserved = adjusted_fee_wei + keep_on_source_wei;
                let available = if source_balance > reserved { source_balance - reserved } else { ethers::types::U256::zero() };
                
                let native_token = self.config.native_token();
                let speed_label = gas_speed_label(speed);
                
                egui::Grid::new("calc_summary_grid")
                    .num_columns(2)
                    .spacing([20.0, 4.0])
                    .show(ui, |ui| {
                        ui.label("Source Balance:");
                        ui.label(RichText::new(format!("{} {}", utils::format_ether(source_balance), native_token)).strong());
                        ui.end_row();
                        
                        ui.label("Gas Price:");
                        ui.label(RichText::new(format!("{} Gwei ({:.1}x {})", format_gwei(adjusted_gas_price), speed, speed_label)).color(self.theme.text_secondary));
                        ui.end_row();
                        
                        ui.label("Gas Limit:");
                        ui.label(RichText::new(format!("{} units", gas_limit)).color(self.theme.text_secondary));
                        ui.end_row();
                        
                        ui.label("Max Gas Fee:");
                        ui.label(format!("-{} {}", utils::format_ether(adjusted_fee_wei), native_token));
                        ui.end_row();
                        
                        if !is_mixed_distribution && keep_on_source_wei > ethers::types::U256::zero() {
                            ui.label("Keep on Source:");
                            ui.label(format!("-{} {}", utils::format_ether(keep_on_source_wei), native_token));
                            ui.end_row();
                        }
                        
                        ui.separator();
                        ui.separator();
                        ui.end_row();
                        
                        ui.label("Available to Send:");
                        ui.label(RichText::new(format!("{} {}", utils::format_ether(available), native_token)).strong().color(self.theme.accent_green));
                        ui.end_row();
                        
                        if tip_wei > ethers::types::U256::zero() {
                            ui.label("  └ Tip:");
                            ui.label(format!("-{} {}", utils::format_ether(tip_wei), native_token));
                            ui.end_row();
                            
                            let to_recipients = if available > tip_wei { available - tip_wei } else { ethers::types::U256::zero() };
                            ui.label("  └ To Recipients:");
                            ui.label(RichText::new(format!("{} {}", utils::format_ether(to_recipients), native_token)).color(self.theme.accent_green));
                            ui.end_row();
                            
                            if recipient_count > 0 {
                                if is_mixed_distribution {
                                    ui.label(format!("  └ Distribution ({}):", recipient_count));
                                    ui.label(format!("Mixed (specified amounts: {} {})", utils::format_ether(mixed_total), native_token));
                                } else {
                                    let per_recipient = to_recipients / ethers::types::U256::from(recipient_count);
                                    ui.label(format!("  └ Per Recipient ({}):", recipient_count));
                                    ui.label(format!("~{} {} each", utils::format_ether(per_recipient), native_token));
                                }
                                ui.end_row();
                            }
                        } else if recipient_count > 0 {
                            if is_mixed_distribution {
                                ui.label(format!("Distribution ({}):", recipient_count));
                                ui.label(format!("Mixed (specified amounts: {} {})", utils::format_ether(mixed_total), native_token));
                            } else {
                                let per_recipient = available / ethers::types::U256::from(recipient_count);
                                ui.label(format!("Per Recipient ({}):", recipient_count));
                                ui.label(format!("~{} {} each", utils::format_ether(per_recipient), native_token));
                            }
                            ui.end_row();
                        }
                    });
            } else {
                ui.label(RichText::new("Fetch source balance to see calculations").italics().color(self.theme.text_secondary));
            }
        });
    }

    fn render_execute_section(&mut self, ui: &mut egui::Ui) {
        // Amount to Send
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Amount to Send ({}):", self.config.native_token())).strong());
            ui.add(
                egui::TextEdit::singleline(&mut self.bulk_disperse_state.amount_input)
                    .desired_width(150.0)
            );
            if ui.button("↻ Recalculate").clicked() {
                self.auto_calculate_amount();
            }
        });
        ui.label(RichText::new("Auto-calculated from balance. Edit to override.").italics().size(11.0).color(self.theme.text_secondary));
        ui.add_space(self.theme.spacing_sm);

        // Validation
        let (can_proceed, validation_errors) = self.validate_disperse_parameters();

        // Display validation status
        if !validation_errors.is_empty() {
            ui.add_space(self.theme.spacing_xs);
            for error in &validation_errors {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠").color(self.theme.warning));
                    ui.label(RichText::new(error).color(self.theme.warning));
                });
            }
        } else if can_proceed {
            self.render_ready_status(ui);
        }
        ui.add_space(self.theme.spacing_md);

        let validation_message = if !validation_errors.is_empty() {
            validation_errors.join("; ")
        } else {
            "Ready to disperse".to_string()
        };

        if ui.add_enabled(
            can_proceed,
            self.theme.button_warning("Initiate Bulk Disperse")
        ).on_hover_text(&validation_message).clicked() {
            self.execute_bulk_disperse();
        }
        ui.add_space(self.theme.spacing_sm);

        // Status Display
        if let Some(status) = &self.bulk_disperse_state.status {
            ui.label(status);
        }
    }

    fn validate_disperse_parameters(&self) -> (bool, Vec<String>) {
        let mut can_proceed = true;
        let mut validation_errors: Vec<String> = Vec::new();

        // Check ledger status
        if self.ledger_status.has_problem() {
            can_proceed = false;
            if let Some(warning) = self.get_ledger_warning_message() {
                validation_errors.push(warning);
            }
        }

        // Check basic requirements
        if self.bulk_disperse_state.recipients_input.trim().is_empty() {
            can_proceed = false;
            validation_errors.push("No recipients entered".to_string());
        }
        
        if self.bulk_disperse_state.disperse_contract_address.trim().is_empty() {
            can_proceed = false;
            validation_errors.push("No contract address".to_string());
        }
        
        if self.bulk_disperse_state.amount_input.trim().is_empty() {
            can_proceed = false;
            validation_errors.push("No amount specified".to_string());
        }

        // Parse values
        let tip_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.tip_amount)
            .unwrap_or(ethers::types::U256::zero());
        
        let keep_on_source_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.remaining_balance)
            .unwrap_or(ethers::types::U256::zero());
        
        let amount_wei = utils::parse_eth_str_to_wei(self.bulk_disperse_state.amount_input.trim())
            .unwrap_or(ethers::types::U256::zero());

        let (recipient_count, is_mixed_distribution) = if !self.bulk_disperse_state.recipients_input.trim().is_empty() {
            match bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input) {
                Ok(bulk_disperse::BulkDisperseType::Equal(addrs)) => (addrs.len(), false),
                Ok(bulk_disperse::BulkDisperseType::Mixed(recips)) => (recips.len(), true),
                Err(_) => (0, false),
            }
        } else {
            (0, false)
        };

        if let Some(source_balance) = self.bulk_disperse_state.source_balance {
            if let Some(gas_price) = self.bulk_disperse_state.current_gas_price {
                let speed = self.bulk_disperse_state.gas_speed;
                let total_recipients = if tip_wei.is_zero() { recipient_count } else { recipient_count + 1 };
                let gas_limit = if total_recipients > 0 { calculate_disperse_gas_limit(total_recipients) } else { 150_000u64 };
                let fee_wei = gas_price * ethers::types::U256::from(gas_limit);
                let adjusted_fee_wei = fee_wei * ethers::types::U256::from((speed * 100.0) as u64) / 100;
                
                let total_needed = amount_wei + adjusted_fee_wei + keep_on_source_wei;
                let tolerance = ethers::types::U256::from(1_000_000_000u64);
                let native_token = self.config.native_token();
                
                if total_needed > source_balance + tolerance {
                    can_proceed = false;
                    let shortfall = total_needed.saturating_sub(source_balance);
                    validation_errors.push(format!(
                        "Insufficient balance: need {} {} more",
                        utils::format_ether(shortfall), native_token
                    ));
                }
                
                if !tip_wei.is_zero() && amount_wei < tip_wei {
                    can_proceed = false;
                    validation_errors.push(format!(
                        "Amount ({} {}) < Tip ({} {})",
                        utils::format_ether(amount_wei), native_token,
                        utils::format_ether(tip_wei), native_token
                    ));
                }
                
                if !is_mixed_distribution && amount_wei > ethers::types::U256::zero() && recipient_count > 0 {
                    let to_recipients = if amount_wei > tip_wei { amount_wei - tip_wei } else { ethers::types::U256::zero() };
                    let per_recipient = to_recipients / ethers::types::U256::from(recipient_count);
                    
                    if per_recipient.is_zero() {
                        can_proceed = false;
                        validation_errors.push("Amount per recipient would be 0".to_string());
                    }
                }
            } else {
                can_proceed = false;
                validation_errors.push("Waiting for gas price...".to_string());
            }
        } else {
            can_proceed = false;
            validation_errors.push("Fetch source balance first".to_string());
        }

        // Check for mixed distribution minimum amounts
        if can_proceed && !self.bulk_disperse_state.recipients_input.trim().is_empty() {
            if let Ok(disperse_type) = bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input) {
                if let bulk_disperse::BulkDisperseType::Mixed(ref recipients) = disperse_type {
                    let total_specified: ethers::types::U256 = recipients.iter()
                        .map(|(_, amount)| *amount)
                        .fold(ethers::types::U256::zero(), |acc, x| acc + x);
                    
                    let min_needed = total_specified + tip_wei;
                    if amount_wei < min_needed {
                        can_proceed = false;
                        validation_errors.push(format!(
                            "Need {} {} for specified amounts + tip",
                            utils::format_ether(min_needed), self.config.native_token()
                        ));
                    }
                }
            }
        }

        (can_proceed, validation_errors)
    }

    fn render_ready_status(&self, ui: &mut egui::Ui) {
        let amount_wei = utils::parse_eth_str_to_wei(self.bulk_disperse_state.amount_input.trim())
            .unwrap_or(ethers::types::U256::zero());
        let tip_wei = Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.tip_amount)
            .unwrap_or(ethers::types::U256::zero());

        if let Ok(disperse_type) = bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input) {
            let native_token = self.config.native_token();
            let to_recipients = if amount_wei > tip_wei { amount_wei - tip_wei } else { ethers::types::U256::zero() };

            ui.horizontal(|ui| {
                ui.label(RichText::new("✓").color(self.theme.accent_green));
                match disperse_type {
                    bulk_disperse::BulkDisperseType::Mixed(ref recipients) => {
                        let mixed_total: ethers::types::U256 = recipients.iter().map(|(_, amt)| *amt).fold(ethers::types::U256::zero(), |acc, x| acc + x);
                        ui.label(RichText::new(format!(
                            "Ready: {} {} to {} recipients (mixed amounts)",
                            utils::format_ether(mixed_total), native_token,
                            recipients.len()
                        )).color(self.theme.accent_green));
                    }
                    bulk_disperse::BulkDisperseType::Equal(ref addresses) => {
                        let per_recipient = to_recipients / ethers::types::U256::from(addresses.len());
                        ui.label(RichText::new(format!(
                            "Ready: {} {} to {} recipients (~{} each)",
                            utils::format_ether(to_recipients), native_token,
                            addresses.len(),
                            utils::format_ether(per_recipient)
                        )).color(self.theme.accent_green));
                    }
                }
            });
        }
    }

    fn execute_bulk_disperse(&mut self) {
        match bulk_disperse::parse_bulk_disperse_input(&self.bulk_disperse_state.recipients_input) {
            Ok(disperse_type) => {
                let is_empty = match &disperse_type {
                    bulk_disperse::BulkDisperseType::Equal(addresses) => addresses.is_empty(),
                    bulk_disperse::BulkDisperseType::Mixed(recipients) => recipients.is_empty(),
                };

                if is_empty {
                    self.notifications.push_back(NotificationEntry::new("[XX] No recipients found in input"));
                    return;
                }

                let config = self.config.clone();
                let disperse_contract = if self.bulk_disperse_state.disperse_contract_address.trim().is_empty() {
                    None
                } else {
                    Some(self.bulk_disperse_state.disperse_contract_address.trim().to_string())
                };
                let source_index = self.bulk_disperse_state.source_index as usize;

                let amount_to_send = match utils::parse_eth_str_to_wei(self.bulk_disperse_state.amount_input.trim()) {
                    Ok(wei) => {
                        if wei.is_zero() {
                            self.notifications.push_back(NotificationEntry::new("[XX] Amount must be greater than 0"));
                            return;
                        }
                        wei
                    }
                    Err(e) => {
                        self.notifications.push_back(NotificationEntry::new(format!("[XX] Invalid amount: {}", e)));
                        return;
                    }
                };
                
                let tip_amount = if !self.bulk_disperse_state.tip_amount.trim().is_empty() {
                    Self::parse_optional_eth_to_wei(&self.bulk_disperse_state.tip_amount)
                } else {
                    None
                };
                
                let tip_recipient: Option<ethers::types::Address> = if tip_amount.is_some() {
                    crate::disperse::BEAUG_OWNER_ADDRESS.parse().ok()
                } else {
                    None
                };
                
                let gas_speed = self.bulk_disperse_state.gas_speed;
                let use_native_ledger = self.user_settings.use_native_ledger;

                self.bulk_disperse_state.status = Some("Preparing transaction...".to_string());

                let job = self.spawn_job(move || async move {
                    bulk_disperse::bulk_disperse(
                        config,
                        disperse_type,
                        amount_to_send,
                        disperse_contract,
                        Some(source_index),
                        tip_amount,
                        tip_recipient,
                        gas_speed,
                        use_native_ledger,
                    ).await
                });

                self.bulk_disperse_state.job = Some(job);
                self.notifications.push_back(NotificationEntry::new("Bulk disperse initiated..."));
            }
            Err(e) => {
                self.notifications.push_back(NotificationEntry::new(format!("[XX] Failed to parse recipients: {}", e)));
            }
        }
    }
}
