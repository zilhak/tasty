//! 컨트롤 아래의 보조 설명과 키보드 힌트 예제.
//! 보조 설명은 text-muted를 사용하며 입력 필드 안의 placeholder와 구분한다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Input, kbd};

use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

thread_local! {
    static BUF: RefCell<String> = const { RefCell::new(String::new()) };
}

/// 상단 라벨 (text-primary, body) — 힌트가 설명하는 대상.
fn field_label(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_body.value())
            .color(egui::Color32::from(theme.text_primary())),
    );
}

/// hint 한 줄 (caption, text-muted, line-height 1.5).
fn hint_line(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(theme.font_size_caption.value())
            .color(egui::Color32::from(theme.text_muted())),
    );
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    BUF.with(|b| {
        let mut buf = b.borrow_mut();
        stage(ui, theme, StageVariant::Column, |ui| {
            ui.set_max_width(theme.measure_md.value());
            ui.spacing_mut().item_spacing.y = theme.spacing_md.value();

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                field_label(ui, theme, "Remote tasty path");
                Input::new()
                    .mono(true)
                    .placeholder("/usr/local/bin/tasty")
                    .width(theme.measure_md.value())
                    .show(ui, theme, &mut buf);
                hint_line(ui, theme, "Leave empty to auto-detect on first connect.");
            });

            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                field_label(ui, theme, "Reduced motion");
                hint_line(
                    ui,
                    theme,
                    "Disables the terminal cursor blink and spinner animation.",
                );
            });

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = theme.spacing_xs.value();
                hint_line(ui, theme, "Press");
                kbd(ui, theme, "↵");
                hint_line(ui, theme, "to confirm,");
                kbd(ui, theme, "Esc");
                hint_line(ui, theme, "to cancel.");
            });
        });
    });

    meta(
        ui,
        theme,
        &[
            ("size", "11–12px"),
            ("color", "text-muted"),
            ("case", "sentence case"),
            ("line-height", "1.5"),
            ("placement", "below the thing it explains"),
        ],
        &[
            TokenChip::new(
                "text-muted",
                "hint color",
                egui::Color32::from(theme.text_muted()),
            ),
            TokenChip::new(
                "font-size-caption",
                "hint size",
                egui::Color32::from(theme.text_muted()),
            ),
            TokenChip::new(
                "font-mono",
                "id hint",
                egui::Color32::from(theme.text_secondary()),
            ),
        ],
    );
}
