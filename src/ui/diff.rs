//! DiffPanel surface 의 egui 렌더링. 좌/우 분할로 before/after 를 표시한다.
//!
//! 색상 처리는 단순 line diff: 각 줄을 라인 단위로 페어 매칭하지 않고, 좌측엔
//! before 의 모든 줄(삭제 표시 색), 우측엔 after 의 모든 줄(추가 표시 색)을
//! 그대로 표시한다. 정교한 hunk-aligned 표시는 후속 단계.
//!
//! Apply / Reject 버튼은 사용자 입력 기반 (마우스 클릭) 이므로 IPC 로는 노출되지
//! 않는다. Apply 클릭 시 `apply_action` 을 새 터미널에서 spawn 하는 요청은 호출자가
//! 반환된 [`DiffAction`] 으로 처리한다.

use crate::model::DiffPanel;
use crate::theme;

pub enum DiffAction {
    Apply(String),
    Reject,
}

/// `DiffPanel` 의 콘텐츠 영역을 그린다. 사용자가 Apply/Reject 를 클릭하면 해당
/// 액션을 반환 — 호출자가 터미널 spawn 등 부수효과를 일으킨다.
pub fn draw_diff(ui: &mut egui::Ui, panel: &DiffPanel) -> Option<DiffAction> {
    let th = theme::theme();
    let body_size = th.font_size_body.value();
    let removed_bg = th.red.to_egui().linear_multiply(0.18);
    let added_bg = th.green.to_egui().linear_multiply(0.18);
    let header_color: egui::Color32 = th.subtext0.into();

    let mut action: Option<DiffAction> = None;

    ui.vertical(|ui| {
        // 헤더 (타이틀 + 액션 버튼).
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(&panel.title)
                    .size(body_size)
                    .color(header_color),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button(crate::i18n::t("diff.button.reject").to_string())
                    .clicked()
                {
                    action = Some(DiffAction::Reject);
                }
                if let Some(cmd) = panel.apply_action.as_ref() {
                    if ui
                        .button(crate::i18n::t("diff.button.apply").to_string())
                        .clicked()
                    {
                        action = Some(DiffAction::Apply(cmd.clone()));
                    }
                }
            });
        });
        ui.separator();

        let avail = ui.available_size();
        let col_w = (avail.x - 12.0) * 0.5;

        ui.horizontal_top(|ui| {
            // 좌측 — before.
            ui.allocate_ui(egui::vec2(col_w, avail.y - 28.0), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("diff_before")
                    .show(ui, |ui| {
                        for line in panel.before.lines() {
                            let bg = egui::Frame::default().fill(removed_bg);
                            bg.show(ui, |ui| {
                                ui.label(egui::RichText::new(line).size(body_size).monospace());
                            });
                        }
                    });
            });

            ui.separator();

            // 우측 — after.
            ui.allocate_ui(egui::vec2(col_w, avail.y - 28.0), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("diff_after")
                    .show(ui, |ui| {
                        for line in panel.after.lines() {
                            let bg = egui::Frame::default().fill(added_bg);
                            bg.show(ui, |ui| {
                                ui.label(egui::RichText::new(line).size(body_size).monospace());
                            });
                        }
                    });
            });
        });
    });

    action
}
