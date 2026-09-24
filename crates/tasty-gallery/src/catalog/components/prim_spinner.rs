//! Spinner의 크기·색·동작 줄이기 예제. 기본 크기는 위젯과 같은 icon-size-md에서 읽는다.

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Button, ButtonVariant, Spinner};

use crate::catalog::spec::{StageVariant, TokenChip, cluster, meta, stage};

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    let base = theme.icon_glyph_size_md.value();
    stage(ui, theme, StageVariant::Column, |ui| {
        cluster(ui, theme, "sizes — 12 · 16 · 20 · 24", |ui| {
            Spinner::new().size(12.0).show(ui, theme);
            Spinner::new().size(base).show(ui, theme);
            Spinner::new().size(20.0).show(ui, theme);
            Spinner::new().size(24.0).show(ui, theme);
        });
        cluster(ui, theme, "inline with text", |ui| {
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            Spinner::new().size(14.0).show(ui, theme);
            ui.label(
                egui::RichText::new("Collecting…")
                    .size(theme.font_size_body.value())
                    .color(egui::Color32::from(theme.text_muted())),
            );
        });
        cluster(ui, theme, "in a button · detecting row", |ui| {
            // Button.leading_icon은 정적 아이콘만 받으므로 스피너를 버튼 앞에 별도로 그린다.
            Spinner::new().size(14.0).show(ui, theme);
            Button::new("Installing…")
                .variant(ButtonVariant::Secondary)
                .enabled(false)
                .show(ui, theme);
            ui.add_space(theme.spacing_md.value());
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            Spinner::new().size(12.0).show(ui, theme);
            ui.label(
                egui::RichText::new("detecting shell…")
                    .size(theme.font_size_caption.value())
                    .monospace()
                    .color(egui::Color32::from(theme.text_muted())),
            );
        });
        cluster(ui, theme, "reduced motion — 3 static dots", |ui| {
            // 사용자 설정과 무관하게 두 상태를 비교하려고 이 예제만 동작 줄이기를 강제한다.
            ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
            Spinner::new()
                .reduced_motion(false)
                .size(base)
                .show(ui, theme);
            Spinner::new()
                .reduced_motion(true)
                .size(base)
                .show(ui, theme);
        });
    });

    meta(
        ui,
        theme,
        &[
            ("sizes", "12 / 16 / 20 / 24px"),
            ("stroke", "2px arc + faint track"),
            ("spin", "0.9s linear"),
            ("color", "currentColor"),
            ("reduced motion", "→ 3 static dots"),
        ],
        &[
            TokenChip::new(
                "text-muted",
                "default color",
                egui::Color32::from(theme.text_muted()),
            ),
            TokenChip::new(
                "icon-size-md",
                "default 16",
                egui::Color32::from(theme.accent_primary()),
            ),
        ],
    );
}
