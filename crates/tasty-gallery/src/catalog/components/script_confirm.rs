//! 등록 후 내용이 바뀐 Lua 스크립트의 실행 확인 예제.
//! 본체 뷰를 직접 호출하지 않고 같은 순서와 Theme 값으로 그린다.

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{Button, ButtonVariant, TagVariant, tag};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

/// `popup/defs.rs` 의 `script_changed_confirm` 기본 폭.
const POPUP_WIDTH: LogicalPx = LogicalPx(360.0);

fn card(ui: &mut egui::Ui, theme: &Theme, name: &str) {
    kit::frame_card(ui, theme, POPUP_WIDTH, kit::panel_fill(theme), |ui| {
        kit::region_sym(ui, theme.spacing_md, theme.spacing_md, |ui| {
            ui.spacing_mut().item_spacing.y = theme.spacing_sm.value();

            // 본체 제목은 kit::title과 다른 font_size_body를 사용한다.
            ui.label(
                egui::RichText::new("Script changed since registration")
                    .size(theme.font_size_body.value())
                    .strong()
                    .color(theme.text_primary().to_egui()),
            );

            ui.add(
                egui::Label::new(
                    egui::RichText::new(name)
                        .size(theme.font_size_caption.value())
                        .family(egui::FontFamily::Monospace)
                        .color(theme.text_muted().to_egui()),
                )
                .truncate(),
            );

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
                tag(ui, theme, "changed", TagVariant::Warning, false);
                ui.label(
                    egui::RichText::new(
                        "Review it, then run the new version. Its recorded hash will be updated.",
                    )
                    .size(theme.font_size_caption.value())
                    .color(theme.text_secondary().to_egui()),
                );
            });

            ui.add_space(theme.spacing_xs.value());

            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    Button::new("Run anyway")
                        .variant(ButtonVariant::Primary)
                        .show(ui, theme);
                    Button::new("Cancel")
                        .variant(ButtonVariant::Ghost)
                        .show(ui, theme);
                });
            });
        });
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        spec::cluster(ui, theme, "Changed script", |ui| {
            card(ui, theme, "~/.tasty/scripts/reload-panes.lua")
        });
        spec::cluster(ui, theme, "Long path (truncate)", |ui| {
            card(
                ui,
                theme,
                "~/.tasty/scripts/very/deeply/nested/path/that/overflows/the-card.lua",
            )
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("frame", "360px · bg-panel · popup 기본 크기 360×150"),
            ("name", "font-size-caption mono text-muted · truncate"),
            ("warning", "tag(Warning) + caption text-secondary"),
            ("footer", "Run anyway(Primary) / Cancel(Ghost) · 우측정렬"),
        ],
        &[
            TokenChip::new("bg-panel", "frame", theme.bg_panel().to_egui()),
            TokenChip::new(
                "accent-warning",
                "changed tag",
                theme.accent_warning().to_egui(),
            ),
            TokenChip::new("text-muted", "script path", theme.text_muted().to_egui()),
        ],
    );

    spec::note(
        ui,
        theme,
        "등록 시점 해시와 현재 파일 해시가 다를 때만 뜬다. Run anyway 로 확정해야 해시가 \
         갱신·영속되고 실행된다 — Escape·Cancel·X 는 모두 실행하지 않는다.",
    );
}
