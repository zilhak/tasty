//! 훅 완료 신호 → agent task 매핑 (TODO80 §B-4/§C).
//!
//! 완료 전략 레지스트리(TODO80 §B, 아직 미구현)의 push 형 전략이 훅 핸들러
//! (`notify_via: HookHandlerId`)로 완료를 보고할 때, 그 훅이 "어느 task 의"
//! 완료인지는 훅 정의 자체엔 담을 수 없다 — owner 가 등록 시 고정되고
//! (`HookHandlerAction` 의 데이터/흐름 분리 불변식), task id 를 담을 슬롯이
//! 없다. §B-4 결정 (ii) 가 채택한 해법이 이 모듈이다: 러너(host executor)가
//! `AwaitExternal` 핸들로 dispatch 하며 `register` 하고, `PendingHostEvent::
//! HookFired` 소비부(`Core::resolve_hook_task_wait`)가 훅 발화마다 조회해
//! 매칭되면 task 를 마감한다.
//!
//! **등록자(러너의 push-dispatch 경로)는 TODO80 §B(레지스트리)가 아직 없어
//! 오늘은 존재하지 않는다** — 이 모듈은 그 미래 등록자를 위한 준비된
//! 착지점이다. 다만 소비부(`resolve`)는 이미 실제로 살아 있는 `HookFired`
//! 이벤트 스트림에 매 발화마다 실행되는 진짜 코드다 — 등록 0건이면 안전한
//! no-op 이라, `AutoWaitDecl` 이 겪은 "소비자 없는 확장점" 문제(TODO80 문서의
//! 선례 경고)를 이 절반은 피한다.

use std::collections::HashMap;
use std::sync::Mutex;

use tasty_agent::task::TaskId;

/// hook_id → (workspace_id, task_id) 1회성 매핑.
pub(crate) struct HookTaskWaits {
    inner: Mutex<HashMap<u64, (u32, TaskId)>>,
}

impl HookTaskWaits {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// `task_id` 가 `hook_id` 의 완료를 기다리고 있음을 등록. 같은 hook_id 에
    /// 재등록하면 이전 대기를 덮어쓴다(마지막 등록자 승리 — 한 hook 은 한 번에
    /// 한 task 만 기다릴 수 있다는 전제).
    // 이유: 등록자(완료 전략 레지스트리의 push-dispatch 경로, TODO80 §B)가 아직
    // 없다 — `resolve`(소비부, 이미 HookFired 마다 실행되는 실제 코드)의 착지점
    // 으로 미리 마련해 둔 API. `Core::register_hook_task_wait` 를 통해서만 외부
    // 노출되며 그쪽도 동일 사유로 allow.
    #[allow(dead_code)]
    pub fn register(&self, hook_id: u64, workspace_id: u32, task_id: TaskId) {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.insert(hook_id, (workspace_id, task_id));
    }

    /// `hook_id` 에 대기 중인 task 가 있으면 **제거하고** 반환(1회성 소비) —
    /// 조회 후 잔존시키면 같은 hook_id 의 재발화(예: 같은 훅이 다른 목적으로
    /// 재등록된 경우)가 이미 끝난 task 를 다시 매칭하는 오탐 위험이 있다.
    pub fn resolve(&self, hook_id: u64) -> Option<(u32, TaskId)> {
        let mut guard = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.remove(&hook_id)
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        let guard = match self.inner.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        guard.len()
    }
}

impl Default for HookTaskWaits {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_resolve_removes_entry() {
        let waits = HookTaskWaits::new();
        waits.register(42, 1, "t-1".to_string());
        assert_eq!(waits.len(), 1);
        assert_eq!(waits.resolve(42), Some((1, "t-1".to_string())));
        // 1회성 소비 — 같은 hook_id 재조회는 None.
        assert_eq!(waits.resolve(42), None);
        assert_eq!(waits.len(), 0);
    }

    #[test]
    fn resolve_unregistered_hook_id_is_none() {
        let waits = HookTaskWaits::new();
        assert_eq!(waits.resolve(999), None);
    }

    #[test]
    fn reregistering_same_hook_id_overwrites_previous_wait() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-old".to_string());
        waits.register(1, 2, "t-new".to_string());
        assert_eq!(waits.len(), 1);
        assert_eq!(waits.resolve(1), Some((2, "t-new".to_string())));
    }

    #[test]
    fn independent_hook_ids_do_not_interfere() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-1".to_string());
        waits.register(2, 1, "t-2".to_string());
        assert_eq!(waits.resolve(1), Some((1, "t-1".to_string())));
        // hook_id 2 의 대기는 hook_id 1 소비와 무관하게 살아있어야 한다.
        assert_eq!(waits.len(), 1);
        assert_eq!(waits.resolve(2), Some((1, "t-2".to_string())));
    }
}
