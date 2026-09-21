//! IPC 핸들러가 낸 UI intent 의 출구.
//!
//! 핸들러는 intent 를 창 큐(`AppState::pending_intents`)에 직접 넣지 않고 요청 하나의
//! [`IntentOutbox`] 에 넣는다. 진입점(`check_request` · 라우터의 `dispatch_routed` ·
//! `record_plugin_rss_samples`)이 요청이 끝날 때 그 출구를 창 큐 끝으로 한 번에 옮긴다. 요청
//! 하나 안의 적재 순서는 출구에 넣은 순서 그대로이고, 게이트가 낸 것이 핸들러가 낸 것보다
//! 먼저다(`handler/intent_order_tests.rs` 가 고정한다). 그래서 intent 만 내는 핸들러는 창
//! 상태를 인자로 받지 않는다.

use crate::intent::DispatchedIntent;

/// 요청 하나가 발화한 UI intent 의 출구. 핸들러는 창 큐가 아니라 여기에 넣는다.
///
/// 출구는 진입점이 만들고 요청이 끝날 때 창 큐로 비운다. 핸들러가
/// 출구에만 닿으면 "이 핸들러는 창 상태를 안 읽고 intent 만 낸다" 를 시그니처가 말한다.
#[derive(Default, Debug)]
pub(crate) struct IntentOutbox(Vec<DispatchedIntent>);

impl IntentOutbox {
    /// intent 하나를 출구 끝에 넣는다.
    pub(crate) fn push(&mut self, intent: DispatchedIntent) {
        self.0.push(intent);
    }

    /// 넣은 순서 그대로 꺼낸다.
    pub(crate) fn into_vec(self) -> Vec<DispatchedIntent> {
        self.0
    }

    /// 넣은 것이 없는가.
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
