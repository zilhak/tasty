//! Markdown large-file confirm — 디자인(4) Overlays `MdLargeFilePopup` Spec.
//!
//! 360px 모달. title + 파일 경로 mono; 경고 태그(크기) + 안내문 한 줄; Cancel / Open.
//! 대용량 확인 팝업은 **plugin 소유**(`crates/tasty-plugin-markdown` 의 `draw_confirm`)
//! 이며 이 specimen 은 그 plugin 컴포넌트의 디자인 SoT 다 — 토큰·구조 정합.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, TagVariant, tag};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

const WIDTH: LogicalPx = LogicalPx(360.0);

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();
                kit::title(ui, theme, "Open large file?");
                kit::caption(ui, theme, ".../docs/big-notes.md", true);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                    tag(ui, theme, "3.2 MB", TagVariant::Warning, false);
                    ui.label(
                        egui::RichText::new("Over 1 MB — rendering may be slow.")
                            .size(theme.font_size_caption.value())
                            .color(theme.text_secondary().to_egui()),
                    );
                });
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        Button::new("Open")
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
            ("path", "mono caption · muted · ellipsis"),
            ("size", "Tag warning · accent-warning"),
            ("note", "caption · text-secondary"),
            ("footer", "Cancel (ghost) · Open (primary)"),
        ],
        &[
            TokenChip::new(
                "accent-warning",
                "size tag",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new("text-secondary", "note", theme.text_secondary().to_egui()),
            TokenChip::new("bg-panel", "frame fill", theme.bg_panel().to_egui()),
        ],
    );
}
