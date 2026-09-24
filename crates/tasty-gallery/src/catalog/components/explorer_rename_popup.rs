//! 탐색기 파일·폴더 이름 변경 팝업 예제. 본체는 rename 팝업을 해당 서피스 중앙에 연다.
//! 즐겨찾기 추가와 프레임을 공유하고 제목·경로·초기 파일명·확인 버튼만 다르다.

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
                kit::title(ui, theme, "Rename");
                kit::caption(ui, theme, "Path: ~/Downloads/photo.png", false);
                kit::field(ui, theme, None, "photo.png", false, false);
                ui.add_space(theme.spacing_xs.value());
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
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
            ("frame", "≈280 · bg-panel · 1px border-strong"),
            ("title", "heading semibold"),
            ("path", "caption · text-muted"),
            ("input", "28 · surface-raised · seeded current name"),
            ("footer", "right — Cancel · Rename (primary)"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new("surface-raised", "input", theme.surface_raised().to_egui()),
            TokenChip::new(
                "accent-primary",
                "Rename button",
                theme.accent_primary().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "The host opens this popup in the selected surface scope. It shares the Add-to-favorites \
         frame, with a different title, current file name including its extension, and button label.",
    );
}
