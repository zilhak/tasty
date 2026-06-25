//! Foundations Spacing specimen — 디자인(4) "Spacing — the 4px grid, in use".
//!
//! Spec "Five steps, each with a job". 추상 갭이 아니라 **실사용 데모**로 다섯 스텝을
//! 보여준다: xs(4 chip gap) · sm(8 button pair) · md(12 card padding) · lg(16 column)
//! · xl(24 region). 모든 치수는 4 의 배수이고 spacing 토큰에서만 가져온다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{badge, BadgeVariant, Button, ButtonVariant};

use crate::catalog::spec::{meta, note, stage, StageVariant, TokenChip};

#[inline]
fn ec(c: impl Into<egui::Color32>) -> egui::Color32 {
    c.into()
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    stage(ui, theme, StageVariant::Column, |ui| {
        // xs(4) — 촘촘한 chip 묶음 사이 gap.
        use_row(ui, theme, "space-xs", "4 · chip gap", |ui, theme| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
            badge(ui, theme, "3", BadgeVariant::Danger);
            badge(ui, theme, "new", BadgeVariant::Agent);
            badge(ui, theme, "ok", BadgeVariant::Success);
        });
        // sm(8) — 버튼 쌍 사이 gap.
        use_row(ui, theme, "space-sm", "8 · button pair", |ui, theme| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            Button::new("Cancel").variant(ButtonVariant::Secondary).show(ui, theme);
            Button::new("Confirm").show(ui, theme);
        });
        // md(12) — 카드 내부 padding.
        use_row(ui, theme, "space-md", "12 · card padding", |ui, theme| {
            egui::Frame::new()
                .fill(ec(theme.surface_raised()))
                .stroke(egui::Stroke::new(theme.border_width.value(), ec(theme.border_default())))
                .corner_radius(theme.corner_radius.value())
                .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Card body")
                            .size(theme.font_size_body.value())
                            .color(ec(theme.text_secondary())),
                    );
                });
        });
        // lg(16) — 컬럼 사이 gap.
        use_row(ui, theme, "space-lg", "16 · column gap", |ui, theme| {
            ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
            for label in ["Column A", "Column B"] {
                col_block(ui, theme, label);
            }
        });
        // xl(24) — region 분리 gap.
        use_row(ui, theme, "space-xl", "24 · region gap", |ui, theme| {
            ui.spacing_mut().item_spacing.x = theme.spacing_xl.value();
            for label in ["Region 1", "Region 2"] {
                region_block(ui, theme, label);
            }
        });
    });

    meta(
        ui,
        theme,
        &[
            ("grid", "4 / 8 / 12 / 16 / 24"),
            ("rule", "multiples of 4px only"),
            ("heights", "control heights snap to the grid"),
        ],
        &[
            TokenChip::new("space-xs", "4 · tight", ec(theme.accent_primary())),
            TokenChip::new("space-sm", "8 · pair", ec(theme.accent_primary())),
            TokenChip::new("space-md", "12 · padding", ec(theme.accent_primary())),
            TokenChip::new("space-lg", "16 · column", ec(theme.accent_primary())),
            TokenChip::new("space-xl", "24 · region", ec(theme.accent_primary())),
        ],
    );
    note(ui, theme, "모든 간격·높이는 4px 그리드의 배수 — 어긋난 값은 디자인 결함이다.");
}

/// SpaceUse 한 행 — [150px 라벨(토큰 + 용도)] [실사용 데모].
fn use_row(ui: &mut egui::Ui, theme: &Theme, tok: &str, role: &str, demo: impl FnOnce(&mut egui::Ui, &Theme)) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme.spacing_lg.value();
        ui.allocate_ui(egui::vec2(theme.tab_width.value(), theme.item_height_interactive.value()), |ui| {
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new(tok)
                        .monospace()
                        .size(theme.font_size_micro.value())
                        .color(ec(theme.text_primary())),
                );
                ui.label(
                    egui::RichText::new(role)
                        .size(theme.font_size_micro.value())
                        .color(ec(theme.text_muted())),
                );
            });
        });
        ui.horizontal(|ui| demo(ui, theme));
    });
}

fn col_block(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    egui::Frame::new()
        .fill(ec(theme.bg_panel()))
        .stroke(egui::Stroke::new(theme.border_width.value(), ec(theme.separator)))
        .corner_radius(theme.corner_radius_sm.value())
        .inner_margin(egui::Margin::same(theme.spacing_sm.value() as i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(theme.font_size_caption.value())
                    .color(ec(theme.text_secondary())),
            );
        });
}

fn region_block(ui: &mut egui::Ui, theme: &Theme, label: &str) {
    egui::Frame::new()
        .fill(ec(theme.surface_raised()))
        .corner_radius(theme.corner_radius.value())
        .inner_margin(egui::Margin::same(theme.spacing_md.value() as i8))
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(label)
                    .size(theme.font_size_caption.value())
                    .color(ec(theme.text_muted())),
            );
        });
}
