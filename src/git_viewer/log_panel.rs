//! 하단 패널 (기본 모드) — 커밋 평면 리스트.
//!
//! 그래프는 Phase 2 로 분리. 여기서는 줄당 `[oid] [refs?] summary  author  time` 형식.

use crate::i18n::t;
use crate::theme;

use super::GitViewerState;

pub fn draw_log_panel(ui: &mut egui::Ui, state: &GitViewerState) {
    let th = theme::theme();

    if state.log_entries.is_empty() {
        ui.label(
            egui::RichText::new(t("git_viewer.no_commits"))
                .small()
                .color(th.subtext0),
        );
        return;
    }

    egui::ScrollArea::vertical()
        .id_salt("git_viewer_log_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for entry in &state.log_entries {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&entry.oid_short)
                            .monospace()
                            .small()
                            .color(th.blue),
                    );
                    for r in &entry.refs {
                        let color = if r.starts_with("tags/") {
                            th.green
                        } else {
                            th.yellow
                        };
                        ui.label(egui::RichText::new(format!("({r})")).small().color(color));
                    }
                    ui.label(egui::RichText::new(&entry.summary).small().color(th.text));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(&entry.time).small().color(th.subtext0));
                        ui.label(
                            egui::RichText::new(&entry.author)
                                .small()
                                .color(th.subtext0),
                        );
                    });
                });
            }
        });
}
