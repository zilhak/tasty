//! Settings window — 디자인(4) Overlays `settings` Spec (신규).
//!
//! 620×380 2-tier 모달. L1 top 탭(활성 2px accent underline, bg-sidebar) + close ·
//! L2 sidebar(width 168, filter + 섹션 리스트, selected surface-active) ·
//! content(Theme preset grid 2col + 스와치 행) · footer(Cancel/Save).

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, IconButton, IconButtonVariant};

use crate::catalog::icons;
use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: f32 = 620.0;
const L2_WIDTH: f32 = 168.0;
const MID_HEIGHT: f32 = 248.0;

const L1_TABS: &[&str] = &["Appearance", "Keybindings", "Plugins", "Advanced"];
const L2_SECTIONS: &[&str] = &["Theme", "Typography", "Cursor", "Window", "Display"];
const PRESETS: &[(&str, bool)] = &[("Mocha", true), ("Latte", false), ("Macchiato", false), ("Frappé", false)];

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            // L1 top 탭 (bg-sidebar).
            egui::Frame::new()
                .fill(theme.bg_sidebar().to_egui())
                .inner_margin(egui::Margin::symmetric(theme.spacing_md.value() as i8, 0))
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
                        for (i, t) in L1_TABS.iter().enumerate() {
                            l1_tab(ui, theme, t, i == 0);
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            IconButton::new()
                                .variant(IconButtonVariant::Ghost)
                                .show(ui, theme, &|ui, rect, c| {
                                    icons::CLOSE.image(rect.height(), c).paint_at(ui, rect)
                                });
                        });
                    });
                });
            kit::hsep(ui, theme);

            // 중단 — L2 sidebar + content. content 폭은 명시 계산(측정 패스에서
            // available_width 가 0 이 되면 음수 폭 할당으로 패닉하므로 상수 기반).
            let content_w = (WIDTH
                - L2_WIDTH
                - theme.spacing_sm.value() * 2.0
                - theme.spacing_md.value() * 2.0
                - theme.border_width.value())
            .max(theme.measure_sm.value());
            ui.horizontal_top(|ui| {
                // L2 sidebar.
                egui::Frame::new()
                    .fill(theme.bg_sidebar().to_egui())
                    .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
                    .show(ui, |ui| {
                        ui.set_width(L2_WIDTH);
                        ui.set_min_height(MID_HEIGHT);
                        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                        kit::field(ui, theme, None, "Filter settings…", true, false);
                        ui.add_space(theme.spacing_xs.value());
                        for (i, s) in L2_SECTIONS.iter().enumerate() {
                            l2_item(ui, theme, s, i == 0);
                        }
                    });
                // separator + content.
                let (r, _) = ui.allocate_exact_size(egui::vec2(theme.border_width.value(), MID_HEIGHT), egui::Sense::hover());
                ui.painter().vline(r.center().x, r.y_range(), egui::Stroke::new(theme.border_width.value(), theme.separator.to_egui()));
                egui::Frame::new()
                    .inner_margin(egui::Margin::same(theme.spacing_lg.value() as i8))
                    .show(ui, |ui| {
                        ui.set_min_width(content_w);
                        ui.set_min_height(MID_HEIGHT);
                        ui.spacing_mut().item_spacing.y = theme.spacing_md.value();
                        ui.label(
                            egui::RichText::new("Theme")
                                .size(theme.font_size_max.value())
                                .strong()
                                .color(theme.text_primary().to_egui()),
                        );
                        // preset grid 2col — content 내부 폭(content_w - 좌우 lg margin) 기반.
                        let inner = content_w - theme.spacing_lg.value() * 2.0;
                        let cw = ((inner - theme.spacing_md.value()) * 0.5)
                            .max(theme.spacing_xl.value());
                        for pair in PRESETS.chunks(2) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = theme.spacing_md.value();
                                for (name, on) in pair {
                                    swatch_row(ui, theme, cw, name, *on);
                                }
                            });
                        }
                    });
            });
            kit::hsep(ui, theme);

            // footer.
            kit::region_sym(ui, theme.spacing_md.value(), theme.spacing_sm.value(), |ui| {
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Save").variant(ButtonVariant::Primary).show(ui, theme);
                        Button::new("Cancel").variant(ButtonVariant::Ghost).show(ui, theme);
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "620×380 · bg-panel"),
            ("L1", "top tabs · height 44 · active 2px accent underline"),
            ("L2", "sidebar 168 · filter + sections · selected surface-active"),
            ("content", "padding 18 · preset grid 2col · swatch row 34"),
            ("footer", "Cancel · Save"),
        ],
        &[
            TokenChip::new("bg-sidebar", "L1 + L2", theme.bg_sidebar().to_egui()),
            TokenChip::new("bg-panel", "content", theme.bg_panel().to_egui()),
            TokenChip::new("accent-primary", "active tab", theme.accent_primary().to_egui()),
            TokenChip::new("surface-active", "selected section", theme.surface_active().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "Two tiers: L1 tabs pick a domain, L2 the section within it, and content \
         shows the controls. The same two-depth idiom backs the Layouts page.",
    );
}

fn l1_tab(ui: &mut egui::Ui, theme: &Theme, label: &str, active: bool) {
    let h = theme.titlebar_height.value() + theme.spacing_sm.value();
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        egui::Color32::PLACEHOLDER,
    );
    let pad = theme.spacing_md.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(galley.rect.width() + pad * 2.0, h), egui::Sense::hover());
    let fg = if active { theme.text_primary() } else { theme.text_muted() };
    ui.painter().galley(rect.center() - galley.rect.size() * 0.5, galley, fg.to_egui());
    if active {
        let bar = egui::Rect::from_min_size(
            egui::pos2(rect.left(), rect.bottom() - theme.tab_indicator_width.value()),
            egui::vec2(rect.width(), theme.tab_indicator_width.value()),
        );
        ui.painter().rect_filled(bar, 0.0, theme.accent_primary().to_egui());
    }
}

fn l2_item(ui: &mut egui::Ui, theme: &Theme, label: &str, active: bool) {
    let h = theme.item_height_interactive.value();
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
    if active {
        ui.painter().rect_filled(rect, theme.corner_radius_sm.value(), theme.surface_active().to_egui());
    }
    let fg = if active { theme.text_primary() } else { theme.text_secondary() };
    ui.painter().text(
        egui::pos2(rect.left() + theme.spacing_sm.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(theme.font_size_body.value()),
        fg.to_egui(),
    );
}

fn swatch_row(ui: &mut egui::Ui, theme: &Theme, width: f32, name: &str, selected: bool) {
    let h = theme.item_height_interactive.value() + theme.spacing_xs.value();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, h), egui::Sense::hover());
    let border = if selected { theme.accent_primary() } else { theme.border_default() };
    let bw = if selected { theme.focus_ring_width.value() } else { theme.border_width.value() };
    ui.painter().rect_filled(rect, theme.corner_radius_sm.value(), theme.surface_raised().to_egui());
    ui.painter().rect_stroke(rect, theme.corner_radius_sm.value(), egui::Stroke::new(bw, border.to_egui()), egui::StrokeKind::Inside);
    // swatch.
    let s = theme.icon_glyph_size_sm.value();
    let sw = egui::Rect::from_center_size(
        egui::pos2(rect.left() + theme.spacing_sm.value() + s * 0.5, rect.center().y),
        egui::vec2(s, s),
    );
    ui.painter().rect_filled(sw, theme.corner_radius_sm.value(), theme.accent_primary().to_egui());
    ui.painter().text(
        egui::pos2(sw.right() + theme.spacing_sm.value(), rect.center().y),
        egui::Align2::LEFT_CENTER,
        name,
        egui::FontId::proportional(theme.font_size_body.value()),
        theme.text_primary().to_egui(),
    );
}
