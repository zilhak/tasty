//! Rename popup — 디자인(4) Overlays `rename` Spec.
//!
//! 360px 모달. title + 키 힌트(↵/Esc) + autofocus Input(block) + Cancel/Rename.
//! workspace/tab/subtitle rename 의 단일 view — 차이는 제목·버퍼뿐이다(디자인은
//! rename 1 Spec, 기존 widgets/dialog 와 통합).

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(360.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                ui.horizontal(|ui| {
                    kit::title(ui, theme, "Rename workspace");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        kit::caption(ui, theme, "Press ↵ to confirm, Esc to cancel.", false);
                    });
                });
                // autofocus Input (block) — gallery 는 정적 값으로 표시(focus 경합 회피).
                kit::field(ui, theme, None, "tasty-core", false, false);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Rename")
                            .variant(ButtonVariant::Primary)
                            .show(ui, theme);
                        Button::new("Cancel")
                            .variant(ButtonVariant::Ghost)
                            .show(ui, theme);
                    });
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "360px · bg-panel"),
            ("title", "14px semibold"),
            ("hint", "↵ rename · Esc cancel"),
            ("input", "block · autofocused + selected"),
            ("footer", "Cancel · Rename"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new("surface-raised", "input", theme.surface_raised().to_egui()),
            TokenChip::new("border-focus", "focus ring", theme.border_focus().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "One view serves workspace, subtitle, and tab renames — only the title and \
         buffer differ. The field autofocuses with the text selected; Enter commits.",
    );
}
