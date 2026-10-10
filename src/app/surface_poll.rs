//! 보이는 surface 의 주기 확인을 예약한다. DAG 조회와 포커스 탭 탐색기의 외부 변경 확인을 함께 맞춘다.
//! 탐색기 확인은 그리는 동안 하므로, 확인할 때가 된 창은 다시 그리게 표시한다.

use std::time::Instant;

use crate::app::App;
use crate::view::ui::View as _;

impl App {
    pub(crate) fn sync_surface_poll_timers(&mut self, now: Instant) {
        self.sync_dag_poll_timers(now);
        let mut next: Option<Instant> = None;
        for window in self.view.views.values_mut() {
            let Some(main) = window.as_main_mut() else {
                continue;
            };
            let Some(at) = main.state.explorer_views.next_poll_at() else {
                continue;
            };
            if at <= now {
                main.mark_dirty();
            }
            next = Some(next.map_or(at, |n| n.min(at)));
        }
        crate::app::timers::sync_explorer_poll_timer(&mut self.timers, next, now);
    }
}
