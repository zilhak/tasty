//! 받지 않은 파일 작업 요청을 그 칸에 알리는 경고 카드. 결과 카드와 같은 모양으로 상태줄 위에
//! 쌓이고 표준 시간 뒤 사라진다. 대기열이 찼을 때는 이 칸의 대기열 팝오버를 여는 동작을 붙인다.

use std::time::Instant;

use tasty_type_appearance::theme::Theme;
use tasty_type_appearance::toast_kind::ToastKind;
use tasty_ui_widgets::{ButtonVariant, ResultAction, ResultCardProps, result_card};

use super::{OpsState, glyph, in_cell_area};
use crate::adapters::ui::icons;
use crate::i18n::t;

/// 받지 않은 요청의 이유.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    /// 기다리는 작업이 상한까지 찼다.
    QueueFull,
    /// 한 요청의 경로 자료가 상한을 넘는다.
    TooLarge,
}

impl Refused {
    pub(crate) fn text(self) -> &'static str {
        match self {
            Self::QueueFull => t("explorer.request.queue_full"),
            Self::TooLarge => t("explorer.request.too_large"),
        }
    }
}

pub(super) struct Notice {
    id: u64,
    pub(super) refused: Refused,
    pub(super) expires: Instant,
}

impl OpsState {
    /// 받지 않은 요청을 알린다. 같은 이유의 카드가 떠 있으면 새로 쌓지 않고 수명만 늘린다.
    pub(crate) fn push_refused(&mut self, refused: Refused, expires: Instant) {
        if let Some(notice) = self.notices.iter_mut().find(|n| n.refused == refused) {
            notice.expires = expires;
            return;
        }
        self.next_card += 1;
        self.notices.push(Notice {
            id: self.next_card,
            refused,
            expires,
        });
    }
    #[cfg(test)]
    pub(crate) fn refused(&self) -> Vec<Refused> {
        self.notices.iter().map(|n| n.refused).collect()
    }
}

/// 대기열 보기 동작을 붙일 수 있는가. 대기열 팝오버는 이 칸의 작업이 실행 중일 때만 열린다.
pub(super) fn offers_show_queue(refused: Refused, ops: &OpsState) -> bool {
    refused == Refused::QueueFull && ops.running.is_some()
}

/// 알림 카드를 상태줄 바로 위부터 쌓고, 그 위 카드가 시작할 높이를 돌려준다.
pub(super) fn draw(
    ui: &mut egui::Ui,
    theme: &Theme,
    ops: &mut OpsState,
    cell: egui::Rect,
    status_top: f32,
) -> f32 {
    if ops.notices.is_empty() {
        return status_top;
    }
    let width = theme.toast_max_width().value().min(cell.width());
    let sm = theme.spacing_sm.value();
    let close = glyph(icons::CLOSE);
    let show_queue = t("explorer.request.show_queue");
    let entries: Vec<(u64, Refused, bool)> = ops
        .notices
        .iter()
        .map(|n| (n.id, n.refused, offers_show_queue(n.refused, ops)))
        .collect();
    let (top, picks) = in_cell_area(ui, cell, "explorer_ops_notices", |ui| {
        let mut bottom = status_top - sm;
        let mut picks = Vec::new();
        for &(id, refused, queue) in entries.iter().rev() {
            let key = ui.id().with(("explorer_ops_notice", id));
            let h = ui.ctx().data(|d| d.get_temp::<f32>(key)).unwrap_or(0.0);
            let min = egui::pos2(cell.right() - sm - width, bottom - h);
            let mut child = ui.new_child(egui::UiBuilder::new().id_salt(key).max_rect(
                egui::Rect::from_min_size(min, egui::vec2(width, h.max(1.0))),
            ));
            let actions: &[ResultAction<'_>] = if queue {
                &[ResultAction {
                    label: show_queue,
                    variant: ButtonVariant::Ghost,
                }]
            } else {
                &[]
            };
            let (rect, resp) = result_card(
                &mut child,
                theme,
                width,
                &ResultCardProps {
                    kind: ToastKind::Warning,
                    title: refused.text(),
                    lines: &[],
                    more: None,
                    actions,
                    dismiss_tip: t("explorer.result.dismiss"),
                },
                &close,
            );
            if (rect.height() - h).abs() > f32::EPSILON {
                ui.ctx().data_mut(|d| d.insert_temp(key, rect.height()));
                ui.ctx().request_repaint();
            }
            bottom -= rect.height() + sm;
            if resp.dismiss {
                picks.push((id, false));
            } else if resp.action == Some(0) {
                picks.push((id, true));
            }
        }
        (bottom + sm, picks)
    });
    for (id, show) in picks {
        if show {
            ops.queue_open = true;
        }
        ops.notices.retain(|n| n.id != id);
    }
    top
}
