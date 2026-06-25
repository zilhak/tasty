//! `Hint text` specimen — 디자인(4) `components/text/Hint text` 카드.
//!
//! 입력/컨트롤 *아래* 에 붙는 보조 설명 텍스트. 작은 크기(caption) · text-muted ·
//! sentence case · 줄높이 1.5 · 대상 바로 아래. 본체의 placeholder 도 같은 muted
//! 색을 쓴다(`tasty_egui_theme::hint_text`).

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::Input;

use crate::catalog::spec::{StageVariant, TokenChip, meta, stage};

thread_local! {
    static BUFS: RefCell<[String; 2]> = const { RefCell::new([String::new(), String::new()]) };
}

/// field + 그 아래 hint 한 줄.
fn field_with_hint(
    ui: &mut egui::Ui,
    theme: &Theme,
    buf: &mut String,
    placeholder: &str,
    hint: &str,
) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = theme.spacing_xs.value();
        Input::new()
            .placeholder(placeholder)
            .width(theme.measure_sm.value())
            .show(ui, theme, buf);
        ui.label(
            egui::RichText::new(hint)
                .size(theme.font_size_caption.value())
                .color(egui::Color32::from(theme.text_muted())),
        );
    });
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    BUFS.with(|b| {
        let mut bufs = b.borrow_mut();
        stage(ui, theme, StageVariant::Column, |ui| {
            ui.set_max_width(theme.measure_md.value());
            field_with_hint(
                ui,
                theme,
                &mut bufs[0],
                "Workspace name",
                "Shown in the sidebar and window title. You can rename it later.",
            );
            field_with_hint(
                ui,
                theme,
                &mut bufs[1],
                "s_01HXK9",
                "Surface ids are immutable — copy this to address it from the CLI.",
            );
        });
    });

    meta(
        ui,
        theme,
        &[
            ("size", "caption 11–12"),
            ("color", "text-muted"),
            ("case", "sentence"),
            ("line-height", "1.5"),
            ("position", "below the field"),
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
