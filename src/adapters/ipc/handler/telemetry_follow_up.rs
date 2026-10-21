//! commit된 telemetry 기록 뒤의 cap 액션과 RSS 탐지.
//!
//! `telemetry.record`의 응답 continuation은 창 문맥 없이 실행된다. 알림·승인 요청은 창 큐에 넣어야
//! 하므로 continuation이 여기에 쌓고, 메인 루프가 회차마다 첫 engine 문맥으로 실행한다. 요청 게이트의
//! 기록은 게이트가 창 문맥을 가지고 있어 이 대기열을 거치지 않는다.

use std::sync::Mutex;

use super::telemetry::{self, TriggeredCap};
use crate::app::services::AppServices;
use crate::runtime::engine_access::EngineMut;
use crate::state::RequestContext;

/// RSS 자기 보고 하나.
pub(crate) struct RssSample {
    pub(crate) agent: String,
    pub(crate) bytes: u64,
    pub(crate) ts: u64,
}

/// 기록 하나의 후속 처리. 기록 순서대로 실행한다.
pub(crate) struct FollowUp {
    pub(crate) triggered: Vec<TriggeredCap>,
    pub(crate) rss: Option<RssSample>,
}

static POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Default)]
pub(crate) struct FollowUps(Mutex<Vec<FollowUp>>);

impl FollowUps {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<FollowUp>> {
        crate::poison::recover_mutex(self.0.lock(), "telemetry follow-ups", &POISONED)
    }

    pub(crate) fn push(&self, follow_up: FollowUp) {
        if follow_up.triggered.is_empty() && follow_up.rss.is_none() {
            return;
        }
        self.lock().push(follow_up);
    }

    #[cfg(feature = "gui")]
    pub(crate) fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }

    fn take(&self) -> Vec<FollowUp> {
        std::mem::take(&mut *self.lock())
    }
}

/// 쌓인 후속 처리를 실행한다. 표시 출력은 같은 회차에 적용한다.
pub(crate) fn run(core: &mut AppServices, state: &mut RequestContext, engine: &mut EngineMut<'_>) {
    let pending = core.telemetry_follow_ups().take();
    if pending.is_empty() {
        return;
    }
    let mut scope = crate::ipc::request_scope::RequestScope::capture(
        state,
        engine.core,
        #[cfg(feature = "gui")]
        None,
    );
    let mut out = crate::ipc::window_port::IntentOutbox::default();
    for follow_up in pending {
        telemetry::fire_triggered_caps(core, &mut scope, &mut out, engine, &follow_up.triggered);
        if let Some(rss) = follow_up.rss {
            telemetry::record_rss_sample(
                core, &mut scope, &mut out, engine, &rss.agent, rss.bytes, rss.ts,
            );
        }
    }
    crate::ipc::window_port::IpcWindow::enqueue_intents(&mut scope, out);
    let outputs = scope.finish();
    outputs.apply(state, engine.core);
}

/// engine이 없어 실행할 수 없는 후속 처리를 버린다. 기록과 cap triggered는 이미 저장됐다.
#[cfg(feature = "gui")]
pub(crate) fn discard(core: &AppServices) {
    let dropped = core.telemetry_follow_ups().take().len();
    if dropped > 0 {
        tracing::warn!(
            dropped,
            "telemetry follow-ups dropped without an engine to notify"
        );
    }
}
