//! 훅 완료 신호 → agent task 매핑.
//!
//! 완료 전략 레지스트리(`src/completion_strategy/`)의 push 형 전략이 훅 핸들러
//! (`notify_via: HookHandlerId`)로 완료를 보고할 때, 그 훅이 "어느 task 의"
//! 완료인지는 훅 정의 자체엔 담을 수 없다 — owner 가 등록 시 고정되고
//! (`HookHandlerAction` 의 데이터/흐름 분리 불변식), task id 를 담을 슬롯이
//! 없다. 그 해법이 이 모듈이다: 러너(host executor)가
//! `AwaitExternal` 핸들로 dispatch 하며 `register` 하고, `PendingHostEvent::
//! HookFired` 소비부(`Core::resolve_hook_task_wait`)가 훅 발화마다 조회해
//! 매칭되면 task 를 마감한다.
//!
//! 등록자는 runner thread(`HostExecutor::dispatch_command` 의 push-kind 분기)다
//! — `Custom` task 가 push-kind 완료 전략을 참조하면 그
//! 시점에 `register` 를 호출한다. 소비부(`resolve`)는 실제로 살아 있는
//! `HookFired` 이벤트 스트림에 매 발화마다 실행되는 코드다 — 등록되지 않은
//! hook_id 는 안전한 no-op.

use std::collections::HashMap;
use std::sync::Mutex;

use tasty_agent::task::TaskId;

/// 이 레지스트리 락의 poison 복구 공용 보고 좌표(첫-1 회). 인스턴스는 하나다.
const HOOK_WAIT_WHAT: &str = "agent hook-wait registry";
static HOOK_WAIT_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// hook_id → (workspace_id, task_id, deadline_ms) 1회성 매핑. `deadline_ms` 는
/// push 전략의 필수 timeout(§B-3/§C-3) — 훅 보고가 유실돼도 task 가 영구
/// Running 에 남지 않도록 `sweep_expired` 가 강제 마감한다.
pub(crate) struct HookTaskWaits {
    inner: Mutex<HashMap<u64, (u32, TaskId, u64)>>,
}

impl HookTaskWaits {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// `task_id` 가 `hook_id` 의 완료를 기다리고 있음을 등록. 같은 hook_id 에
    /// 재등록하면 이전 대기를 덮어쓴다(마지막 등록자 승리 — 한 hook 은 한 번에
    /// 한 task 만 기다릴 수 있다는 전제). `deadline_ms` 는 unix epoch ms 절대
    /// 시각 — push 전략의 `timeout_ms` 로부터 호출자가 계산해 넘긴다.
    ///
    /// 등록자는 runner thread(`HostExecutor::dispatch_command` 의 push-kind
    /// 분기) — `RunnerContext.hook_task_waits` 로 이 구조체의
    /// `Arc` 를 그대로 공유해 `Core` 를 거치지 않고 직접 호출한다(runner thread
    /// 는 main thread 소유 `Core`/`CoreState` 에 접근할 수 없다 — `task_waker_hub`
    /// 공유와 동형).
    pub fn register(&self, hook_id: u64, workspace_id: u32, task_id: TaskId, deadline_ms: u64) {
        let mut guard =
            crate::poison::recover_mutex(self.inner.lock(), HOOK_WAIT_WHAT, &HOOK_WAIT_POISONED);
        guard.insert(hook_id, (workspace_id, task_id, deadline_ms));
    }

    /// `hook_id` 에 대기 중인 task 가 있으면 **제거하고** 반환(1회성 소비) —
    /// 조회 후 잔존시키면 같은 hook_id 의 재발화(예: 같은 훅이 다른 목적으로
    /// 재등록된 경우)가 이미 끝난 task 를 다시 매칭하는 오탐 위험이 있다.
    pub fn resolve(&self, hook_id: u64) -> Option<(u32, TaskId)> {
        let mut guard =
            crate::poison::recover_mutex(self.inner.lock(), HOOK_WAIT_WHAT, &HOOK_WAIT_POISONED);
        guard.remove(&hook_id).map(|(ws, tid, _)| (ws, tid))
    }

    /// `now_ms` 기준 deadline 이 지난 항목을 전부 제거하고 반환한다(§C-3 timeout
    /// 안전망). 워크스페이스 무관 — 실행 중인 아무 runner thread 의 tick 이든
    /// 이 sweep 을 돌려도 안전하다(`runner_thread.rs::expire_overdue_hook_waits`
    /// 참조): 제거가 원자적이라 여러 thread 가 동시에 sweep 해도 항목이 중복
    /// 처리되지 않는다.
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
        // hook_id 2 의 대기는 hook_id 1 소비와 무관하게 살아있어야 한다.
        assert_eq!(waits.len(), 1);
        assert_eq!(waits.resolve(2), Some((1, "t-2".to_string())));
    }

    /// §C-3 timeout 안전망 — deadline 이 지난 항목만 sweep 되고, 아직 안 지난
    /// 항목은 살아남는다.
    #[test]
    fn sweep_expired_removes_only_overdue_entries() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-overdue".to_string(), 1000);
        waits.register(2, 1, "t-fresh".to_string(), 5000);

        let expired = waits.sweep_expired(2000);
        assert_eq!(expired, vec![(1, "t-overdue".to_string())]);
        assert_eq!(waits.len(), 1);
        // 아직 안 지난 항목은 그대로 resolve 가능.
        assert_eq!(waits.resolve(2), Some((1, "t-fresh".to_string())));
    }

    #[test]
    fn sweep_expired_is_noop_when_nothing_overdue() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-1".to_string(), 5000);
        assert!(waits.sweep_expired(1000).is_empty());
        assert_eq!(waits.len(), 1);
    }

    /// deadline 이 정확히 now 와 같으면(경계값) 만료로 취급한다.
    #[test]
    fn sweep_expired_deadline_equal_to_now_is_expired() {
        let waits = HookTaskWaits::new();
        waits.register(1, 1, "t-1".to_string(), 1000);
        assert_eq!(waits.sweep_expired(1000), vec![(1, "t-1".to_string())]);
    }
}
