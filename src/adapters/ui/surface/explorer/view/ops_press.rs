//! Retry 한 번이 작업 둘(남은 원본 지우기와 다시 옮기기)을 넣을 때 결과를 카드 하나로 모은다.
//! 그 누름의 작업이 모두 끝나야 카드가 뜨고, 수는 합치며 톤은 더 나쁜 결과를 따른다.

use std::time::{Duration, Instant};

use crate::app::explorer_files::job::{OpKind, Report};

use super::OpsState;

/// 한 번의 누름으로 넣은 요청들.
pub(super) struct Press {
    id: u64,
    /// 아직 끝나지 않은 요청 번호.
    waiting: Vec<u64>,
    /// 먼저 끝난 작업들의 합친 결과.
    held: Option<Report>,
    /// 누를 때의 표준 표시 시간. 남은 결과만으로 카드를 낼 때도 이 시간을 따른다.
    lifetime: Duration,
}

impl OpsState {
    /// `requests` 를 한 누름으로 묶는다. 묶음 번호를 돌려준다.
    pub(crate) fn expect_press(&mut self, requests: Vec<u64>, lifetime: Duration) -> u64 {
        self.next_card += 1;
        let id = self.next_card;
        self.presses.push(Press {
            id,
            waiting: requests,
            held: None,
            lifetime,
        });
        id
    }
    /// 묶음의 요청 하나가 끝났다. 묶음이 다 끝났으면 합친 결과를, 아니면 None 을 돌려준다.
    pub(crate) fn arrive(&mut self, press: u64, request: u64, report: Report) -> Option<Report> {
        let Some(index) = self.presses.iter().position(|p| p.id == press) else {
            return Some(report);
        };
        let entry = &mut self.presses[index];
        entry.waiting.retain(|r| *r != request);
        entry.held = Some(match entry.held.take() {
            Some(held) => merge(held, report),
            None => report,
        });
        if !entry.waiting.is_empty() {
            return None;
        }
        self.presses.remove(index).held
    }
    /// 시작하지 않고 빠진 요청을 묶음에서 뺀다. 남은 작업이 모두 끝났으면 그 결과를 카드로 둔다.
    /// 카드는 다른 결과와 같이 모두 끝났거나 취소한 결과만 누를 때의 표준 시간 뒤 사라진다.
    pub(crate) fn forget_request(&mut self, request: u64) {
        if let Some((report, lifetime)) = self.take_forgotten(request) {
            let expires = super::result_is_timed(&report).then(|| Instant::now() + lifetime);
            self.push_result(report, None, expires);
        }
    }
    fn take_forgotten(&mut self, request: u64) -> Option<(Report, Duration)> {
        let index = self
            .presses
            .iter()
            .position(|p| p.waiting.contains(&request))?;
        let entry = &mut self.presses[index];
        entry.waiting.retain(|r| *r != request);
        if !entry.waiting.is_empty() {
            return None;
        }
        let press = self.presses.remove(index);
        Some((press.held?, press.lifetime))
    }
}

/// 두 결과를 합친다. 다시 옮긴 작업이 있으면 이동 결과로 보이고, 일부만 되돌릴 수 있으므로 Undo 는 없다.
/// 원본이 남았거나 실패한 항목이 있으면 취소보다 그쪽을 보인다.
pub(super) fn merge(mut a: Report, b: Report) -> Report {
    if b.kind == OpKind::Move {
        a.kind = OpKind::Move;
    }
    a.dest = a.dest.or(b.dest);
    a.total += b.total;
    a.done += b.done;
    a.undo.clear();
    a.failed.extend(b.failed);
    a.skipped.extend(b.skipped);
    a.leftovers.extend(b.leftovers);
    a.trash_unavailable |= b.trash_unavailable;
    a.cancelled = (a.cancelled || b.cancelled) && a.failed.is_empty() && a.skipped.is_empty();
    a
}
