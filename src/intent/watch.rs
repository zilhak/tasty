//! Debug 빌드 전용 Intent watch 모듈. 모든 Intent 발화를 `tracing::debug!`로 로그한다.
//!
//! release 빌드에는 컴파일되지 않는다 (`#[cfg(debug_assertions)]`).
//!
//! 활성화: `TASTY_LOG=tasty::intent::watch=debug`. 본체가 읽는 변수는 `TASTY_LOG` 다
//! (`RUST_LOG` 아님 — `docs/dev-guide/crash-diagnostics.md`).

use super::DispatchedIntent;

pub fn observe(intent: &DispatchedIntent) {
    tracing::debug!(
        target: "tasty::intent::watch",
        body = ?intent.body,
        origin = ?intent.origin,
        trace_id = ?intent.trace_id,
        "dispatching intent",
    );
}
