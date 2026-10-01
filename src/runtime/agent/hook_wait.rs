//! 완료 훅 ID를 대기 중인 작업에 연결한다.
//! 러너가 공유 Arc에 등록하고 호스트의 HookFired 소비자가 찾아 작업을 마친다.

use std::collections::HashMap;
use std::sync::Mutex;

use tasty_agent::task::TaskId;

const HOOK_WAIT_WHAT: &str = "agent hook-wait registry";
static HOOK_WAIT_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// hook ID별 일회성 대기. 보고가 없으면 sweep_expired로 만료 항목을 회수해 호출자가 처리한다.
pub(crate) struct HookTaskWaits {
    inner: Mutex<HashMap<u64, (u32, TaskId, u64)>>,
}

impl HookTaskWaits {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// 같은 hook ID는 마지막 등록으로 바꾼다. deadline_ms는 Unix epoch 기준 절대 밀리초다.
    /// 러너는 메인 스레드의 Core를 직접 사용하지 않고 공유 Arc로 등록한다.
    pub fn register(&self, hook_id: u64, workspace_id: u32, task_id: TaskId, deadline_ms: u64) {
        let mut guard =
            crate::poison::recover_mutex(self.inner.lock(), HOOK_WAIT_WHAT, &HOOK_WAIT_POISONED);
        guard.insert(hook_id, (workspace_id, task_id, deadline_ms));
    }

    /// 한 번 반환한 매핑은 지워 같은 훅의 재발생이 끝난 작업과 다시 연결되지 않게 한다.
    pub fn resolve(&self, hook_id: u64) -> Option<(u32, TaskId)> {
        let mut guard =
            crate::poison::recover_mutex(self.inner.lock(), HOOK_WAIT_WHAT, &HOOK_WAIT_POISONED);
        guard.remove(&hook_id).map(|(ws, tid, _)| (ws, tid))
    }

    /// deadline <= now_ms인 항목을 락 안에서 제거한다. 여러 러너가 호출해도 같은 항목을 중복 회수하지 않는다.
    pub fn sweep_expired(&self, now_ms: u64) -> Vec<(u32, TaskId)> {
        let mut guard =
            crate::poison::recover_mutex(self.inner.lock(), HOOK_WAIT_WHAT, &HOOK_WAIT_POISONED);
        let expired_ids: Vec<u64> = guard
            .iter()
            .filter(|(_, (_, _, deadline))| *deadline <= now_ms)
            .map(|(hook_id, _)| *hook_id)
            .collect();
        expired_ids
            .into_iter()
            .filter_map(|id| guard.remove(&id).map(|(ws, tid, _)| (ws, tid)))
            .collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        let guard =
            crate::poison::recover_mutex(self.inner.lock(), HOOK_WAIT_WHAT, &HOOK_WAIT_POISONED);
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

    const NO_DEADLINE: u64 = u64::MAX;

    #[test]
    fn register_then_resolve_removes_entry() {
        let waits = HookTaskWaits::new();
        waits.register(42, 1, "t-1".to_string(), NO_DEADLINE);
        assert_eq!(waits.len(), 1);
        assert_eq!(waits.resolve(42), Some((1, "t-1".to_string())));
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
        waits.register(1, 1, "t-old".to_string(), NO_DEADLINE);
        waits.register(1, 2, "t-new".to_string(), NO_DEADLINE);
        assert_eq!(waits.len(), 1);
        assert_eq!(waits.resolve(1), Some((2, "t-new".to_string())));
    }

    #[test]
    fn independent_hook_ids_do_not_interfere() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-1".to_string(), NO_DEADLINE);
        waits.register(2, 1, "t-2".to_string(), NO_DEADLINE);
        assert_eq!(waits.resolve(1), Some((1, "t-1".to_string())));
        assert_eq!(waits.len(), 1);
        assert_eq!(waits.resolve(2), Some((1, "t-2".to_string())));
    }

    #[test]
    fn sweep_expired_removes_only_overdue_entries() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-overdue".to_string(), 1000);
        waits.register(2, 1, "t-fresh".to_string(), 5000);

        let expired = waits.sweep_expired(2000);
        assert_eq!(expired, vec![(1, "t-overdue".to_string())]);
        assert_eq!(waits.len(), 1);
        assert_eq!(waits.resolve(2), Some((1, "t-fresh".to_string())));
    }

    #[test]
    fn sweep_expired_is_noop_when_nothing_overdue() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-1".to_string(), 5000);
        assert!(waits.sweep_expired(1000).is_empty());
        assert_eq!(waits.len(), 1);
    }

    #[test]
    fn sweep_expired_deadline_equal_to_now_is_expired() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-1".to_string(), 1000);
        assert_eq!(waits.sweep_expired(1000), vec![(1, "t-1".to_string())]);
    }
}
