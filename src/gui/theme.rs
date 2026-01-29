//! Centralized theme and styling system for the GUI
//!
//! Provides the AppTheme struct with colors, spacing, and styled widget factories.

use eframe::egui;

/// Centralized theme and styling system
#[derive(Clone, Copy)]
pub struct AppTheme {
    // Base colors
    pub background: egui::Color32,
    pub surface: egui::Color32,
    pub surface_hover: egui::Color32,
    pub surface_active: egui::Color32,
    pub panel_fill: egui::Color32,
    pub text_primary: egui::Color32,
    pub text_secondary: egui::Color32,

    // Semantic colors
    pub primary: egui::Color32,
    pub primary_hover: egui::Color32,
    pub secondary: egui::Color32,
    pub success: egui::Color32,
    pub success_hover: egui::Color32,
    pub warning: egui::Color32,
    pub error: egui::Color32,
    pub info: egui::Color32,

    // Accent colors
    pub accent_blue: egui::Color32,
    pub accent_green: egui::Color32,
    pub accent_orange: egui::Color32,
    pub accent_purple: egui::Color32,

    // Spacing constants
    pub spacing_xs: f32, // 4px
    pub spacing_sm: f32, // 8px
    pub spacing_md: f32, // 16px
    pub spacing_lg: f32, // 24px
    pub spacing_xl: f32, // 32px

    // Button sizes
    pub button_small: egui::Vec2,  // 80x24
    pub button_medium: egui::Vec2, // 120x32
    pub button_large: egui::Vec2,  // 160x40
}

impl Default for AppTheme {
    fn default() -> Self {
        Self {
            // Cassette Futurism color scheme - dark background with bright green accents
            background: egui::Color32::from_rgb(8, 8, 8),         // Very dark background
            surface: egui::Color32::from_rgb(15, 15, 15),         // Slightly lighter surface
            surface_hover: egui::Color32::from_rgb(25, 25, 25),   // Hover state
            surface_active: egui::Color32::from_rgb(35, 35, 35),  // Active state
            panel_fill: egui::Color32::from_rgb(12, 12, 12),      // Panel background
            text_primary: egui::Color32::from_rgb(0, 221, 119),   // Bright green text (#00dd77)
            text_secondary: egui::Color32::from_rgb(170, 170, 170), // Gray for secondary text

            // Semantic colors with cassette futurism theme
            primary: egui::Color32::from_rgb(0, 221, 119),       // Main green (#00dd77)
            primary_hover: egui::Color32::from_rgb(0, 255, 136), // Brighter green hover
            secondary: egui::Color32::from_rgb(80, 80, 80),      // Dark gray for secondary
            success: egui::Color32::from_rgb(0, 221, 119),       // Same as primary for success
            success_hover: egui::Color32::from_rgb(0, 255, 136),
            warning: egui::Color32::from_rgb(255, 170, 0), // Amber warning (#ffaa00)
            error: egui::Color32::from_rgb(255, 85, 85),   // Red error
            info: egui::Color32::from_rgb(0, 221, 119),    // Same as primary for info

            // Accent colors - amber and green tones
            accent_blue: egui::Color32::from_rgb(0, 170, 170),   // Cyan accent
            accent_green: egui::Color32::from_rgb(0, 221, 119),  // Main green
            accent_orange: egui::Color32::from_rgb(255, 170, 0), // Amber accent
            accent_purple: egui::Color32::from_rgb(170, 0, 170), // Magenta accent

            // Spacing scale - slightly larger for retro readability
            spacing_xs: 6.0,
            spacing_sm: 12.0,
            spacing_md: 20.0,
            spacing_lg: 28.0,
            spacing_xl: 36.0,

            // Button sizes - more retro terminal-like
            button_small: egui::vec2(100.0, 28.0),
            button_medium: egui::vec2(140.0, 36.0),
            button_large: egui::vec2(180.0, 44.0),
        }
    }
}

impl AppTheme {
    /// Create a themed button with consistent sizing and colors
    pub fn button_primary(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(text)
                .color(self.text_primary) // Explicit text color for readability
                .strong(),
        )
        .fill(self.surface) // Dark background
        .stroke(egui::Stroke::new(3.0, self.primary)) // Bright green border
        .min_size(self.button_medium)
    }

    /// Create a themed button for success actions
    pub fn button_success(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(text)
                .color(self.text_primary) // Explicit text color for readability
                .strong(),
        )
        .fill(self.surface) // Dark background
        .stroke(egui::Stroke::new(3.0, self.success)) // Bright green border
        .min_size(self.button_medium)
    }

    /// Create a themed button for warning actions
    pub fn button_warning(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(
            egui::RichText::new(text)
                .color(self.text_primary) // Explicit text color for readability
                .strong(),
        )
        .fill(self.surface) // Dark background
        .stroke(egui::Stroke::new(3.0, self.primary)) // Bright green border (consistent with other buttons)
        .min_size(self.button_medium)
    }

    /// Create a themed secondary button (outlined style)
    pub fn button_secondary(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(egui::RichText::new(text).color(self.text_primary))
            .fill(self.surface)
            .stroke(egui::Stroke::new(2.0, self.secondary))
            .min_size(self.button_medium)
    }

    /// Create a small themed button
    pub fn button_small(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(egui::RichText::new(text).color(self.text_primary))
            .fill(self.secondary)
            .stroke(egui::Stroke::new(1.0, self.surface_active))
            .min_size(self.button_small)
    }

    /// Create a large themed button
    pub fn button_large(&self, text: &str) -> egui::Button<'_> {
        egui::Button::new(text)
            .fill(self.primary)
            .min_size(self.button_large)
    }

    /// Create a themed frame for surface elements
    pub fn frame_surface(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.surface)
            .rounding(2.0) // Sharp corners for retro feel
            .inner_margin(self.spacing_md)
            .stroke(egui::Stroke::new(1.0, self.accent_green))
    }

    /// Create a themed frame for panels/cards
    pub fn frame_panel(&self) -> egui::Frame {
        egui::Frame::none()
            .fill(self.panel_fill)
            .rounding(2.0) // Sharp corners for retro terminal look
            .inner_margin(self.spacing_md)
            .stroke(egui::Stroke::new(2.0, self.accent_green)) // Thicker green border
    }

    /// Calculate responsive width clamped to min/max bounds
    pub fn responsive_width(ui: &egui::Ui, min: f32, preferred: f32, max: f32) -> f32 {
        let available = ui.available_width();
        available.clamp(min, max.min(preferred))
    }

    /// Create a section header with retro ASCII styling
    pub fn section_header_text(&self, icon: &str, title: &str) -> String {
        format!("  {} {}", icon, title)
    }
}

/// Configure the egui context style with the given theme
pub fn configure_style(ctx: &egui::Context, theme: &AppTheme) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = theme.background;
    visuals.panel_fill = theme.panel_fill;
    visuals.override_text_color = Some(theme.text_primary);

    // Customize widget visuals to use theme colors
    visuals.widgets.noninteractive.bg_fill = theme.surface;
    visuals.widgets.inactive.bg_fill = theme.surface;
    visuals.widgets.hovered.bg_fill = theme.surface_hover;
    visuals.widgets.active.bg_fill = theme.surface_active;
    visuals.widgets.open.bg_fill = theme.surface_active;

    // Style text input boxes with accent colors for visibility
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(2.0, theme.accent_green);
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(2.0, theme.accent_green);
    visuals.widgets.active.bg_stroke = egui::Stroke::new(3.0, theme.primary); // Thicker and brighter when active

    // Use default fonts but configure them for retro terminal styling
    ctx.set_visuals(visuals);

    // Additional styling for retro terminal feel
    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = egui::vec2(8.0, 6.0); // Tighter spacing for compact retro look
    style.spacing.button_padding = egui::vec2(12.0, 8.0); // Comfortable button padding
    style.spacing.menu_margin = egui::Margin::same(8.0);
    style.spacing.indent = 20.0; // Standard indentation

    // Make text slightly larger for retro readability
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(20.0, egui::FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(14.0, egui::FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(14.0, egui::FontFamily::Monospace),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(12.0, egui::FontFamily::Monospace),
    );

    ctx.set_style(style);
}
