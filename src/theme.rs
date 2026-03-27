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
    pub font_size_caption: f32,
    pub font_size_body: f32,
    pub font_size_heading: f32,
    pub font_size_max: f32,

    // ── UI Sizing ──
    pub border_width: f32,
    pub corner_radius: f32,
    pub item_height_tree: f32,
    pub item_height_interactive: f32,
    pub item_height_tab: f32,

    // ── Spacing (4px grid) ──
    pub spacing_xs: f32,
    pub spacing_sm: f32,
    pub spacing_md: f32,
    pub spacing_lg: f32,
    pub spacing_xl: f32,

    // ── Terminal (float format for GPU renderer) ──
    pub terminal_fg: [f32; 4],
    pub terminal_bg: [f32; 4],
    pub ansi_colors: [[f32; 3]; 16],
}

impl Theme {
    /// Catppuccin Mocha dark theme.
    pub fn dark() -> Self {
        Self {
            // Surfaces
            crust:    egui::Color32::from_rgb(17, 17, 27),    // #11111b
            mantle:   egui::Color32::from_rgb(24, 24, 37),    // #181825
            base:     egui::Color32::from_rgb(30, 30, 46),    // #1e1e2e
            surface0: egui::Color32::from_rgb(49, 50, 68),    // #313244
            surface1: egui::Color32::from_rgb(69, 71, 90),    // #45475a
            surface2: egui::Color32::from_rgb(88, 91, 112),   // #585b70

            // Overlays
            overlay0: egui::Color32::from_rgb(108, 112, 134), // #6c7086
            overlay1: egui::Color32::from_rgb(127, 132, 156), // #7f849c
            overlay2: egui::Color32::from_rgb(147, 153, 178), // #9399b2

            // Text
            text:     egui::Color32::from_rgb(205, 214, 244), // #cdd6f4
            subtext1: egui::Color32::from_rgb(186, 194, 222), // #bac2de
            subtext0: egui::Color32::from_rgb(166, 173, 200), // #a6adc8

            // Accent colors
            blue:      egui::Color32::from_rgb(137, 180, 250), // #89b4fa
            green:     egui::Color32::from_rgb(166, 227, 161), // #a6e3a1
            red:       egui::Color32::from_rgb(243, 139, 168), // #f38ba8
            yellow:    egui::Color32::from_rgb(249, 226, 175), // #f9e2af
            peach:     egui::Color32::from_rgb(250, 179, 135), // #fab387
            mauve:     egui::Color32::from_rgb(203, 166, 247), // #cba6f7
            teal:      egui::Color32::from_rgb(148, 226, 213), // #94e2d5
            sky:       egui::Color32::from_rgb(137, 220, 235), // #89dceb
            lavender:  egui::Color32::from_rgb(180, 190, 254), // #b4befe
            flamingo:  egui::Color32::from_rgb(242, 205, 205), // #f2cdcd
            pink:      egui::Color32::from_rgb(245, 194, 231), // #f5c2e7
            maroon:    egui::Color32::from_rgb(235, 160, 172), // #eba0ac
            rosewater: egui::Color32::from_rgb(245, 224, 220), // #f5e0dc

            // Semantic
            hover_overlay:  egui::Color32::from_rgba_premultiplied(255, 255, 255, 20), // ~8%
            active_overlay: egui::Color32::from_rgba_premultiplied(255, 255, 255, 31), // ~12%
            separator:      egui::Color32::from_rgba_premultiplied(255, 255, 255, 20), // ~8%

            // UI Typography
            font_size_caption: 11.0,
            font_size_body: 13.0,
            font_size_heading: 13.0,  // semibold로 구분, 크기는 같음
            font_size_max: 14.0,

            // UI Sizing
            border_width: 1.0,
            corner_radius: 4.0,
            item_height_tree: 22.0,
            item_height_interactive: 28.0,
            item_height_tab: 35.0,

            // Spacing (4px grid)
            spacing_xs: 4.0,
            spacing_sm: 8.0,
            spacing_md: 12.0,
            spacing_lg: 16.0,
            spacing_xl: 24.0,

            // Terminal (GPU float format)
            terminal_fg: [0.804, 0.839, 0.957, 1.0], // Text #cdd6f4
            terminal_bg: [0.118, 0.118, 0.180, 1.0], // Base #1e1e2e
            ansi_colors: [
                [0.176, 0.176, 0.271],  // 0: black      (Surface1 #45475a)
                [0.953, 0.545, 0.659],  // 1: red         (#f38ba8)
                [0.651, 0.890, 0.631],  // 2: green       (#a6e3a1)
                [0.976, 0.886, 0.686],  // 3: yellow      (#f9e2af)
                [0.537, 0.706, 0.980],  // 4: blue        (#89b4fa)
                [0.796, 0.651, 0.969],  // 5: magenta     (#cba6f7)
                [0.580, 0.886, 0.835],  // 6: cyan        (#94e2d5)
                [0.729, 0.761, 0.882],  // 7: white       (Subtext1 #bac2de)
                [0.424, 0.439, 0.537],  // 8: bright black(Overlay0 #6c7086)
                [0.953, 0.545, 0.659],  // 9: bright red  (#f38ba8)
                [0.651, 0.890, 0.631],  // 10: bright green(#a6e3a1)
                [0.976, 0.886, 0.686],  // 11: bright yellow(#f9e2af)
                [0.537, 0.706, 0.980],  // 12: bright blue(#89b4fa)
                [0.796, 0.651, 0.969],  // 13: bright magenta(#cba6f7)
                [0.537, 0.784, 0.922],  // 14: bright cyan(Sky #89dceb)
                [0.804, 0.839, 0.957],  // 15: bright white(Text #cdd6f4)
            ],
        }
    }

    /// Convert an egui Color32 to GPU float format [r, g, b, a].
    pub fn to_float(c: egui::Color32) -> [f32; 4] {
        [
            c.r() as f32 / 255.0,
            c.g() as f32 / 255.0,
            c.b() as f32 / 255.0,
            c.a() as f32 / 255.0,
        ]
    }

    /// Apply this theme to an egui context.
    pub fn apply_to_egui(&self, ctx: &egui::Context) {
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
    }
}

/// Global theme instance. Currently always dark.
static THEME: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();

/// Get the current theme.
pub fn theme() -> &'static Theme {
    THEME.get_or_init(Theme::dark)
}
