//! 보이는 surface 의 주기 확인을 예약한다. DAG 조회와 포커스 탭 탐색기의 외부 변경 확인을 함께 맞춘다.
//! 탐색기 확인은 창을 그리지 않고 시작한다. 바뀐 폴더를 다시 읽은 결과가 와야 창을 그린다.

use std::time::Instant;

use crate::app::App;

impl App {
    pub(crate) fn sync_surface_poll_timers(&mut self, now: Instant) {
        self.sync_dag_poll_timers(now);
        let mut next: Option<Instant> = None;
        let mut started = false;
        for window in self.view.views.values_mut() {
            let Some(main) = window.as_main_mut() else {
                continue;
            };
            let views = &mut main.state.explorer_views;
            started |= views.start_due_checks(now);
            if let Some(at) = views.next_poll_at() {
                next = Some(next.map_or(at, |n| n.min(at)));
            }
        }
        if started {
            // 이번 루프의 읽기 처리는 이미 지났으므로 시작한 확인을 바로 worker 에 넘긴다.
            self.poll_local_reads();
        }
        crate::app::timers::sync_explorer_poll_timer(&mut self.timers, next, now);
    }
}
