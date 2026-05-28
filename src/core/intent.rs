//! `CoreIntent` — `Core::apply` 의 입력. 도메인 변경 요청.
//!
//! 기존 `crate::intent::Intent` 와 *역할이 다르다*:
//! - `Intent` (AppState dispatch): UI + 도메인 혼합. AppState.pending_intents 큐로 push.
//! - `CoreIntent` (Core::apply): *순수 도메인 mutate*. handler 가 read 후 발행, dispatcher 가 drain.
//!
//! Phase D 진행 중. variant 는 점진 추가된다. 초기에는 비어있음.

/// 도메인 변경 요청. Core 만이 자기 메서드로 적용한다.
#[derive(Debug, Clone)]
pub(crate) enum CoreIntent {
    // Phase D 진행 중 점진 추가됨.
}

/// `Core::apply` 의 결과 — 도메인이 *변경 후 알리는* 이벤트.
/// observer / replay / remote attach 의 기반.
#[derive(Debug, Clone)]
pub(crate) enum CoreEvent {
    // Phase D 진행 중 점진 추가됨.
}
