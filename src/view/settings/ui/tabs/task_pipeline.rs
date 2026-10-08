//! 작업 파이프라인의 report 상한을 편집한다. 저장소는 TaskPipelineSettings다.
//!
//! 행 모양은 원격 전송 탭의 숫자 행을 그대로 쓴다. append 상한은 항상 블록 상한보다 작아야 하므로
//! 두 입력의 범위를 서로의 현재 값으로 좁혀 어긋난 쌍을 만들 수 없게 한다.

use tasty_settings::{REPORT_APPEND_BYTES_RANGE, REPORT_BLOCK_BYTES_RANGE};
use tasty_ui_widgets::vspace;

use super::remote_transfer::{row_desc, row_separator, settings_row};
use crate::i18n::t;
use crate::settings::Settings;

pub fn draw_task_pipeline_tab(ui: &mut egui::Ui, settings: &mut Settings) {
    let th = crate::theme::theme();
    vspace(ui, th.spacing_sm);

    ui.label(
        egui::RichText::new(t("settings.task_pipeline.section").to_uppercase())
            .size(th.font_size_micro.value())
            .monospace()
            .color(th.text_muted()),
    );
    vspace(ui, th.spacing_sm);

    let pipeline = &mut settings.task_pipeline;
    let append_max = REPORT_APPEND_BYTES_RANGE
        .1
        .min(pipeline.report_block_bytes.saturating_sub(1));
    if let Some(v) = bytes_row(
        ui,
        &th,
        t("settings.task_pipeline.report_append"),
        "task_pipeline_report_append_bytes",
        (REPORT_APPEND_BYTES_RANGE.0, append_max),
        pipeline.report_append_bytes,
    ) {
        pipeline.report_append_bytes = v;
    }
    row_desc(ui, &th, t("settings.task_pipeline.report_append_desc"));
    row_separator(ui, &th);

    let block_min = REPORT_BLOCK_BYTES_RANGE
        .0
        .max(pipeline.report_append_bytes.saturating_add(1));
    if let Some(v) = bytes_row(
        ui,
        &th,
        t("settings.task_pipeline.report_block"),
        "task_pipeline_report_block_bytes",
        (block_min, REPORT_BLOCK_BYTES_RANGE.1),
        pipeline.report_block_bytes,
    ) {
        pipeline.report_block_bytes = v;
    }
    row_desc(ui, &th, t("settings.task_pipeline.report_block_desc"));
}

/// 바이트 단위 정수 입력 행. 확정된 새 값을 돌려준다.
fn bytes_row(
    ui: &mut egui::Ui,
    th: &tasty_type_appearance::theme::Theme,
    label: &str,
    id: &str,
    (min, max): (u64, u64),
    current: u64,
) -> Option<u64> {
    let mut value = current as f64;
    let mut committed = false;
    settings_row(ui, th, label, |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
            committed = super::number::number_field(
                ui,
                th,
                id,
                &super::number::NumberSpec {
                    min: Some(min as f64),
                    max: Some(max as f64),
                    step: None,
                    decimals: 0,
                    suffix: Some("B"),
                    suffix_mono: true,
                    enabled: true,
                },
                &mut value,
            );
        });
    });
    committed.then_some(value as u64)
}
