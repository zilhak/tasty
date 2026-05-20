//! 하단 패널 (diff 모드) — 선택된 파일의 diff 표시.
//!
//! 닫기 요청(Back 버튼) 시 true 반환.

use crate::i18n::t;
use crate::theme;

use super::GitViewerState;
use super::data::DiffLineKind;

pub fn draw_diff_panel(ui: &mut egui::Ui, state: &GitViewerState) -> bool {
    let th = theme::theme();
    let mut close_requested = false;

    let Some(diff) = state.diff_content.as_ref() else {
        ui.label(
            egui::RichText::new(t("git_viewer.loading"))
                .small()
                .color(th.subtext0),
        );
        return false;
    };

    // 도구 모음
    ui.horizontal(|ui| {
        if ui.small_button(t("git_viewer.back_to_log")).clicked() {
            close_requested = true;
        }
        ui.label(
            egui::RichText::new(&diff.file_path)
                .small()
                .color(th.subtext0),
        );
    });

    if diff.hunks.is_empty() {
        ui.label(
            egui::RichText::new(t("git_viewer.no_changes"))
                .small()
                .color(th.subtext0),
        );
        return close_requested;
    }

    let font = egui::FontId::monospace(th.font_size_body.value());

    egui::ScrollArea::vertical()
        .id_salt("git_viewer_diff_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for hunk in &diff.hunks {
                ui.label(
                    egui::RichText::new(&hunk.header)
                        .font(font.clone())
                        .color(th.blue),
                );
                for line in &hunk.lines {
                    let (prefix, color): (&str, egui::Color32) = match line.kind {
                        DiffLineKind::Addition => ("+", th.green.into()),
                        DiffLineKind::Deletion => ("-", th.red.into()),
                        DiffLineKind::Context => (" ", th.text.into()),
                    };
                    let old_no = line
                        .old_lineno
                        .map(|n| format!("{n:>4}"))
                        .unwrap_or_else(|| "    ".to_string());
                    let new_no = line
                        .new_lineno
                        .map(|n| format!("{n:>4}"))
                        .unwrap_or_else(|| "    ".to_string());
                    ui.label(
                        egui::RichText::new(format!("{old_no} {new_no} {prefix} {}", line.content))
                            .font(font.clone())
                            .color(color),
                    );
                }
            }
        });

    close_requested
}
