use egui::emath::GuiRounding as _;

use crate::model::LogicalPx;

/// UI theme colors used across all rendering (egui + GPU).
/// All colors are in the Catppuccin Mocha palette for the dark theme.

#[derive(Debug, Clone, Copy)]
pub struct Theme {
    // ── Surfaces (low → high elevation) ──
    pub crust: egui::Color32,
    pub mantle: egui::Color32,
    pub base: egui::Color32,
    pub surface0: egui::Color32,
    pub surface1: egui::Color32,
    pub surface2: egui::Color32,

    // ── Overlays ──
    pub overlay0: egui::Color32,
    pub overlay1: egui::Color32,
    pub overlay2: egui::Color32,

    // ── Text ──
    pub text: egui::Color32,
    pub subtext1: egui::Color32,
    pub subtext0: egui::Color32,

    // ── Accent colors ──
    pub blue: egui::Color32,
    pub green: egui::Color32,
    pub red: egui::Color32,
    pub yellow: egui::Color32,
    pub peach: egui::Color32,
    pub mauve: egui::Color32,
    pub teal: egui::Color32,
    pub sky: egui::Color32,
    pub lavender: egui::Color32,
    pub flamingo: egui::Color32,
    pub pink: egui::Color32,
    pub maroon: egui::Color32,
    pub rosewater: egui::Color32,

    // ── Semantic aliases ──
    pub hover_overlay: egui::Color32,
    pub active_overlay: egui::Color32,
    pub separator: egui::Color32,

    // ── UI Typography (not terminal font) ──
    pub font_size_caption: LogicalPx,
    pub font_size_body: LogicalPx,
    pub font_size_heading: LogicalPx,
    pub font_size_max: LogicalPx,

    // ── UI Sizing ──
    pub border_width: LogicalPx,
    pub corner_radius: LogicalPx,
    pub item_height_tree: LogicalPx,
    pub item_height_interactive: LogicalPx,
    pub item_height_tab: LogicalPx,
    pub tab_width: LogicalPx,

    // ── Spacing (4px grid) ──
    pub spacing_xs: LogicalPx,
    pub spacing_sm: LogicalPx,
    pub spacing_md: LogicalPx,
    pub spacing_lg: LogicalPx,
    pub spacing_xl: LogicalPx,

    // ── Terminal (float format for GPU renderer) ──
    pub terminal_fg: [f32; 4],
    pub terminal_bg: [f32; 4],
    pub selection_bg: [f32; 4],
    pub ansi_colors: [[f32; 3]; 16],
}

impl Theme {
    /// Catppuccin Mocha dark theme (const so it can initialize the global RwLock).
    pub const DARK: Self = Self {
        // Surfaces
        crust: egui::Color32::from_rgb(17, 17, 27), // #11111b
        mantle: egui::Color32::from_rgb(24, 24, 37), // #181825
        base: egui::Color32::from_rgb(30, 30, 46),  // #1e1e2e
        surface0: egui::Color32::from_rgb(49, 50, 68), // #313244
        surface1: egui::Color32::from_rgb(69, 71, 90), // #45475a
        surface2: egui::Color32::from_rgb(88, 91, 112), // #585b70

        // Overlays
        overlay0: egui::Color32::from_rgb(108, 112, 134), // #6c7086
        overlay1: egui::Color32::from_rgb(127, 132, 156), // #7f849c
        overlay2: egui::Color32::from_rgb(147, 153, 178), // #9399b2

        // Text
        text: egui::Color32::from_rgb(205, 214, 244), // #cdd6f4
        subtext1: egui::Color32::from_rgb(186, 194, 222), // #bac2de
        subtext0: egui::Color32::from_rgb(166, 173, 200), // #a6adc8

        // Accent colors
        blue: egui::Color32::from_rgb(137, 180, 250), // #89b4fa
        green: egui::Color32::from_rgb(166, 227, 161), // #a6e3a1
        red: egui::Color32::from_rgb(243, 139, 168),  // #f38ba8
        yellow: egui::Color32::from_rgb(249, 226, 175), // #f9e2af
        peach: egui::Color32::from_rgb(250, 179, 135), // #fab387
        mauve: egui::Color32::from_rgb(203, 166, 247), // #cba6f7
        teal: egui::Color32::from_rgb(148, 226, 213), // #94e2d5
        sky: egui::Color32::from_rgb(137, 220, 235),  // #89dceb
        lavender: egui::Color32::from_rgb(180, 190, 254), // #b4befe
        flamingo: egui::Color32::from_rgb(242, 205, 205), // #f2cdcd
        pink: egui::Color32::from_rgb(245, 194, 231), // #f5c2e7
        maroon: egui::Color32::from_rgb(235, 160, 172), // #eba0ac
        rosewater: egui::Color32::from_rgb(245, 224, 220), // #f5e0dc

        // Semantic (premultiplied: white at ~8% / ~12% alpha)
        hover_overlay: egui::Color32::from_rgba_premultiplied(20, 20, 20, 20),  // ~8%
        active_overlay: egui::Color32::from_rgba_premultiplied(31, 31, 31, 31), // ~12%
        separator: egui::Color32::from_rgba_premultiplied(20, 20, 20, 20),      // ~8%

        // UI Typography
        font_size_caption: LogicalPx(11.0),
        font_size_body: LogicalPx(13.0),
        font_size_heading: LogicalPx(13.0), // semibold로 구분, 크기는 같음
        font_size_max: LogicalPx(14.0),

        // UI Sizing
        border_width: LogicalPx(1.0),
        corner_radius: LogicalPx(4.0),
        item_height_tree: LogicalPx(22.0),
        item_height_interactive: LogicalPx(28.0),
        item_height_tab: LogicalPx(24.0),
        tab_width: LogicalPx(150.0),

        // Spacing (4px grid)
        spacing_xs: LogicalPx(4.0),
        spacing_sm: LogicalPx(8.0),
        spacing_md: LogicalPx(12.0),
        spacing_lg: LogicalPx(16.0),
        spacing_xl: LogicalPx(24.0),

        // Terminal (GPU float format)
        terminal_fg: [0.804, 0.839, 0.957, 1.0], // Text #cdd6f4
        terminal_bg: [0.118, 0.118, 0.180, 1.0], // Base #1e1e2e
        selection_bg: [0.345, 0.357, 0.439, 1.0], // Surface2 #585b70
        ansi_colors: [
            [0.176, 0.176, 0.271], // 0: black      (Surface1 #45475a)
            [0.953, 0.545, 0.659], // 1: red         (#f38ba8)
            [0.651, 0.890, 0.631], // 2: green       (#a6e3a1)
            [0.976, 0.886, 0.686], // 3: yellow      (#f9e2af)
            [0.537, 0.706, 0.980], // 4: blue        (#89b4fa)
            [0.796, 0.651, 0.969], // 5: magenta     (#cba6f7)
            [0.580, 0.886, 0.835], // 6: cyan        (#94e2d5)
            [0.729, 0.761, 0.882], // 7: white       (Subtext1 #bac2de)
            [0.424, 0.439, 0.537], // 8: bright black(Overlay0 #6c7086)
            [0.953, 0.545, 0.659], // 9: bright red  (#f38ba8)
            [0.651, 0.890, 0.631], // 10: bright green(#a6e3a1)
            [0.976, 0.886, 0.686], // 11: bright yellow(#f9e2af)
            [0.537, 0.706, 0.980], // 12: bright blue(#89b4fa)
            [0.796, 0.651, 0.969], // 13: bright magenta(#cba6f7)
            [0.537, 0.784, 0.922], // 14: bright cyan(Sky #89dceb)
            [0.804, 0.839, 0.957], // 15: bright white(Text #cdd6f4)
        ],
    };

    /// Catppuccin Latte light theme.
    pub const LATTE: Self = Self {
        // Surfaces
        crust: egui::Color32::from_rgb(220, 224, 232),    // #dce0e8
        mantle: egui::Color32::from_rgb(230, 233, 239),   // #e6e9ef
        base: egui::Color32::from_rgb(239, 241, 245),     // #eff1f5
        surface0: egui::Color32::from_rgb(204, 208, 218), // #ccd0da
        surface1: egui::Color32::from_rgb(188, 192, 204), // #bcc0cc
        surface2: egui::Color32::from_rgb(172, 176, 190), // #acb0be

        // Overlays
        overlay0: egui::Color32::from_rgb(156, 160, 176), // #9ca0b0
        overlay1: egui::Color32::from_rgb(140, 143, 161), // #8c8fa1
        overlay2: egui::Color32::from_rgb(124, 127, 147), // #7c7f93

        // Text
        text: egui::Color32::from_rgb(76, 79, 105),       // #4c4f69
        subtext1: egui::Color32::from_rgb(92, 95, 119),   // #5c5f77
        subtext0: egui::Color32::from_rgb(108, 111, 133), // #6c6f85

        // Accent colors
        blue: egui::Color32::from_rgb(30, 102, 245),      // #1e66f5
        green: egui::Color32::from_rgb(64, 160, 43),      // #40a02b
        red: egui::Color32::from_rgb(210, 15, 57),        // #d20f39
        yellow: egui::Color32::from_rgb(223, 142, 29),    // #df8e1d
        peach: egui::Color32::from_rgb(254, 100, 11),     // #fe640b
        mauve: egui::Color32::from_rgb(136, 57, 239),     // #8839ef
        teal: egui::Color32::from_rgb(23, 146, 153),      // #179299
        sky: egui::Color32::from_rgb(4, 165, 229),        // #04a5e5
        lavender: egui::Color32::from_rgb(114, 135, 253), // #7287fd
        flamingo: egui::Color32::from_rgb(221, 120, 120), // #dd7878
        pink: egui::Color32::from_rgb(234, 118, 203),     // #ea76cb
        maroon: egui::Color32::from_rgb(230, 69, 83),     // #e64553
        rosewater: egui::Color32::from_rgb(220, 138, 120), // #dc8a78

        // Semantic (premultiplied: black at ~8% / ~12% alpha)
        hover_overlay: egui::Color32::from_rgba_premultiplied(0, 0, 0, 20),  // ~8%
        active_overlay: egui::Color32::from_rgba_premultiplied(0, 0, 0, 31), // ~12%
        separator: egui::Color32::from_rgba_premultiplied(0, 0, 0, 20),      // ~8%

        // UI Typography (same as dark)
        font_size_caption: LogicalPx(11.0),
        font_size_body: LogicalPx(13.0),
        font_size_heading: LogicalPx(13.0),
        font_size_max: LogicalPx(14.0),

        // UI Sizing (same as dark)
        border_width: LogicalPx(1.0),
        corner_radius: LogicalPx(4.0),
        item_height_tree: LogicalPx(22.0),
        item_height_interactive: LogicalPx(28.0),
        item_height_tab: LogicalPx(24.0),
        tab_width: LogicalPx(150.0),

        // Spacing (same as dark)
        spacing_xs: LogicalPx(4.0),
        spacing_sm: LogicalPx(8.0),
        spacing_md: LogicalPx(12.0),
        spacing_lg: LogicalPx(16.0),
        spacing_xl: LogicalPx(24.0),

        // Terminal (GPU float format)
        terminal_fg: [0.298, 0.310, 0.412, 1.0], // Text #4c4f69
        terminal_bg: [0.937, 0.945, 0.961, 1.0], // Base #eff1f5
        selection_bg: [0.675, 0.686, 0.714, 1.0], // Surface0 #acb0be
        ansi_colors: [
            [0.675, 0.686, 0.714], // 0: black      (Surface1 #bcc0cc)
            [0.824, 0.059, 0.224], // 1: red         (#d20f39)
            [0.251, 0.627, 0.169], // 2: green       (#40a02b)
            [0.875, 0.557, 0.114], // 3: yellow      (#df8e1d)
            [0.118, 0.400, 0.961], // 4: blue        (#1e66f5)
            [0.533, 0.224, 0.937], // 5: magenta     (#8839ef)
            [0.090, 0.573, 0.600], // 6: cyan        (#179299)
            [0.361, 0.373, 0.467], // 7: white       (Subtext1 #5c5f77)
            [0.612, 0.627, 0.690], // 8: bright black(Overlay0 #9ca0b0)
            [0.824, 0.059, 0.224], // 9: bright red  (#d20f39)
            [0.251, 0.627, 0.169], // 10: bright green(#40a02b)
            [0.875, 0.557, 0.114], // 11: bright yellow(#df8e1d)
            [0.118, 0.400, 0.961], // 12: bright blue(#1e66f5)
            [0.533, 0.224, 0.937], // 13: bright magenta(#8839ef)
            [0.016, 0.647, 0.898], // 14: bright cyan(Sky #04a5e5)
            [0.298, 0.310, 0.412], // 15: bright white(Text #4c4f69)
        ],
    };

    /// Convert an egui Color32 to GPU float format [r, g, b, a].
    pub fn to_float(c: egui::Color32) -> [f32; 4] {
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            c.a() as f32 / 255.0,
        ]
    }

    /// Apply this theme to an egui context with UI scale factor.
    pub fn apply_to_egui(&self, ctx: &egui::Context, ui_scale: f32) {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = self.mantle;
        visuals.window_fill = self.base;
        visuals.window_stroke = egui::Stroke::new(1.0, self.surface0);
        visuals.extreme_bg_color = self.crust;
        visuals.widgets.inactive.bg_fill = self.base;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, self.surface0);
        visuals.widgets.hovered.bg_fill = self.surface0;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, self.surface1);
        visuals.widgets.active.bg_fill = self.surface1;
        visuals.override_text_color = Some(self.text);
        ctx.set_visuals(visuals);

        // Apply scaled UI text sizes and spacing
        let mut style = (*ctx.style()).clone();
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::proportional((self.font_size_body.value() * ui_scale).round_ui()),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::proportional((self.font_size_caption.value() * ui_scale).round_ui()),
        );
        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::proportional(
                (self.font_size_heading.value() * ui_scale * 1.15).round_ui(),
            ),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::proportional((self.font_size_body.value() * ui_scale).round_ui()),
        );
        style.text_styles.insert(
            egui::TextStyle::Monospace,
            egui::FontId::monospace((self.font_size_body.value() * ui_scale).round_ui()),
        );
        style.spacing.item_spacing = egui::vec2(
            (self.spacing_sm.value() * ui_scale).round_ui(),
            (self.spacing_xs.value() * ui_scale).round_ui(),
        );
        style.spacing.button_padding = egui::vec2(
            (self.spacing_sm.value() * ui_scale).round_ui(),
            (self.spacing_xs.value() * ui_scale).round_ui(),
        );
        ctx.set_style(style);
    }
}

/// A theme preset: UI theme + default surface colors for each surface type.
pub struct ThemePreset {
    pub id: &'static str,
    pub label: &'static str,
    pub theme: Theme,
    pub terminal_colors: crate::color::SurfaceColors,
    pub markdown_colors: crate::color::SurfaceColors,
    pub explorer_colors: crate::color::SurfaceColors,
}

/// List all available theme presets.
pub fn presets() -> Vec<ThemePreset> {
    use crate::color::{HexColor, SurfaceColors};

    vec![
        ThemePreset {
            id: "catppuccin-mocha",
            label: "Catppuccin Mocha",
            theme: Theme::DARK,
            terminal_colors: SurfaceColors::terminal_default(),
            markdown_colors: SurfaceColors::markdown_default(),
            explorer_colors: SurfaceColors::explorer_default(),
        },
        ThemePreset {
            id: "catppuccin-latte",
            label: "Catppuccin Latte",
            theme: Theme::LATTE,
            terminal_colors: SurfaceColors {
                focused_bg: HexColor::from_rgb(255, 255, 255),  // #ffffff
                focused_fg: HexColor::from_rgb(76, 79, 105),    // #4c4f69
                unfocused_bg: HexColor::from_rgb(230, 233, 239), // #e6e9ef
                unfocused_fg: HexColor::from_rgb(108, 111, 133), // #6c6f85
            },
            markdown_colors: SurfaceColors {
                focused_bg: HexColor::from_rgb(255, 255, 255),  // #ffffff
                focused_fg: HexColor::from_rgb(76, 79, 105),    // #4c4f69
                unfocused_bg: HexColor::from_rgb(230, 233, 239), // #e6e9ef
                unfocused_fg: HexColor::from_rgb(108, 111, 133), // #6c6f85
            },
            explorer_colors: SurfaceColors {
                focused_bg: HexColor::from_rgb(255, 255, 255),  // #ffffff
                focused_fg: HexColor::from_rgb(76, 79, 105),    // #4c4f69
                unfocused_bg: HexColor::from_rgb(230, 233, 239), // #e6e9ef
                unfocused_fg: HexColor::from_rgb(108, 111, 133), // #6c6f85
            },
        },
    ]
}

/// Global theme instance. Mutable at runtime via `set_theme()`.
static THEME: std::sync::RwLock<Theme> = std::sync::RwLock::new(Theme::DARK);

/// Get the current theme (read lock).
pub fn theme() -> std::sync::RwLockReadGuard<'static, Theme> {
    THEME.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replace the current theme at runtime.
pub fn set_theme(new_theme: Theme) {
    let mut guard = THEME
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = new_theme;
}
