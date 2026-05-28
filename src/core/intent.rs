//! `CoreIntent` — `Core::apply` 의 입력. 도메인 변경 요청.
//!
//! 기존 `crate::intent::Intent` 와 *역할이 다르다*:
//! - `Intent` (AppState dispatch): UI + 도메인 혼합. AppState.pending_intents 큐로 push.
//! - `CoreIntent` (Core::apply): *순수 도메인 mutate*. handler 가 read 후 발행, dispatcher 가 drain.
//!
//! Phase D 진행 중. variant 는 점진 추가된다.

use tasty_settings::Settings;

/// 도메인 변경 요청. Core 만이 자기 메서드로 적용한다.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum CoreIntent {
    // ─── Settings (D.3.C.A.2) ───
    /// Settings 전체 교체. cascade — Theme apply / Scrollback limit / clipboard
    /// max / notification coalesce 가 Core 내부에서 자동 발동.
    UpdateSettings(Settings),
}

/// `Core::apply` 의 결과 — 도메인이 *변경 후 알리는* 이벤트.
/// observer / replay / remote attach 의 기반.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(crate) enum CoreEvent {
    // ─── Settings (D.3.C.A.2) ───
    /// Settings 가 갱신됨. 새 값 동봉.
    SettingsUpdated(Settings),
}
