//! Settings view implementation
//!
//! Contains the settings panel rendering including:
//! - Network & RPC configuration
//! - Default gas speed settings  
//! - Scan & operation defaults
//! - Hardware wallet settings
//! - Derivation path configuration
//! - Custom network management

use crate::gui::app::GuiApp;
use crate::gui::helpers::{gas_speed_emoji, gas_speed_label, gas_speed_warning};
use crate::gui::notifications::NotificationEntry;
use crate::user_settings::CustomNetwork;
use eframe::egui::{self, RichText};

impl GuiApp {
    /// Render the settings view
    pub(crate) fn view_settings(&mut self, ui: &mut egui::Ui) {
        // Section header
        self.render_section_header(ui, "[*]", "SETTINGS");
        ui.add_space(self.theme.spacing_md);

        // Network & RPC Configuration Panel
        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("Network & RPC Configuration").size(18.0).strong().color(self.theme.text_primary));
            ui.add_space(self.theme.spacing_sm);

            // --- Default Network Section ---
            ui.group(|ui| {
                ui.label(RichText::new("[~] Default Network").strong().color(self.theme.accent_blue));
                ui.add_space(self.theme.spacing_xs);

                ui.label("Select the default network for new sessions:");
                ui.add_space(self.theme.spacing_xs);

                // Network selection combo box - using persistent state field
                let current_selection_text = crate::config::find_network_by_chain_id(self.settings_pending_chain_id)
                    .map(|n| n.label.to_string())
                    .or_else(|| {
                        self.user_settings.get_custom_network(self.settings_pending_chain_id)
                            .map(|n| n.label.clone())
                    })
                    .unwrap_or_else(|| "Unknown".to_string());

                egui::ComboBox::from_label("")
                    .selected_text(&current_selection_text)
                    .show_ui(ui, |ui| {
                        // Built-in networks
                        for network in crate::config::NETWORKS.iter() {
                            let is_selected = network.chain_id == self.settings_pending_chain_id;
                            if ui.selectable_label(is_selected, network.label).clicked() {
                                self.settings_pending_chain_id = network.chain_id;
                            }
                        }
                        // Custom networks
                        if !self.user_settings.custom_networks.is_empty() {
                            ui.separator();
                            ui.label(RichText::new("── Custom ──").small().color(egui::Color32::from_rgb(180, 140, 200)));
                            for network in &self.user_settings.custom_networks {
                                let is_selected = network.chain_id == self.settings_pending_chain_id;
                                if ui.selectable_label(is_selected, &network.label).clicked() {
                                    self.settings_pending_chain_id = network.chain_id;
                                }
                            }
                        }
                    });

                // Show save button if selection differs from saved value
                if self.settings_pending_chain_id != self.user_settings.selected_chain_id {
                    ui.add_space(self.theme.spacing_xs);
                    ui.horizontal(|ui| {
                        if ui.add(self.theme.button_primary("Save Default Network")).clicked() {
                            self.user_settings.selected_chain_id = self.settings_pending_chain_id;
                            if let Err(e) = self.user_settings.save() {
                                self.notifications.push_back(NotificationEntry::new(format!("Failed to save settings: {}", e)));
                            } else {
                                self.notifications.push_back(NotificationEntry::new("Default network updated successfully."));
                            }
                        }
                        ui.label(RichText::new("(unsaved changes)").small().color(self.theme.warning));
                    });
                }

                ui.add_space(self.theme.spacing_xs);
                ui.label(RichText::new("This setting will take effect on the next application restart.").small().color(self.theme.text_secondary));
            });

            ui.add_space(self.theme.spacing_md);

            // --- Custom RPC Section ---
            ui.group(|ui| {
                ui.label(RichText::new("[@] Custom RPC Override").strong().color(self.theme.accent_blue));
                ui.add_space(self.theme.spacing_xs);

                let mut rpc_changed = false;
                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut self.use_custom_rpc, "Use custom RPC URL")
                        .changed()
                    {
                        rpc_changed = true;
                    }
                });
                if self.use_custom_rpc {
                    ui.add_space(self.theme.spacing_xs);
                    egui::Grid::new("rpc_grid")
                        .num_columns(2)
                        .spacing([self.theme.spacing_sm, self.theme.spacing_xs])
                        .show(ui, |ui| {
                            ui.label("RPC URL:");
                            ui.horizontal(|ui| {
                                let response = ui.add(
                                    egui::TextEdit::singleline(&mut self.custom_rpc)
                                        .desired_width(400.0)
                                );
                                if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    rpc_changed = true;
                                }
                                if ui.add(self.theme.button_small("Apply")).clicked() {
                                    rpc_changed = true;
                                }
                            });
                            ui.end_row();
                        });
                }
                if rpc_changed {
                    self.apply_network_selection();
                    self.notifications.push_back(NotificationEntry::new("RPC configuration updated."));
                }

                ui.add_space(self.theme.spacing_xs);
                ui.label(RichText::new("Override the default RPC endpoint for the current session.").small().color(self.theme.text_secondary));
            });
        });

        ui.add_space(self.theme.spacing_lg);

        // Default Gas Speed Panel
        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("Default Gas Speed").size(18.0).strong().color(self.theme.text_primary));
            ui.add_space(self.theme.spacing_sm);

            ui.label("Adjust transaction priority by multiplying the network's suggested gas price:");
            ui.add_space(self.theme.spacing_xs);

            // Gas speed slider with labels
            ui.horizontal(|ui| {
                ui.label("Gas speed:");
                
                // Show current speed with emoji and label
                let speed_label = gas_speed_label(self.settings_pending_gas_speed);
                let speed_emoji = gas_speed_emoji(self.settings_pending_gas_speed);
                ui.label(RichText::new(format!("{} {:.1}x ({})", speed_emoji, self.settings_pending_gas_speed, speed_label)).color(self.theme.accent_green));
            });
            
            ui.add_space(self.theme.spacing_xs);
            
            // Slider with snap points
            ui.horizontal(|ui| {
                ui.label(RichText::new("Slow").small().color(self.theme.text_secondary));
                if ui.add(egui::Slider::new(&mut self.settings_pending_gas_speed, 0.8..=2.5)
                    .show_value(true)
                    .suffix("x")
                    .step_by(0.1)).changed() {
                    // Value updated automatically
                }
                ui.label(RichText::new("Aggressive").small().color(self.theme.text_secondary));
            });

            // Show warning for extreme values
            if let Some(warning) = gas_speed_warning(self.settings_pending_gas_speed) {
                ui.colored_label(self.theme.warning, warning);
            }

            // Show save button if value differs from saved value
            if (self.settings_pending_gas_speed - self.user_settings.default_gas_speed).abs() > 0.01 {
                ui.add_space(self.theme.spacing_xs);
                ui.horizontal(|ui| {
                    if ui.add(self.theme.button_primary("Save Gas Speed")).clicked() {
                        self.user_settings.default_gas_speed = self.settings_pending_gas_speed;
                        if let Err(e) = self.user_settings.save() {
                            self.notifications.push_back(NotificationEntry::new(format!("Failed to save settings: {}", e)));
                        } else {
                            self.notifications.push_back(NotificationEntry::new("Default gas speed updated successfully."));
                        }
                    }
                    ui.label(RichText::new("(unsaved changes)").small().color(self.theme.warning));
                });
            }

            ui.add_space(self.theme.spacing_xs);
            ui.label(RichText::new("1.0x = Network base fee • Higher = Faster confirmation, higher cost • Lower = Slower, may get stuck").small().color(self.theme.text_secondary));
            
            // Show network-specific gas guidance
            let chain_desc = crate::config::chain_gas_description(self.config.chain_id);
            ui.add_space(self.theme.spacing_xs);
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("💡 {}: ", self.config.network_label())).small().color(self.theme.info));
                ui.label(RichText::new(chain_desc).small().italics().color(self.theme.text_secondary));
            });
            
            // Show if chain uses EIP-1559
            let uses_eip1559 = crate::config::chain_supports_eip1559(self.config.chain_id);
            ui.label(RichText::new(format!("Transaction type: {}", if uses_eip1559 { "EIP-1559 (dynamic fees)" } else { "Legacy (fixed gas price)" })).small().color(self.theme.text_secondary));
        });

        ui.add_space(self.theme.spacing_lg);

        // Scan & Operation Defaults Panel
        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("Scan & Operation Defaults").size(18.0).strong().color(self.theme.text_primary));
            ui.add_space(self.theme.spacing_sm);

            ui.label("Configure default values used for scanning addresses and split operations:");
            ui.add_space(self.theme.spacing_xs);

            egui::Grid::new("scan_defaults_grid")
                .num_columns(2)
                .spacing([self.theme.spacing_md, self.theme.spacing_xs])
                .show(ui, |ui| {
                    // Scan start index
                    ui.label("Default scan start index:");
                    ui.horizontal(|ui| {
                        let mut start_index = self.settings_pending_scan_start_index as i32;
                        if ui.add(egui::DragValue::new(&mut start_index)
                            .speed(1)
                            .clamp_range(0..=100_000)
                            .suffix(" addresses")).changed() {
                            self.settings_pending_scan_start_index = start_index as u32;
                        }
                        ui.label(RichText::new("(First address index to start scanning from)").small().color(self.theme.text_secondary));
                    });
                    ui.end_row();

                    // Number of outputs for split operations
                    ui.label("Default split outputs:");
                    ui.horizontal(|ui| {
                        let mut outputs = self.settings_pending_split_outputs as i32;
                        if ui.add(egui::DragValue::new(&mut outputs)
                            .speed(1)
                            .clamp_range(1..=50)
                            .suffix(" outputs")).changed() {
                            self.settings_pending_split_outputs = outputs as u32;
                        }
                        ui.label(RichText::new("(Number of outputs for split operations)").small().color(self.theme.text_secondary));
                    });
                    ui.end_row();

                    // Default remaining balance for split operations
                    ui.label("Default remaining balance:");
                    ui.horizontal(|ui| {
                        let mut remaining = self.settings_pending_remaining_balance as f64 / 1_000_000_000_000_000_000.0;
                        let token_suffix = format!(" {}", self.config.native_token());
                        if ui.add(egui::DragValue::new(&mut remaining)
                            .speed(0.01)
                            .clamp_range(0.0..=1000.0)
                            .suffix(token_suffix)).changed() {
                            self.settings_pending_remaining_balance = (remaining * 1_000_000_000_000_000_000.0) as u64;
                        }
                        ui.label(RichText::new("(Amount to keep on source address)").small().color(self.theme.text_secondary));
                    });
                    ui.end_row();

                    // Consecutive empty addresses to stop scanning
                    ui.label("Default consecutive empties:");
                    ui.horizontal(|ui| {
                        let mut empties = self.settings_pending_scan_empty_streak as i32;
                        if ui.add(egui::DragValue::new(&mut empties)
                            .speed(1)
                            .clamp_range(1..=100)
                            .suffix(" addresses")).changed() {
                            self.settings_pending_scan_empty_streak = empties as u32;
                        }
                        ui.label(RichText::new("(Stop scanning after this many consecutive empty addresses)").small().color(self.theme.text_secondary));
                    });
                    ui.end_row();
                });

            // Show save button if any values differ from saved values
            let has_changes = self.settings_pending_scan_start_index != self.user_settings.default_scan_start_index
                || self.settings_pending_split_outputs != self.user_settings.default_split_outputs
                || self.settings_pending_scan_empty_streak != self.user_settings.default_scan_empty_streak
                || self.settings_pending_remaining_balance != self.user_settings.default_remaining_balance;

            if has_changes {
                ui.add_space(self.theme.spacing_xs);
                ui.horizontal(|ui| {
                    if ui.add(self.theme.button_primary("Save Scan Defaults")).clicked() {
                        self.user_settings.default_scan_start_index = self.settings_pending_scan_start_index;
                        self.user_settings.default_split_outputs = self.settings_pending_split_outputs;
                        self.user_settings.default_scan_empty_streak = self.settings_pending_scan_empty_streak;
                        self.user_settings.default_remaining_balance = self.settings_pending_remaining_balance;
                        if let Err(e) = self.user_settings.save() {
                            self.notifications.push_back(NotificationEntry::new(format!("Failed to save settings: {}", e)));
                        } else {
                            // Update all state objects with the new settings
                            self.check_state.update_from_settings(&self.user_settings);
                            self.split_random.update_from_settings(&self.user_settings);
                            self.split_equal.update_from_settings(&self.user_settings);

                            self.notifications.push_back(NotificationEntry::new("Scan defaults updated successfully."));
                        }
                    }
                    ui.label(RichText::new("(unsaved changes)").small().color(self.theme.warning));
                });
            }

            ui.add_space(self.theme.spacing_xs);
            ui.label(RichText::new("These settings will take effect immediately for new operations.").small().color(self.theme.text_secondary));
        });

        ui.add_space(self.theme.spacing_lg);

        // Hardware Wallet Settings Panel
        self.render_hardware_wallet_settings(ui);

        ui.add_space(self.theme.spacing_lg);

        // Derivation Path Configuration Panel
        self.render_derivation_path_settings(ui);

        ui.add_space(self.theme.spacing_lg);

        // Custom Networks Panel
        self.render_custom_networks_settings(ui);
    }

    fn render_hardware_wallet_settings(&mut self, ui: &mut egui::Ui) {
        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("Hardware Wallet Settings").size(18.0).strong().color(self.theme.text_primary));
            ui.add_space(self.theme.spacing_sm);

            // Ledger Connection Method Selection
            ui.group(|ui| {
                ui.label(RichText::new("Ledger Connection Method").strong().color(self.theme.accent_blue));
                ui.add_space(self.theme.spacing_xs);
                
                ui.label("Choose how Beaug communicates with your Ledger device:");
                ui.add_space(self.theme.spacing_xs);
                
                let mut use_native = self.user_settings.use_native_ledger;
                
                // Native ethers-rs option (default/recommended)
                ui.horizontal(|ui| {
                    if ui.radio_value(&mut use_native, true, "").clicked() {
                        self.user_settings.use_native_ledger = true;
                        if let Err(e) = self.user_settings.save() {
                            self.notifications.push_back(NotificationEntry::new(format!("Failed to save: {}", e)));
                        } else {
                            self.notifications.push_back(NotificationEntry::new("Switched to Native for Ledger operations."));
                        }
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Native (Default)").strong());
                        ui.label(RichText::new("Built-in HID support. No external software required.").small().color(self.theme.text_secondary));
                    });
                });
                
                ui.add_space(self.theme.spacing_xs);
                
                // Foundry Cast option (backup)
                ui.horizontal(|ui| {
                    if ui.radio_value(&mut use_native, false, "").clicked() {
                        self.user_settings.use_native_ledger = false;
                        if let Err(e) = self.user_settings.save() {
                            self.notifications.push_back(NotificationEntry::new(format!("Failed to save: {}", e)));
                        } else {
                            self.notifications.push_back(NotificationEntry::new("Switched to Foundry Cast for Ledger operations."));
                        }
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Foundry Cast").strong());
                        ui.label(RichText::new("Uses external 'cast' CLI tool. Alternative if native mode has issues.").small().color(self.theme.text_secondary));
                    });
                });
                
                ui.add_space(self.theme.spacing_xs);
                
                // Show current status
                let current_method = if self.user_settings.use_native_ledger { "Native" } else { "Foundry Cast" };
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Current method:").small().color(self.theme.text_secondary));
                    ui.label(RichText::new(current_method).small().strong().color(self.theme.accent_green));
                });
                
                // Info/warning based on mode
                if !self.user_settings.use_native_ledger {
                    ui.add_space(self.theme.spacing_xs);
                    ui.colored_label(self.theme.info, "ℹ Foundry Cast mode requires 'cast' to be installed.");
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Install Foundry:").small().color(self.theme.text_secondary));
                        if ui.link(RichText::new("https://getfoundry.sh").small()).clicked() {
                            let _ = open::that("https://getfoundry.sh");
                        }
                    });
                }
            });

            ui.add_space(self.theme.spacing_md);

            ui.label("Configure Ledger connection monitoring:");
            ui.add_space(self.theme.spacing_xs);

            egui::Grid::new("ledger_settings_grid")
                .num_columns(2)
                .spacing([self.theme.spacing_md, self.theme.spacing_xs])
                .show(ui, |ui| {
                    ui.label("Status check interval:");
                    ui.horizontal(|ui| {
                        let mut interval = self.user_settings.ledger_refresh_interval_secs as i32;
                        if ui.add(egui::DragValue::new(&mut interval)
                            .speed(1)
                            .clamp_range(0..=60)
                            .suffix(" sec")).changed() {
                            self.user_settings.ledger_refresh_interval_secs = interval as u64;
                            if let Err(e) = self.user_settings.save() {
                                self.notifications.push_back(NotificationEntry::new(format!("Failed to save: {}", e)));
                            }
                        }
                        
                        // Quick presets
                        if ui.small_button("Off").on_hover_text("Disable auto-check").clicked() {
                            self.user_settings.ledger_refresh_interval_secs = 0;
                            let _ = self.user_settings.save();
                        }
                        if ui.small_button("5s").clicked() {
                            self.user_settings.ledger_refresh_interval_secs = 5;
                            let _ = self.user_settings.save();
                        }
                        if ui.small_button("15s").clicked() {
                            self.user_settings.ledger_refresh_interval_secs = 15;
                            let _ = self.user_settings.save();
                        }
                        if ui.small_button("30s").clicked() {
                            self.user_settings.ledger_refresh_interval_secs = 30;
                            let _ = self.user_settings.save();
                        }
                    });
                    ui.end_row();
                });
            
            ui.add_space(self.theme.spacing_xs);
            let interval_text = if self.user_settings.ledger_refresh_interval_secs == 0 {
                "Auto-check disabled. Use the [R] button in the top bar to manually check status.".to_string()
            } else {
                format!("Ledger connection will be checked every {} seconds automatically.", self.user_settings.ledger_refresh_interval_secs)
            };
            ui.label(RichText::new(interval_text).small().color(self.theme.text_secondary));
        });
    }

    fn render_derivation_path_settings(&mut self, ui: &mut egui::Ui) {
        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("Derivation Path Configuration").size(18.0).strong().color(self.theme.text_primary));
            ui.add_space(self.theme.spacing_sm);

            // Mode selection
            ui.group(|ui| {
                ui.label(RichText::new("Derivation Mode").strong());
                ui.add_space(self.theme.spacing_xs);

                // Account-index mode (default - used by Ledger Live, MetaMask)
                ui.horizontal(|ui| {
                    if ui.radio_value(
                        &mut self.config_derivation_mode,
                        crate::config::DerivationMode::AccountIndex,
                        ""
                    ).clicked() {
                        // Mode changed
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Account-Index Mode").strong());
                        ui.label(RichText::new("Standard mode used by Ledger Live and MetaMask").small().color(self.theme.success));
                        ui.monospace("m/44'/60'/[i]'/0/n — account varies, addres fixed");

                        if self.config_derivation_mode == crate::config::DerivationMode::AccountIndex {
                            ui.add_space(self.theme.spacing_xs);
                            ui.horizontal(|ui| {
                                ui.label("Constant address number:");
                                ui.add(egui::TextEdit::singleline(&mut self.config_custom_address_index)
                                    .desired_width(60.0));
                                if ui.add(self.theme.button_small("Reset")).clicked() {
                                    self.config_custom_address_index = "0".to_string();
                                }
                            });
                        }
                    });
                });

                ui.add_space(self.theme.spacing_sm);

                // Address-index mode
                ui.horizontal(|ui| {
                    if ui.radio_value(
                        &mut self.config_derivation_mode,
                        crate::config::DerivationMode::AddressIndex,
                        ""
                    ).clicked() {
                        // Mode changed
                    }
                    ui.vertical(|ui| {
                        ui.label(RichText::new("Address-Index Mode").strong());
                        ui.label(RichText::new("Alternative mode with fixed account").small().color(self.theme.text_secondary));
                        ui.monospace("m/44'/60'/n'/0/[i] — account fixed, address varies");

                        if self.config_derivation_mode == crate::config::DerivationMode::AddressIndex {
                            ui.add_space(self.theme.spacing_xs);
                            ui.horizontal(|ui| {
                                ui.label("Constant account number:");
                                ui.add(egui::TextEdit::singleline(&mut self.config_custom_account)
                                    .desired_width(60.0));
                                if ui.add(self.theme.button_small("Reset")).clicked() {
                                    self.config_custom_account = "0".to_string();
                                }
                            });
                        }
                    });
                });
            });

            ui.add_space(self.theme.spacing_md);

            // Coin Type Configuration (Advanced)
            ui.group(|ui| {
                ui.label(RichText::new("Coin Type (Advanced)").strong().color(self.theme.accent_green));
                ui.add_space(self.theme.spacing_xs);
                
                ui.label(
                    RichText::new("⚠️ Warning: Changing the coin type will generate different addresses!")
                        .small()
                        .color(self.theme.warning)
                );
                ui.label(
                    RichText::new("For compatibility with MetaMask and Ledger Live, keep the default (60).")
                        .small()
                        .color(self.theme.text_secondary)
                );
                ui.add_space(self.theme.spacing_xs);

                // Default coin type option
                ui.horizontal(|ui| {
                    if ui.radio(!self.use_custom_coin_type, "").clicked() {
                        self.use_custom_coin_type = false;
                    }
                    ui.label("Use Ethereum coin type (60) for all chains");
                    ui.label(RichText::new("(recommended)").small().color(self.theme.success));
                });

                // Custom coin type option
                ui.horizontal(|ui| {
                    if ui.radio(self.use_custom_coin_type, "").clicked() {
                        self.use_custom_coin_type = true;
                    }
                    ui.label("Custom coin type:");
                    ui.add_enabled(
                        self.use_custom_coin_type,
                        egui::TextEdit::singleline(&mut self.config_coin_type)
                            .desired_width(80.0)
                    );
                });

                ui.add_space(self.theme.spacing_xs);

                // Show common coin types as reference
                ui.collapsing("Common SLIP-44 Coin Types", |ui| {
                    egui::Grid::new("coin_types_grid")
                        .num_columns(4)
                        .spacing([self.theme.spacing_md, self.theme.spacing_xs])
                        .show(ui, |ui| {
                            ui.label(RichText::new("60").monospace().color(self.theme.accent_blue));
                            ui.label("Ethereum / L2s");
                            ui.label(RichText::new("61").monospace().color(self.theme.accent_blue));
                            ui.label("Ethereum Classic");
                            ui.end_row();
                            ui.label(RichText::new("714").monospace().color(self.theme.accent_blue));
                            ui.label("BNB Chain");
                            ui.label(RichText::new("966").monospace().color(self.theme.accent_blue));
                            ui.label("Polygon");
                            ui.end_row();
                            ui.label(RichText::new("9005").monospace().color(self.theme.accent_blue));
                            ui.label("Avalanche");
                            ui.label(RichText::new("1007").monospace().color(self.theme.accent_blue));
                            ui.label("Fantom");
                            ui.end_row();
                            ui.label(RichText::new("52752").monospace().color(self.theme.accent_blue));
                            ui.label("Celo");
                            ui.label(RichText::new("369").monospace().color(self.theme.accent_blue));
                            ui.label("Pulsechain");
                            ui.end_row();
                        });
                    ui.add_space(self.theme.spacing_xs);
                    ui.label(
                        RichText::new("Note: Most wallets use coin type 60 for all EVM chains.")
                            .small()
                            .italics()
                            .color(self.theme.text_secondary)
                    );
                });
            });

            ui.add_space(self.theme.spacing_md);

            // Apply button
            ui.horizontal(|ui| {
                if ui.add(self.theme.button_primary("Apply Changes"))
                    .on_hover_text("Save the derivation path configuration")
                    .clicked()
                {
                    self.config.derivation_mode = self.config_derivation_mode;
                    if let Ok(account) = self.config_custom_account.parse::<u32>() {
                        self.config.custom_account = account;
                    } else {
                        self.config.custom_account = 0;
                        self.config_custom_account = "0".to_string();
                    }
                    if let Ok(addr_idx) = self.config_custom_address_index.parse::<u32>() {
                        self.config.custom_address_index = addr_idx;
                    } else {
                        self.config.custom_address_index = 0;
                        self.config_custom_address_index = "0".to_string();
                    }
                    // Apply coin type
                    if self.use_custom_coin_type {
                        if let Ok(coin_type) = self.config_coin_type.parse::<u32>() {
                            self.config.coin_type = coin_type;
                            self.user_settings.coin_type_override = Some(coin_type);
                        } else {
                            self.config.coin_type = crate::config::DEFAULT_COIN_TYPE;
                            self.config_coin_type = crate::config::DEFAULT_COIN_TYPE.to_string();
                            self.user_settings.coin_type_override = None;
                        }
                    } else {
                        self.config.coin_type = crate::config::DEFAULT_COIN_TYPE;
                        self.user_settings.coin_type_override = None;
                    }
                    // Save settings
                    if let Err(e) = self.user_settings.save() {
                        self.notifications.push_back(NotificationEntry::new(format!("Warning: Failed to save settings: {}", e)));
                    }
                    self.notifications.push_back(NotificationEntry::new("[OK] Derivation path configuration updated"));
                }

                // Check for unsaved changes
                let coin_type_changed = if self.use_custom_coin_type {
                    self.config_coin_type.parse::<u32>().ok() != Some(self.config.coin_type)
                } else {
                    self.config.coin_type != crate::config::DEFAULT_COIN_TYPE
                };
                
                if self.config.derivation_mode != self.config_derivation_mode ||
                   self.config.custom_account.to_string() != self.config_custom_account ||
                   self.config.custom_address_index.to_string() != self.config_custom_address_index ||
                   coin_type_changed {
                    ui.label(RichText::new("(Changes not yet applied)").italics().color(self.theme.warning));
                }
            });

            ui.add_space(self.theme.spacing_md);

            // Current configuration display
            ui.group(|ui| {
                ui.label(RichText::new("Current Configuration").strong());
                ui.add_space(self.theme.spacing_xs);

                let example_index = 5;
                let path = self.config.get_derivation_path(example_index);

                egui::Grid::new("config_display_grid")
                    .num_columns(2)
                    .spacing([self.theme.spacing_sm, self.theme.spacing_xs])
                    .show(ui, |ui| {
                        ui.label("Mode:");
                        ui.label(RichText::new(match self.config.derivation_mode {
                            crate::config::DerivationMode::AccountIndex => "Account-Index",
                            crate::config::DerivationMode::AddressIndex => "Address-Index",
                        }).strong());
                        ui.end_row();

                        ui.label("Coin Type:");
                        let coin_type_text = if self.config.coin_type == crate::config::DEFAULT_COIN_TYPE {
                            format!("{} (Ethereum - default)", self.config.coin_type)
                        } else {
                            format!("{} (custom)", self.config.coin_type)
                        };
                        let coin_type_color = if self.config.coin_type == crate::config::DEFAULT_COIN_TYPE {
                            self.theme.success
                        } else {
                            self.theme.warning
                        };
                        ui.label(RichText::new(coin_type_text).strong().color(coin_type_color));
                        ui.end_row();

                        if self.config.derivation_mode == crate::config::DerivationMode::AccountIndex && self.config.custom_address_index > 0 {
                            ui.label("Constant Address Index:");
                            ui.label(RichText::new(self.config.custom_address_index.to_string()).strong());
                            ui.end_row();
                        }

                        if self.config.derivation_mode == crate::config::DerivationMode::AddressIndex {
                            ui.label("Constant Account:");
                            ui.label(RichText::new(self.config.custom_account.to_string()).strong());
                            ui.end_row();
                        }

                        ui.label("Example (index 5):");
                        ui.monospace(&path);
                        ui.end_row();
                    });
            });
        });
    }

    fn render_custom_networks_settings(&mut self, ui: &mut egui::Ui) {
        use crate::gui::app::NetworkSelection;
        
        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("Custom Networks").size(18.0).strong().color(self.theme.text_primary));
            ui.add_space(self.theme.spacing_sm);
            
            ui.label("Add your own EVM-compatible networks with custom RPC endpoints.");
            ui.add_space(self.theme.spacing_md);

            // Add/Edit Network Form
            ui.group(|ui| {
                let form_title = if self.custom_network_form.editing_chain_id.is_some() {
                    "[~] Edit Network"
                } else {
                    "[+] Add New Network"
                };
                ui.label(RichText::new(form_title).strong().color(self.theme.accent_blue));
                ui.add_space(self.theme.spacing_xs);

                egui::Grid::new("custom_network_form_grid")
                    .num_columns(2)
                    .spacing([self.theme.spacing_sm, self.theme.spacing_xs])
                    .show(ui, |ui| {
                        ui.label("Network Name:");
                        ui.add(egui::TextEdit::singleline(&mut self.custom_network_form.label)
                            .desired_width(300.0)
                            .hint_text("e.g., My Custom Chain"));
                        ui.end_row();

                        ui.label("Chain ID:");
                        let chain_id_enabled = self.custom_network_form.editing_chain_id.is_none();
                        ui.add_enabled(
                            chain_id_enabled,
                            egui::TextEdit::singleline(&mut self.custom_network_form.chain_id)
                                .desired_width(150.0)
                                .hint_text("e.g., 12345")
                        );
                        ui.end_row();

                        ui.label("Native Token Symbol:");
                        ui.add(egui::TextEdit::singleline(&mut self.custom_network_form.native_token)
                            .desired_width(100.0)
                            .hint_text("e.g., ETH"));
                        ui.end_row();

                        ui.label("RPC URL:");
                        ui.add(egui::TextEdit::singleline(&mut self.custom_network_form.rpc_url)
                            .desired_width(400.0)
                            .hint_text("https://rpc.example.com"));
                        ui.end_row();
                    });

                if let Some(err) = &self.custom_network_form.error {
                    ui.add_space(self.theme.spacing_xs);
                    ui.colored_label(self.theme.error, err);
                }

                ui.add_space(self.theme.spacing_sm);
                ui.horizontal(|ui| {
                    let button_text = if self.custom_network_form.editing_chain_id.is_some() {
                        "Update Network"
                    } else {
                        "Add Network"
                    };
                    
                    if ui.add(self.theme.button_primary(button_text)).clicked() {
                        // Validate and add/update network
                        let validation_result = self.validate_and_save_custom_network();
                        match validation_result {
                            Ok(msg) => {
                                self.notifications.push_back(NotificationEntry::new(msg));
                                self.custom_network_form.clear();
                            }
                            Err(err) => {
                                self.custom_network_form.error = Some(err);
                            }
                        }
                    }
                    
                    if self.custom_network_form.editing_chain_id.is_some() {
                        if ui.add(self.theme.button_small("Cancel Edit")).clicked() {
                            self.custom_network_form.clear();
                        }
                    }
                    
                    if !self.custom_network_form.label.is_empty() 
                        || !self.custom_network_form.chain_id.is_empty()
                        || !self.custom_network_form.native_token.is_empty()
                        || !self.custom_network_form.rpc_url.is_empty()
                    {
                        if ui.add(self.theme.button_small("Clear")).clicked() {
                            self.custom_network_form.clear();
                        }
                    }
                });
            });

            ui.add_space(self.theme.spacing_md);

            // List of existing custom networks
            if !self.user_settings.custom_networks.is_empty() {
                ui.group(|ui| {
                    ui.label(RichText::new("[=] Saved Custom Networks").strong().color(self.theme.accent_green));
                    ui.add_space(self.theme.spacing_xs);

                    let mut network_to_delete: Option<u64> = None;
                    let mut network_to_edit: Option<CustomNetwork> = None;

                    egui::Grid::new("custom_networks_list_grid")
                        .num_columns(5)
                        .spacing([self.theme.spacing_md, self.theme.spacing_xs])
                        .striped(true)
                        .show(ui, |ui| {
                            // Header row
                            ui.label(RichText::new("Name").strong());
                            ui.label(RichText::new("Chain ID").strong());
                            ui.label(RichText::new("Token").strong());
                            ui.label(RichText::new("RPC URL").strong());
                            ui.label(RichText::new("Actions").strong());
                            ui.end_row();

                            for net in &self.user_settings.custom_networks {
                                ui.label(&net.label);
                                ui.label(net.chain_id.to_string());
                                ui.label(&net.native_token);
                                // Truncate long RPC URLs
                                let rpc_display = if net.rpc_url.len() > 40 {
                                    format!("{}...", &net.rpc_url[..37])
                                } else {
                                    net.rpc_url.clone()
                                };
                                ui.label(rpc_display).on_hover_text(&net.rpc_url);
                                ui.horizontal(|ui| {
                                    if ui.add(self.theme.button_small("Edit")).clicked() {
                                        network_to_edit = Some(net.clone());
                                    }
                                    if ui.add(
                                        egui::Button::new(RichText::new("Delete").color(self.theme.error))
                                            .fill(self.theme.secondary)
                                            .stroke(egui::Stroke::new(1.0, self.theme.error))
                                    ).clicked() {
                                        network_to_delete = Some(net.chain_id);
                                    }
                                });
                                ui.end_row();
                            }
                        });

                    // Handle edit action
                    if let Some(net) = network_to_edit {
                        self.custom_network_form.populate_from(&net);
                    }

                    // Handle delete action
                    if let Some(chain_id) = network_to_delete {
                        // Check if this network is currently selected
                        let is_currently_selected = matches!(
                            &self.network_selection,
                            NetworkSelection::Custom(id) if *id == chain_id
                        );
                        
                        if is_currently_selected {
                            // Switch to first built-in network before deleting
                            self.network_selection = NetworkSelection::Builtin(0);
                            self.apply_network_selection();
                        }
                        
                        self.user_settings.remove_custom_network(chain_id);
                        if let Err(e) = self.user_settings.save() {
                            self.notifications.push_back(NotificationEntry::new(format!("Failed to save: {}", e)));
                        } else {
                            self.notifications.push_back(NotificationEntry::new("Custom network deleted."));
                        }
                    }
                });
            } else {
                ui.label(RichText::new("No custom networks configured yet.").italics().color(self.theme.text_secondary));
            }
        });
    }

    /// Validate the custom network form and save/update the network
    pub(crate) fn validate_and_save_custom_network(&mut self) -> Result<String, String> {
        // Validate label
        let label = self.custom_network_form.label.trim().to_string();
        if label.is_empty() {
            return Err("Network name is required.".to_string());
        }

        // Validate chain ID
        let chain_id: u64 = self.custom_network_form.chain_id.trim()
            .parse()
            .map_err(|_| "Chain ID must be a valid positive number.".to_string())?;
        
        if chain_id == 0 {
            return Err("Chain ID cannot be zero.".to_string());
        }

        // Check if chain ID conflicts with built-in networks
        if crate::config::is_builtin_chain_id(chain_id) {
            return Err(format!("Chain ID {} is already used by a built-in network.", chain_id));
        }

        // Validate native token
        let native_token = self.custom_network_form.native_token.trim().to_uppercase();
        if native_token.is_empty() {
            return Err("Native token symbol is required.".to_string());
        }

        // Validate RPC URL
        let rpc_url = self.custom_network_form.rpc_url.trim().to_string();
        if rpc_url.is_empty() {
            return Err("RPC URL is required.".to_string());
        }
        if !rpc_url.starts_with("http://") && !rpc_url.starts_with("https://") {
            return Err("RPC URL must start with http:// or https://".to_string());
        }

        let network = CustomNetwork::new(label.clone(), chain_id, native_token, rpc_url);

        // Check if we're editing or adding
        if let Some(editing_id) = self.custom_network_form.editing_chain_id {
            if editing_id == chain_id {
                // Updating existing network
                self.user_settings.update_custom_network(network);
                self.user_settings.save().map_err(|e| format!("Failed to save: {}", e))?;
                Ok(format!("Network '{}' updated successfully.", label))
            } else {
                Err("Cannot change chain ID when editing. Delete and recreate instead.".to_string())
            }
        } else {
            // Adding new network - check for duplicates
            if self.user_settings.custom_networks.iter().any(|n| n.chain_id == chain_id) {
                return Err(format!("A custom network with chain ID {} already exists.", chain_id));
            }
            
            self.user_settings.add_custom_network(network);
            self.user_settings.save().map_err(|e| format!("Failed to save: {}", e))?;
            Ok(format!("Network '{}' added successfully.", label))
        }
    }
}
