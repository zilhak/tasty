//! GUI debug 빌드의 Intent 처리 로그.
//! TASTY_LOG=tasty::intent::watch=debug로 켠다. RUST_LOG는 읽지 않는다.
//! 설정: docs/dev-guide/crash-diagnostics.md.

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
