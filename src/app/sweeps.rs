//! 요청이 없어도 TTL이 지난 PTY·캡처 업로드와 오래된 로그를 주기적으로 정리한다.
//! 접근 시 정리도 유지한다. 특히 pty.spawn 전 정리는 동시 개수 상한을 판단하는 데 필요하다.

use std::time::Instant;

use crate::app::App;

impl App {
    fn for_each_engine(&mut self, mut f: impl FnMut(&mut crate::core::CoreState)) {
        for w in self.view.views.values_mut() {
            if let Some(main) = w.as_main_mut() {
                f(&mut main.core_state);
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            f(engine);
        }
    }

    pub(crate) fn poll_pty_sweep(&mut self) {
        let now = Instant::now();
        self.for_each_engine(|engine| {
            // 정리는 함수 안에서 끝나며 여기서는 회수한 ID 목록을 사용하지 않는다.
            let _ = engine.sweep_idle_ptys(now);
        });
    }

    pub(crate) fn poll_capture_sweep(&mut self) {
        let now = Instant::now();
        self.for_each_engine(|engine| engine.capture_uploads.sweep_expired(now));
    }

    /// memory store와 집행 조건은 프로세스가 공유하므로 engine별로 반복하지 않는다.
    pub(crate) fn poll_log_prune(&mut self) {
        let now_ms = self.core.now_unix_millis();
        let now_ms = u64::try_from(now_ms).unwrap_or(0);
        self.core.with_memory(|mem| {
            crate::store::log_retention::maybe_prune(mem, now_ms);
        });
    }
}
