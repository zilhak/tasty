//! Inbound port — *외부 어댑터 → Core* 의 진입점.
//!
//! IPC / UI / CLI / Plugin adapter 가 본 trait 을 통해 Core 에 명령 발행.
//! Core 자체가 본 trait 들을 구현한다.

use crate::core::CoreState;
use crate::core::intent::{CoreEvent, CoreIntent};

/// Intent 발행 진입점.
///
/// - `dispatch`: sync 처리 + 결과 반환 (IPC handler 의 응답 contract).
/// - `enqueue`: fire-and-forget (UI 의 click handler 등).
/// - `drain_queue`: dispatcher loop 가 enqueue 된 Intent 들을 dispatch.
#[allow(dead_code)]
pub trait IntentDispatcher: Send + Sync {
    fn dispatch(&mut self, intent: CoreIntent) -> anyhow::Result<ApplyResult>;
    fn enqueue(&mut self, intent: CoreIntent);
    fn drain_queue(&mut self) -> anyhow::Result<Vec<CoreEvent>>;
}

/// Core 의 read-only 접근.
#[allow(dead_code)]
pub trait CoreReader {
    fn state(&self) -> &CoreState;
}

/// Core 의 event 구독 — observer / plugin notification / lua hook.
#[allow(dead_code)]
pub trait EventSubscriber {
    type Subscription;

    fn subscribe(
        &mut self,
        filter: EventFilter,
        callback: Box<dyn FnMut(&CoreEvent) + Send + Sync + 'static>,
    ) -> Self::Subscription;

    fn unsubscribe(&mut self, sub: Self::Subscription);
}

/// `Core::apply` 의 반환 — 발행된 event 목록 + 선택적 sync 반환 data.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub struct ApplyResult {
    pub events: Vec<CoreEvent>,
    pub data: Option<serde_json::Value>,
}

/// Event 구독 필터 (필요 시 변형 추가).
#[allow(dead_code)]
#[derive(Debug, Clone, Default)]
pub struct EventFilter {
    pub kinds: Option<Vec<String>>,
}
