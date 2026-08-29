//! `Tick::PtySweep` / `Tick::CaptureSweep` / `Tick::LogPrune` 처리 — TTL 기반 정리의
//! **주기 경로**.
//!
//! 세 정리 모두 원래 "누가 건드릴 때 같이 치우는"(접근 시점 lazy) 방식으로만 돌았다.
//! 그래서 접근이 멈추면 정리도 멈췄고, 그 순간이 정확히 정리가 가장 필요한 순간이다
//! (에이전트가 조용해진 뒤 남는 headless PTY 좀비가 대표적 —
//! `docs/adr/0050-headless-pty-primitive.md` "좀비 회수 시점").
//!
//! **lazy 경로는 그대로 남는다.** 이 모듈은 대체가 아니라 보완이다. 특히 `pty` 쪽
//! lazy 는 `pty.spawn` 직전에 돌아 동시 개수 상한 판정을 정확하게 유지하는 별개
//! 역할이 있어, 주기 타이머로 대체하면 "실제로는 idle 인 PTY 때문에 spawn 이 상한
//! 초과로 실패" 하는 회귀가 된다. 두 경로는 같은 함수를 부르므로 idempotent 하다.
//!
//! 엔진 순회는 `app/busy.rs` 의 `poll_busy_states` 와 동형이다 — 살아있는 window 의
//! engine + `parked_states` 를 모두 돈다. headless(단일 engine) 대응은 `boot.rs` 의
//! 타이머 블록에 같은 스텝으로 들어간다.

use std::time::Instant;

use crate::app::App;

impl App {
    /// 살아있는 window 의 engine + `parked_states` 를 모두 돈다. 두 컬렉션이 각각
    /// `self` 를 가변 차용해 하나의 이터레이터로 chain 할 수 없어(`poll_busy_states`
    /// 도 같은 이유로 루프를 둘로 나눈다) 클로저를 받는 형태로 묶었다.
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

    /// idle TTL 을 넘긴 headless PTY 를 전 engine 에서 회수한다.
    pub(crate) fn poll_pty_sweep(&mut self) {
        let now = Instant::now();
        self.for_each_engine(|engine| {
            // 반환 id 는 여기서 쓰지 않는다 — 두 store 회수·waker 게이트 해제까지
            // 공용 함수가 이미 끝냈다.
            let _ = engine.sweep_idle_ptys(now);
        });
    }

    /// TTL 을 넘긴 캡처 업로드 partial 을 전 engine 에서 회수한다.
    pub(crate) fn poll_capture_sweep(&mut self) {
        let now = Instant::now();
        self.for_each_engine(|engine| engine.capture_uploads.sweep_expired(now));
    }

    /// IPC 관측 로그 3종의 보존 정책을 집행한다. engine 순회가 아니다 — memory store
    /// 는 `Core` 소유의 프로세스 단일 인스턴스이고, 집행 게이트도 프로세스 전역이다.
    pub(crate) fn poll_log_prune(&mut self) {
        let now_ms = self.core.now_unix_millis();
        let now_ms = u64::try_from(now_ms).unwrap_or(0);
        self.core.with_memory(|mem| {
            crate::adapters::ipc::log_retention::maybe_prune(mem, now_ms);
        });
    }
}
