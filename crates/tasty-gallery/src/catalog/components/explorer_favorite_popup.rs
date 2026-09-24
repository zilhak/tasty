//! 즐겨찾기 추가 팝업 예제. 본체는 rename 팝업의 ExplorerAddFavorite 대상으로 열며
//! 윈도우 중앙에 표시한다. 공용 프레임을 사용하고 입력은 정적으로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let width = theme.measure_sm; // ≈300 (narrow column, design w≈280)
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, width, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                kit::title(ui, theme, "Add to favorites");
                kit::caption(ui, theme, "Path: ~/Downloads", false);
                kit::field(ui, theme, None, "Downloads", false, false);
                ui.add_space(theme.spacing_xs.value());
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                        Button::new("Add")
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
            ("frame", "≈280 · bg-panel · 1px border-strong"),
            ("title", "heading semibold"),
            ("path", "caption · text-muted"),
            ("input", "28 · surface-raised · seeded folder name"),
            ("footer", "right — Cancel · Add (primary)"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new("surface-raised", "input", theme.surface_raised().to_egui()),
            TokenChip::new(
                "accent-primary",
                "Add button",
                theme.accent_primary().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "The host opens the shared Rename popup in window scope. Title, initial value and \
         primary button label differ. Favorites are global.",
    );
}
