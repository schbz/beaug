//! Balance checking view implementation
//!
//! This module contains the balance scanning panel rendering including:
//! - Scan parameters configuration
//! - Live streaming results display
//! - Export functionality

use crate::{balance, utils};
use eframe::egui::{self, RichText};
use std::fs;
use std::io::Write;
use std::path::PathBuf;

use super::super::app::{GuiApp, SplitSelector};
use super::super::notifications::NotificationEntry;

/// Renders the Check Balances view
pub fn view_check_balances(app: &mut GuiApp, ui: &mut egui::Ui) {
    // Section header
    app.render_section_header(ui, "[?]", "SCAN ADDRESSES");
    ui.add_space(app.theme.spacing_sm);

    let running = app
        .check_state
        .job
        .as_ref()
        .map(|job| job.is_running())
        .unwrap_or(false);

    let has_results =
        app.check_state.result.is_some() || !app.check_state.streaming_records.is_empty();
    let scan_complete = app.check_state.result.is_some() && !running;

    // When scan starts, hide parameters
    if running && app.check_state.show_parameters {
        app.check_state.show_parameters = false;
    }

    // Show parameters panel OR results panel (not both)
    if app.check_state.show_parameters && !running && !has_results {
        // === PARAMETERS MODE ===
        ui.label(
            RichText::new("Scan addresses from your Ledger device to check their balances")
                .color(app.theme.text_secondary),
        );
        ui.add_space(app.theme.spacing_md);

        // Scan parameters in a grid layout
        app.theme.frame_panel().show(ui, |ui| {
            ui.label(
                RichText::new("Scan Parameters")
                    .strong()
                    .color(app.theme.text_primary),
            );
            ui.add_space(app.theme.spacing_sm);

            egui::Grid::new("scan_params_grid")
                .num_columns(2)
                .spacing([app.theme.spacing_md, app.theme.spacing_sm])
                .show(ui, |ui| {
                    ui.label("Start index:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut app.check_state.start_index)
                                .clamp_range(0..=100_000),
                        );
                        ui.label(
                            RichText::new("(First address to scan)")
                                .small()
                                .color(app.theme.text_secondary),
                        );
                    });
                    ui.end_row();

                    ui.label("Stop after:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut app.check_state.empty_target)
                                .clamp_range(1..=100),
                        );
                        ui.label(
                            RichText::new("consecutive empty addresses")
                                .small()
                                .color(app.theme.text_secondary),
                        );
                    });
                    ui.end_row();
                });
        });

        ui.add_space(app.theme.spacing_md);

        // Check ledger status for enabling the button
        let ledger_ready = app.ledger_status.is_usable();

        let button = app.theme.button_primary("Start scan");
        let button_enabled = ledger_ready;
        let hover_text = if ledger_ready {
            "Start scanning for addresses".to_string()
        } else {
            app.get_ledger_warning_message()
                .unwrap_or_else(|| "Ledger not ready".to_string())
        };

        if ui
            .add_enabled(button_enabled, button)
            .on_hover_text(&hover_text)
            .clicked()
        {
            let config = app.config.clone();
            let target = app.check_state.empty_target;
            let start = app.check_state.start_index;
            let use_native_ledger = app.user_settings.use_native_ledger;

            // Clear previous results and switch to results mode
            app.check_state.result = None;
            app.check_state.streaming_records.clear();
            app.check_state.error = None;
            app.check_state.show_parameters = false;

            // Create channels for progress and cancellation
            let (progress_sender, progress_receiver) = tokio::sync::mpsc::unbounded_channel();
            let (cancel_sender, cancel_receiver) = tokio::sync::oneshot::channel();

            // Store receivers and cancel sender
            app.check_state.progress_receiver = Some(progress_receiver);
            app.check_state.cancel_sender = Some(cancel_sender);

            // Start the streaming scan
            app.check_state.job = Some(app.spawn_job(move || async move {
                balance::scan_consecutive_empty_streaming(
                    config,
                    target,
                    start,
                    progress_sender,
                    cancel_receiver,
                    use_native_ledger,
                )
                .await
            }));
        }

        // Show ledger warning if not ready
        if !ledger_ready {
            app.render_ledger_warning(ui);
        }
    } else {
        // === RESULTS MODE (running or has results) ===

        // Status bar with scan controls
        ui.horizontal(|ui| {
            if running {
                ui.label(RichText::new("[..] Scanning...").color(app.theme.accent_green));

                // Cancel button
                if ui.button("[X] Cancel").clicked() {
                    if let Some(sender) = app.check_state.cancel_sender.take() {
                        let _ = sender.send(());
                    }
                }

                // Show current progress
                let current_count = app.check_state.streaming_records.len();
                if current_count > 0 {
                    ui.label(format!("({} addresses found)", current_count));
                }

                ui.label(
                    RichText::new("Keep Ledger unlocked")
                        .small()
                        .color(app.theme.text_secondary),
                );
            } else if scan_complete || has_results {
                // New Scan button
                if ui
                    .button("🔄 New Scan")
                    .on_hover_text("Start a new scan with different parameters")
                    .clicked()
                {
                    // Clear results and show parameters
                    app.check_state.result = None;
                    app.check_state.streaming_records.clear();
                    app.check_state.error = None;
                    app.check_state.show_parameters = true;
                }

                ui.separator();
            }
        });

        if let Some(err) = &app.check_state.error {
            ui.colored_label(egui::Color32::LIGHT_RED, err);
        }

        // Display results - either final results or streaming records
        let records: Vec<_> = if let Some(result) = &app.check_state.result {
            result.records.clone()
        } else if !app.check_state.streaming_records.is_empty() {
            app.check_state.streaming_records.clone()
        } else {
            return; // Nothing to display yet
        };

        render_address_table(app, ui, &records);
    }
}

/// Render the address results table
fn render_address_table(
    app: &mut GuiApp,
    ui: &mut egui::Ui,
    records: &[balance::BalanceScanRecord],
) {
    if records.is_empty() {
        return;
    }

    ui.add_space(app.theme.spacing_sm);

    // Show summary with counts
    let funded_count = records.iter().filter(|r| !r.balance.is_zero()).count();
    let empty_count = records.iter().filter(|r| r.balance.is_zero()).count();

    ui.horizontal(|ui| {
        let is_complete = app.check_state.result.is_some();
        let label = if is_complete {
            format!("Scanned {} addresses:", records.len())
        } else {
            format!("Scanning... ({} addresses found):", records.len())
        };
        ui.label(label);

        if funded_count > 0 {
            ui.colored_label(
                egui::Color32::from_rgb(100, 200, 150),
                format!("🟢 {} funded", funded_count),
            );
        }
        if empty_count > 0 {
            ui.colored_label(egui::Color32::GRAY, format!("⚪ {} empty", empty_count));
        }
    });

    if let Some(result) = &app.check_state.result {
        if result.met_target {
            ui.label(format!(
                "[OK] Found {} consecutive empty addresses (target met)",
                result.empty_addresses.len()
            ));
        }
    }

    ui.add_space(app.theme.spacing_xs);

    // Export/Copy buttons
    ui.horizontal(|ui| {
        if ui
            .button("[#] Copy Addresses")
            .on_hover_text("Copy all addresses to clipboard")
            .clicked()
        {
            let addresses: Vec<String> = records.iter().map(|r| format!("{:?}", r.address)).collect();
            ui.output_mut(|o| o.copied_text = addresses.join("\n"));
            app.notifications
                .push_back(NotificationEntry::new("[OK] Addresses copied to clipboard"));
        }

        if ui
            .button("💾 Export CSV")
            .on_hover_text("Save as CSV file")
            .clicked()
        {
            // Create the export directory if it doesn't exist
            if let Err(e) = fs::create_dir_all(&app.config.export_directory) {
                app.notifications.push_back(NotificationEntry::new(format!(
                    "[XX] Failed to create export directory: {}",
                    e
                )));
            } else {
                // Generate filename with timestamp
                let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
                let mut path = PathBuf::from(&app.config.export_directory);
                path.push(format!("addresses_{}.csv", timestamp));
                let filename = path.to_string_lossy().to_string();

                // Generate CSV content
                let mut csv = String::from("Path,Address,Balance,Token,Status\n");
                for record in records {
                    let status = if record.balance.is_zero() {
                        "Empty"
                    } else {
                        "Funded"
                    };
                    csv.push_str(&format!(
                        "\"{}\",\"{:?}\",\"{}\",\"{}\",\"{}\"\n",
                        record.derivation_path,
                        record.address,
                        utils::format_ether(record.balance),
                        app.config.native_token(),
                        status
                    ));
                }

                // Save to file
                match fs::File::create(&filename) {
                    Ok(mut file) => {
                        if let Err(e) = file.write_all(csv.as_bytes()) {
                            app.notifications.push_back(NotificationEntry::new(format!(
                                "[XX] Failed to write CSV: {}",
                                e
                            )));
                        } else {
                            app.notifications.push_back(NotificationEntry::new(format!(
                                "[OK] CSV saved & path copied: {}",
                                filename
                            )));

                            // Also copy to clipboard for convenience
                            ui.output_mut(|o| o.copied_text = filename.clone());
                        }
                    }
                    Err(e) => {
                        app.notifications.push_back(NotificationEntry::new(format!(
                            "[XX] Failed to create file: {}",
                            e
                        )));
                    }
                }
            }
        }

        if records.iter().any(|r| !r.balance.is_zero()) {
            if ui
                .button("[$] Copy Funded Only")
                .on_hover_text("Copy only funded addresses")
                .clicked()
            {
                let funded: Vec<String> = records
                    .iter()
                    .filter(|r| !r.balance.is_zero())
                    .map(|r| format!("{:?}", r.address))
                    .collect();
                ui.output_mut(|o| o.copied_text = funded.join("\n"));
                app.notifications
                    .push_back(NotificationEntry::new("[OK] Funded addresses copied to clipboard"));
            }
        }

        if records.iter().any(|r| r.balance.is_zero()) {
            if ui
                .button("[∅] Copy Empty Only")
                .on_hover_text("Copy only empty addresses")
                .clicked()
            {
                let empty: Vec<String> = records
                    .iter()
                    .filter(|r| r.balance.is_zero())
                    .map(|r| format!("{:?}", r.address))
                    .collect();
                ui.output_mut(|o| o.copied_text = empty.join("\n"));
                app.notifications
                    .push_back(NotificationEntry::new("[OK] Empty addresses copied to clipboard"));
            }
        }
    });

    ui.add_space(app.theme.spacing_xs);

    // Display addresses with individual copy buttons
    let mut copied_address: Option<String> = None;
    let mut add_split_even: Option<String> = None;
    let mut add_split_random: Option<String> = None;
    let mut add_bulk_disperse: Option<String> = None;

    egui::ScrollArea::vertical()
        .max_height(ui.available_height() - 20.0)
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for record in records {
                let address_str = format!("{:?}", record.address);
                
                ui.horizontal(|ui| {
                    // Choose color based on balance
                    let (color, status_icon) = if record.balance.is_zero() {
                        (egui::Color32::GRAY, "⚪")
                    } else {
                        (egui::Color32::from_rgb(100, 200, 150), "🟢")
                    };

                    ui.colored_label(color, status_icon);

                    // Format the display text
                    let display_text = format!(
                        "{} → {} - {} {}",
                        record.derivation_path,
                        address_str,
                        utils::format_ether(record.balance),
                        app.config.native_token()
                    );

                    // Make the text selectable and clickable
                    if ui
                        .add(
                            egui::Label::new(RichText::new(&display_text).color(color))
                                .sense(egui::Sense::click()),
                        )
                        .on_hover_text("Click to copy address")
                        .clicked()
                    {
                        ui.output_mut(|o| o.copied_text = address_str.clone());
                        copied_address = Some(address_str.clone());
                    }

                    ui.scope(|ui| {
                        ui.style_mut().spacing.button_padding = egui::vec2(2.0, 1.0);

                        // Compact action buttons
                        if ui
                            .add(egui::Button::new("[#]").small())
                            .on_hover_text("Copy address")
                            .clicked()
                        {
                            ui.output_mut(|o| o.copied_text = address_str.clone());
                            copied_address = Some(address_str.clone());
                        }

                        if ui
                            .add(egui::Button::new("[=]").small())
                            .on_hover_text("Add to Split Even recipients")
                            .clicked()
                        {
                            add_split_even = Some(address_str.clone());
                        }

                        if ui
                            .add(egui::Button::new("[~]").small())
                            .on_hover_text("Add to Split Random recipients")
                            .clicked()
                        {
                            add_split_random = Some(address_str.clone());
                        }

                        if ui
                            .add(egui::Button::new("[$]").small())
                            .on_hover_text("Add to Bulk Disperse list")
                            .clicked()
                        {
                            add_bulk_disperse = Some(address_str.clone());
                        }
                    });
                });
            }
        });

    // Add notification if an address was copied
    if let Some(addr) = copied_address {
        app.notifications.push_back(NotificationEntry::new(format!(
            "[OK] Copied: {}...{}",
            &addr[..6],
            &addr[addr.len() - 4..]
        )));
    }

    if let Some(addr) = add_split_even {
        app.append_split_recipient(SplitSelector::Equal, &addr);
        app.notifications.push_back(NotificationEntry::new(format!(
            "[OK] Split Even: added {}...{}",
            &addr[..6],
            &addr[addr.len() - 4..]
        )));
    }

    if let Some(addr) = add_split_random {
        app.append_split_recipient(SplitSelector::Random, &addr);
        app.notifications.push_back(NotificationEntry::new(format!(
            "[OK] Split Random: added {}...{}",
            &addr[..6],
            &addr[addr.len() - 4..]
        )));
    }

    if let Some(addr) = add_bulk_disperse {
        app.append_bulk_disperse_recipient(&addr);
        app.notifications.push_back(NotificationEntry::new(format!(
            "[OK] Bulk Disperse: added {}...{}",
            &addr[..6],
            &addr[addr.len() - 4..]
        )));
    }
}
