//! 기타 › 작업 파이프라인 설정의 예제. 실제 설정 저장은 하지 않는다.
//!
//! 새 시각 값은 없다. 원격 전송 탭과 같은 settings-row 숫자 행 두 개이며, 단위만 MiB 대신 B 다.

use std::cell::RefCell;

use tasty_type_appearance::theme::Theme;
use tasty_ui_widgets::{Input, SettingsRow, settings_label_column};

use crate::catalog::spec::{self, StageVariant, TokenChip};
use crate::catalog::widgets::dialog as kit;

// 프레임 폭은 같은 settings-row 를 쓰는 Remote transfer 예제의 값이다.
use super::settings_remote_transfer::WIDTH;

thread_local! {
    static STATE: RefCell<[String; 2]> =
        RefCell::new([String::from("1024"), String::from("16384")]);
}

pub fn draw(ui: &mut egui::Ui, theme: &Theme) {
    spec::stage(ui, theme, StageVariant::Wrap, |ui| {
        kit::frame_card_flat(ui, theme, WIDTH, kit::panel_fill(theme), |ui| {
            kit::region_sym(ui, theme.spacing_lg, theme.spacing_md, |ui| {
                ui.spacing_mut().item_spacing.y = theme.settings_row_gap().value();
                mono_head(ui, theme, "Report limits");
                let rows = [
                    SettingsRow::new("Note size limit")
                        .caption(crate::i18n::t("settings.task_pipeline.report_append_desc")),
                    SettingsRow::new(crate::i18n::t("settings.task_pipeline.report_block"))
                        .caption(crate::i18n::t("settings.task_pipeline.report_block_desc")),
                ];
                let col = settings_label_column(ui, theme, &rows);
                let [note, attempt] = rows;
                STATE.with(|s| {
                    let [append, block] = &mut *s.borrow_mut();
                    note.show(ui, theme, col, |ui| bytes_control(ui, theme, append));
                    separator_line(ui, theme);
                    attempt.show(ui, theme, col, |ui| bytes_control(ui, theme, block));
                });
            });
        });
    });

    spec::meta(
        ui,
        theme,
        &[
            ("L2 position", "Misc — after Scripts"),
            ("rows", "Note size limit · Attempt report limit"),
            ("row", "same as settings-remote-transfer size row"),
            ("unit", "static mono “B”"),
            (
                "ranges",
                "note 64–65536 · attempt 128–131072 · note < attempt",
            ),
        ],
        &[
            TokenChip::without_color("settings-row-min-height", "row height"),
            TokenChip::without_color("field-width-xs", "numeric input"),
            TokenChip::new(
                "separator",
                "row divider",
                theme.separator.to_egui_premultiplied(),
            ),
            TokenChip::new(
                "text-muted",
                "descriptions + unit",
                theme.text_muted().to_egui(),
            ),
        ],
    );

    spec::note(
        ui,
        theme,
        "Two numeric rows for the task report limits. Each input's range follows the other's \
         current value so the note limit always stays below the attempt limit.",
    );
}

fn mono_head(ui: &mut egui::Ui, theme: &Theme, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .monospace()
            .size(theme.font_size_micro.value())
            .color(theme.text_muted().to_egui()),
    );
}

/// 숫자 Input + 단위.
fn bytes_control(ui: &mut egui::Ui, theme: &Theme, buf: &mut String) {
    ui.spacing_mut().item_spacing.x = theme.spacing_sm.value();
    Input::new()
        .mono(true)
        .width(theme.field_width_xs.value())
        .show(ui, theme, buf);
    ui.label(
        egui::RichText::new("B")
            .monospace()
            .size(theme.font_size_caption.value())
            .color(theme.text_muted().to_egui()),
    );
}

fn separator_line(ui: &mut egui::Ui, theme: &Theme) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(w, theme.border_width.value()),
        egui::Sense::hover(),
    );
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        egui::Stroke::new(
            theme.border_width.value(),
            theme.separator.to_egui_premultiplied(),
        ),
    );
}
