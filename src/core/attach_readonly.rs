//! 서버는 attach 점유 중인 터미널의 표시용 사본을 주기적으로 갱신한다.
//! PTY를 가진 원본은 유지하고 GUI는 이 사본을 읽기 전용으로 그린다. 갱신 주기는 호출부 타이머가 정한다.

use tasty_terminal::Terminal;

use crate::core::CoreState;

impl CoreState {
    /// 점유 터미널의 화면을 사본에 적용하고 점유가 끝난 사본은 지운다.
    /// snapshot의 clear·home으로 이전 화면을 덮으며 크기가 바뀌면 사본을 다시 만든다.
    /// live 터미널을 하나라도 갱신했으면 true다.
    pub(crate) fn refresh_readonly_views(&mut self) -> bool {
        let attached: Vec<u32> = self
            .attach
            .locks_snapshot()
            .into_iter()
            .map(|(sid, _)| sid)
            .collect();

        self.readonly_views.retain(|sid, _| attached.contains(sid));

        let mut any = false;
        for sid in attached {
            let Some(live) = self.terminals.get(sid) else {
                continue;
            };
            let cols = live.cols();
            let rows = live.rows();
            let snapshot = live.snapshot_as_vt();
            let mirror = self
                .readonly_views
                .entry(sid)
                .or_insert_with(|| Terminal::new_detached(cols, rows));
            if mirror.cols() != cols || mirror.rows() != rows {
                *mirror = Terminal::new_detached(cols, rows);
            }
            mirror.feed_bytes(&snapshot);
            any = true;
        }
        any
    }

    /// 첫 갱신 전에는 사본이 없어 None이다. live 터미널이 없으면 이후 갱신도 건너뛴다.
    pub(crate) fn readonly_view(&self, surface_id: u32) -> Option<&Terminal> {
        self.readonly_views.get(&surface_id)
    }
}
