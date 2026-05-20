//! 상단 패널 — 변경 파일 목록.

use crate::i18n::t;
use crate::theme;

use super::GitViewerState;
use super::data::FileStatus;

/// 클릭된 파일 인덱스를 반환한다.
pub fn draw_status_panel(ui: &mut egui::Ui, state: &GitViewerState) -> Option<usize> {
    let th = theme::theme();

    if state.status_entries.is_empty() {
        ui.label(
            egui::RichText::new(t("git_viewer.no_changes"))
                .small()
                .color(th.subtext0),
        );
        return None;
    }

    let mut clicked: Option<usize> = None;

    egui::ScrollArea::vertical()
        .id_salt("git_viewer_status_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for (idx, entry) in state.status_entries.iter().enumerate() {
                let selected = state.selected_file == Some(idx);
                let bg = if selected {
                    th.hover_overlay.to_egui_premultiplied()
                } else {
                    egui::Color32::TRANSPARENT
                };
                let frame_resp = egui::Frame::new()
                    .fill(bg)
                    .corner_radius(2.0)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let (label_text, color) = status_label(entry.status, &th);
                            ui.label(
                                egui::RichText::new(label_text)
                                    .monospace()
                                    .small()
                                    .color(color),
                            );
                            ui.label(egui::RichText::new(&entry.path).small());
                        });
                    });
                let resp = frame_resp.response.interact(egui::Sense::click());
                if resp.clicked() {
                    clicked = Some(idx);
                }
            }
        });

    clicked
}

fn status_label(s: FileStatus, th: &theme::Theme) -> (&'static str, egui::Color32) {
    match s {
        FileStatus::Modified => (" M ", th.yellow.into()),
        FileStatus::Added => (" A ", th.green.into()),
        FileStatus::Deleted => (" D ", th.red.into()),
        FileStatus::Renamed => (" R ", th.blue.into()),
        FileStatus::Untracked => (" ? ", th.overlay0.into()),
        FileStatus::Conflicted => (" U ", th.red.into()),
    }
}
