//! `Hint text` specimen — 디자인(4) `components/text/Hint text` 카드.
//!
//! 입력/컨트롤 *아래* 에 붙는 보조 설명 텍스트. 작은 크기(caption) · text-muted ·
//! sentence case · 줄높이 1.5 · 대상 바로 아래. 본체의 placeholder 도 같은 muted
//! 색을 쓴다(`tasty_egui_theme::hint_text`).

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

            // 1) 라벨 + mono 입력 + 힌트 (Remote tasty path).
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

            // 2) 라벨 + 힌트 (Reduced motion, 입력 없음).
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
                field_label(ui, theme, "Reduced motion");
                hint_line(
                    ui,
                    theme,
                    "Disables the terminal cursor blink and spinner animation.",
                );
            });

            // 3) 인라인 Kbd 가 섞인 다이얼로그 힌트 한 줄.
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
