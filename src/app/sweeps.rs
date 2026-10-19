//! 요청이 없어도 TTL이 지난 PTY·캡처 업로드와 오래된 로그를 주기적으로 정리한다.
//! 접근 시 정리도 유지한다. pty.spawn·pty.list는 주기 타이머를 기다리지 않고 같은 회수를 적용한다.

use std::time::Instant;

use crate::app::App;

impl App {
    /// 회수 기록과 알림은 공용 함수가 끝낸다. 창은 알림 표시를 갱신하도록 다시 그린다.
    pub(crate) fn poll_pty_sweep(&mut self) {
        let now = Instant::now();
        for (_, main, mut engine) in self.engines_mut().window_pairs() {
            if engine.reclaim_standalone_ptys(now) > 0 {
                main.base.state.dirty = true;
            }
        }
        for mut engine in self.engines_mut().parked() {
            engine.reclaim_standalone_ptys(now);
        }
    }

    pub(crate) fn poll_capture_sweep(&mut self) {
        let now = Instant::now();
        for engine in self.engines_mut().windowed_and_parked() {
            engine.remote.capture_uploads.sweep_expired(now);
        }
    }

    /// memory store와 집행 조건은 프로세스가 공유하므로 engine별로 반복하지 않는다.
    pub(crate) fn poll_log_prune(&mut self) {
        let now_ms = self.services.now_unix_millis();
        let now_ms = u64::try_from(now_ms).unwrap_or(0);
        self.services.submit_log_prune(now_ms);
    }
}
