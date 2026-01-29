//! Dashboard view implementation
//!
//! Contains the main dashboard rendering with network status,
//! balance lookup, operation logs, and about section.

use crate::gui::app::{GuiApp, GuiSection};
use crate::gui::notifications::NotificationEntry;
use crate::gui::theme::AppTheme;
use anyhow::anyhow;
use eframe::egui::{self, RichText};
use ethers::prelude::Middleware;

impl GuiApp {
    /// Main dashboard view
    pub(crate) fn view_dashboard(&mut self, ui: &mut egui::Ui) {
        // Auto-load logs on first visit to Dashboard if not already loaded
        if self.log_view.content == "No logs yet. Run an operation to generate entries." && self.log_view.job.is_none() {
            self.refresh_logs();
        }

        // Section header
        self.render_section_header(ui, "[H]", "DASHBOARD");
        ui.add_space(self.theme.spacing_md);

        // Top section: Network Status and Recent Notifications in columns
        self.render_dashboard_top_section(ui);

        ui.add_space(self.theme.spacing_lg);

        // Bottom section: Operation Logs (full width)
        self.render_dashboard_logs(ui);

        ui.add_space(self.theme.spacing_lg);

        // About Beaug Panel
        self.theme.frame_panel().show(ui, |ui| {
            ui.label(RichText::new("About Beaug").size(16.0).strong().color(self.theme.text_primary));
            ui.add_space(self.theme.spacing_sm);

            egui::Grid::new("about_grid")
                .num_columns(2)
                .spacing([self.theme.spacing_md, self.theme.spacing_xs])
                .show(ui, |ui| {
                    ui.label(RichText::new("Version:").color(self.theme.text_secondary));
                    ui.label(RichText::new(env!("CARGO_PKG_VERSION")).strong().color(self.theme.accent_green));
                    ui.end_row();

                    ui.label(RichText::new("Settings file:").color(self.theme.text_secondary));
                    let settings_path = crate::user_settings::UserSettings::settings_path_display();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&settings_path).small().color(self.theme.text_secondary));
                        if ui.add(egui::Button::new("📋").small()).on_hover_text("Copy path").clicked() {
                            ui.output_mut(|o| o.copied_text = settings_path.clone());
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Log file:").color(self.theme.text_secondary));
                    let log_path = crate::operation_log::log_file_path();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&log_path).small().color(self.theme.text_secondary));
                        if ui.add(egui::Button::new("📋").small()).on_hover_text("Copy path").clicked() {
                            ui.output_mut(|o| o.copied_text = log_path.clone());
                        }
                    });
                    ui.end_row();
                });

            ui.add_space(self.theme.spacing_sm);
            ui.horizontal(|ui| {
                if ui.link(RichText::new("📖 README").color(self.theme.accent_blue)).clicked() {
                    if let Err(e) = open::that("https://github.com/schbz/beaug#readme") {
                        self.notifications.push_back(NotificationEntry::new(format!("Failed to open URL: {}", e)));
                    }
                }
                ui.separator();
                if ui.link(RichText::new("📋 Changelog").color(self.theme.accent_blue)).clicked() {
                    if let Err(e) = open::that("https://github.com/schbz/beaug/blob/main/CHANGELOG.md") {
                        self.notifications.push_back(NotificationEntry::new(format!("Failed to open URL: {}", e)));
                    }
                }
                ui.separator();
                if ui.link(RichText::new("🐛 Report Issue").color(self.theme.accent_blue)).clicked() {
                    if let Err(e) = open::that("https://github.com/schbz/beaug/issues") {
                        self.notifications.push_back(NotificationEntry::new(format!("Failed to open URL: {}", e)));
                    }
                }
            });
        });
    }

    /// Render a consistent section header with retro ASCII styling
    pub(crate) fn render_section_header(&self, ui: &mut egui::Ui, icon: &str, title: &str) {
        let header_text = self.theme.section_header_text(icon, title);
        let separator = "=".repeat(40);
        
        ui.label(RichText::new(&separator).size(14.0).color(self.theme.primary));
        ui.label(RichText::new(&header_text).size(24.0).strong().color(self.theme.text_primary));
        ui.label(RichText::new(&separator).size(14.0).color(self.theme.primary));
    }

    fn render_dashboard_top_section(&mut self, ui: &mut egui::Ui) {
        // Network Status Panel (full width)
        self.render_network_status_panel(ui);

        ui.add_space(self.theme.spacing_md);

        // Quick Balance Lookup Panel (full width)
        self.render_balance_lookup_panel(ui);
    }

    fn render_network_status_panel(&mut self, ui: &mut egui::Ui) {
        let (network_label, native_token, _, _) = self.selected_network_info();

        // Poll RPC status job if running
        if let Some(job) = &mut self.rpc_status_job {
            if let Some(result) = job.poll() {
                match result {
                    Ok(latency) => {
                        self.rpc_latency_ms = Some(latency);
                    }
                    Err(_) => {
                        self.rpc_latency_ms = None;
                    }
                }
                self.rpc_status_job = None;
            }
        }

        // Network Status Panel (read-only)
        self.theme.frame_panel().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("[@] Network Status").size(16.0).strong().color(self.theme.text_primary));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Refresh button
                    let is_checking = self.rpc_status_job.is_some();
                    if ui.add_enabled(!is_checking, egui::Button::new(if is_checking { "⏳" } else { "🔄" }).small())
                        .on_hover_text("Check RPC connection")
                        .clicked() 
                    {
                        self.start_rpc_check();
                    }
                    
                    // Status indicator
                    if is_checking {
                        ui.label(RichText::new("Checking...").small().color(self.theme.warning));
                    } else if let Some(latency) = self.rpc_latency_ms {
                        let (status_color, status_text) = if latency < 200 {
                            (self.theme.success, format!("🟢 {}ms", latency))
                        } else if latency < 1000 {
                            (self.theme.warning, format!("🟡 {}ms", latency))
                        } else {
                            (self.theme.error, format!("🔴 {}ms", latency))
                        };
                        ui.label(RichText::new(status_text).small().color(status_color));
                    }
                });
            });
            ui.add_space(self.theme.spacing_sm);

            egui::Grid::new("network_status_grid")
                .num_columns(2)
                .spacing([self.theme.spacing_md, self.theme.spacing_xs])
                .show(ui, |ui| {
                    ui.label(RichText::new("Network:").color(self.theme.text_secondary));
                    ui.label(RichText::new(&network_label).strong().color(self.theme.accent_green));
                    ui.end_row();

                    ui.label(RichText::new("Chain ID:").color(self.theme.text_secondary));
                    ui.label(RichText::new(self.config.chain_id.to_string()).color(self.theme.accent_green));
                    ui.end_row();

                    ui.label(RichText::new("Symbol:").color(self.theme.text_secondary));
                    ui.label(RichText::new(&native_token).color(self.theme.accent_green));
                    ui.end_row();

                    ui.label(RichText::new("RPC:").color(self.theme.text_secondary));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&self.config.rpc_url).small().color(self.theme.accent_green));
                        if ui.add(egui::Button::new("📋").small()).on_hover_text("Copy RPC URL").clicked() {
                            ui.output_mut(|o| o.copied_text = self.config.rpc_url.clone());
                        }
                    });
                    ui.end_row();
                });

            ui.add_space(self.theme.spacing_sm);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Need to change settings?").small().color(self.theme.text_secondary));
                if ui.link(RichText::new("Go to Settings").small().color(self.theme.accent_blue)).clicked() {
                    self.previous_section = self.section;
                    self.section = GuiSection::Settings;
                }
            });
        });
    }

    pub(crate) fn start_rpc_check(&mut self) {
        let rpc_url = self.config.rpc_url.clone();
        self.rpc_status_job = Some(self.spawn_job(move || async move {
            let start = std::time::Instant::now();
            let provider = ethers::providers::Provider::<ethers::providers::Http>::try_from(&rpc_url)?;
            let _ = provider.get_block_number().await?;
            let elapsed = start.elapsed().as_millis() as u64;
            Ok(elapsed)
        }));
        self.last_rpc_check = std::time::Instant::now();
    }

    fn render_balance_lookup_panel(&mut self, ui: &mut egui::Ui) {
        // Use responsive width for the panels
        let panel_width = AppTheme::responsive_width(ui, 300.0, 600.0, 800.0);

        // Quick Balance Lookup Panel
        self.theme.frame_panel().show(ui, |ui| {
            ui.set_min_width(panel_width);
            ui.label(RichText::new("[?] Quick Balance Lookup").size(16.0).strong().color(self.theme.text_primary));
            ui.add_space(self.theme.spacing_sm);

            ui.horizontal(|ui| {
                ui.label("Address index:");
                ui.add(
                    egui::DragValue::new(&mut self.balance_view.index)
                        .speed(1)
                        .clamp_range(0..=10_000),
                );
                if ui.add(self.theme.button_primary("Fetch"))
                    .clicked()
                    && self.balance_view.job.is_none()
                {
                    let config = self.config.clone();
                    let index = self.balance_view.index;
                    self.balance_view.job = Some(self.spawn_job(move || async move {
                        let provider = config.get_provider().await?;
                        let address = crate::ledger_ops::get_ledger_address_with_config(config.chain_id, index, Some(&config)).await?;
                        let balance = provider.get_balance(address, None).await?;
                        let derivation_path = config.get_derivation_path(index);
                        Ok((format!("{} -> {:?}", derivation_path, address), crate::utils::format_ether(balance)))
                    }));
                }
            });

            if let Some(addr) = &self.balance_view.address {
                ui.add_space(self.theme.spacing_xs);
                ui.monospace(RichText::new(format!("  {}", addr)).small());
            }
            if let Some(balance) = &self.balance_view.balance {
                let (_, native_token, _, _) = self.selected_network_info();
                ui.label(RichText::new(format!("  Balance: {} {}", balance, native_token)).color(self.theme.success));
            }
            if let Some(err) = &self.balance_view.error {
                ui.colored_label(self.theme.error, format!("  [XX] {}", err));
            }
        });
    }

    pub(crate) fn refresh_logs(&mut self) {
        if self.log_view.job.is_none() {
            self.log_view.scroll_to_bottom = true; // Scroll to bottom after refresh
            self.log_view.job = Some(self.spawn_job(|| async move {
                match crate::operation_log::read_log() {
                    Ok(content) if content.is_empty() => {
                        Ok("Log file not found yet.".to_string())
                    }
                    Ok(content) => Ok(content),
                    Err(e) => Err(anyhow!("Failed to read log file: {}", e)),
                }
            }));
        }
    }

    fn render_dashboard_logs(&mut self, ui: &mut egui::Ui) {
        // Operation Logs Panel
        ui.horizontal(|ui| {
            ui.heading(RichText::new("[#] Operation Log").size(18.0));
            ui.add_space(self.theme.spacing_sm);
            let is_loading = self.log_view.job.is_some();
            if ui
                .add_enabled(
                    !is_loading,
                    egui::Button::new(
                        egui::RichText::new(if is_loading { "[..]" } else { "[R] Refresh" })
                            .color(self.theme.text_primary)
                    )
                        .fill(self.theme.secondary)
                        .stroke(egui::Stroke::new(1.0, self.theme.surface_active))
                        .small(),
                )
                .clicked()
            {
                self.refresh_logs();
            }
        });
        ui.add_space(self.theme.spacing_xs);

        if let Some(err) = &self.log_view.error {
            ui.colored_label(egui::Color32::LIGHT_RED, err);
        }

        let scroll_to_bottom = self.log_view.scroll_to_bottom;
        self.theme.frame_surface().show(ui, |ui| {
                ui.set_min_height(300.0);
                let scroll_area = egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(400.0)
                    .animated(true);
                
                scroll_area.show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.monospace(&self.log_view.content);
                    
                    // Scroll to the bottom with animation when flag is set
                    if scroll_to_bottom {
                        // Add invisible marker at the end and scroll to it
                        let bottom = ui.label("");
                        bottom.scroll_to_me(Some(egui::Align::BOTTOM));
                    }
                });
            });
        
        // Reset scroll flag after rendering
        if self.log_view.scroll_to_bottom {
            self.log_view.scroll_to_bottom = false;
        }
    }
}
