//! 워크스페이스·탭·부제 이름 변경 팝업의 정적 예제.

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
                // 예제끼리 포커스를 다투지 않도록 입력값은 정적으로 그린다.
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
