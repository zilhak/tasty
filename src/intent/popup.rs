//! Popup 도메인 Intent 핸들러. TODO 03 의 마이그레이션이 본 모듈로 들어온다.
//!
//! TODO 02 (코어 인프라) 단계에서는 스텁만 두고, 실제 핸들러 본문은 TODO 03 에서 채운다.

use super::{DispatchedIntent, Intent};
use crate::state::AppState;

/// popup 도메인 분기 핸들러. `dispatch_pending_intents` 에서 호출.
pub fn handle(state: &mut AppState, intent: &DispatchedIntent) {
    match &intent.body {
        Intent::OpenPopup { .. } | Intent::ClosePopup { .. } | Intent::TogglePopup { .. } => {
            // TODO 03: 본문 채우기 (origin.is_user() 분기 + state.popups.open*/close/toggle).
            tracing::debug!(
                "intent::popup::handle (stub) — body={:?} origin={:?}",
                intent.body,
                intent.origin
            );
            let _ = state;
        }
        _ => {}
    }
}
