//! 완료 훅 ID를 대기 중인 작업에 연결한다.
//! 러너가 공유 Arc에 등록하고 호스트의 HookFired 소비자가 찾아 작업을 마친다.

use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use tasty_agent::task::TaskId;

use crate::task_waker::TaskWakerHub;

const HOOK_WAIT_WHAT: &str = "agent hook-wait registry";
static HOOK_WAIT_POISONED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// hook ID별 일회성 대기. 보고가 없으면 sweep_expired로 만료 항목을 회수해 호출자가 처리한다.
/// 재시작으로 hook 매핑을 잃은 대기는 기한만 남겨 같은 만료 회수에 포함한다.
pub struct HookTaskWaits {
    inner: Mutex<Waits>,
}

#[derive(Default)]
struct Waits {
    by_hook: HashMap<u64, HookWait>,
    /// 재시작 뒤 복원한 대기. hook ID는 재시작 뒤 다시 발급될 수 있어 훅으로 찾지 않는다.
    restored: HashMap<(u32, TaskId), HookWait>,
}

struct HookWait {
    workspace: u32,
    task: TaskId,
    deadline: u64,
    owner: Option<HookWaitOwner>,
    /// 훅을 건 회차. 늦게 온 훅이 다음 회차를 끝내지 않게 완료 보고에 싣는다.
    attempt: Option<String>,
}

/// A global sweep can run in another engine; fallback IDs and notifications retain their owner.
pub(crate) struct HookWaitOwner {
    pub(crate) agent_seq: Arc<AtomicU64>,
    pub(crate) completion: Arc<TaskWakerHub>,
}

pub(crate) struct ExpiredWait {
    pub(crate) workspace: u32,
    pub(crate) task: TaskId,
    pub(crate) owner: Option<HookWaitOwner>,
    pub(crate) attempt: Option<String>,
    /// 재시작 뒤 기한만 복원한 대기다. 그 사이 외부 보고로 끝났을 수 있다.
    pub(crate) restored: bool,
}

/// 훅으로 찾은 대기 하나.
pub(crate) struct ResolvedWait {
    pub(crate) workspace: u32,
    pub(crate) task: TaskId,
    pub(crate) owner: Option<HookWaitOwner>,
    pub(crate) attempt: Option<String>,
}

impl HookTaskWaits {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Waits::default()),
        }
    }

    /// 같은 hook ID는 마지막 등록으로 바꾼다. deadline_ms는 Unix epoch 기준 절대 밀리초다.
    /// 러너는 메인 스레드의 Core를 직접 사용하지 않고 공유 Arc로 등록한다.
    pub fn register(&self, hook_id: u64, workspace_id: u32, task_id: TaskId, deadline_ms: u64) {
        self.insert(hook_id, workspace_id, task_id, deadline_ms, None, None);
    }

    /// 한 번 반환한 매핑은 지워 같은 훅의 재발생이 끝난 작업과 다시 연결되지 않게 한다.
    pub fn resolve(&self, hook_id: u64) -> Option<(u32, TaskId)> {
        self.resolve_owned(hook_id)
            .map(|wait| (wait.workspace, wait.task))
    }
    pub(crate) fn resolve_owned(&self, hook_id: u64) -> Option<ResolvedWait> {
        let mut guard = tasty_utils::poison::recover_mutex(
            self.inner.lock(),
            HOOK_WAIT_WHAT,
            &HOOK_WAIT_POISONED,
        );
        guard.by_hook.remove(&hook_id).map(|wait| ResolvedWait {
            workspace: wait.workspace,
            task: wait.task,
            owner: wait.owner,
            attempt: wait.attempt,
        })
    }

    /// Another workspace's runner may expire this wait even after its own runner stopped.
    pub(crate) fn register_owned(
        &self,
        hook_id: u64,
        workspace: u32,
        task: TaskId,
        deadline: u64,
        owner: HookWaitOwner,
        attempt: Option<String>,
    ) {
        self.insert(hook_id, workspace, task, deadline, Some(owner), attempt);
    }

    /// 재시작 뒤 복원한 대기의 기한을 등록한다. 같은 작업을 기다리는 hook 매핑이 아직 있으면
    /// 그 매핑이 만료를 맡으므로 등록하지 않는다. 같은 작업의 재등록은 마지막 값으로 바꾼다.
    pub(crate) fn register_restored(
        &self,
        workspace: u32,
        task: TaskId,
        deadline: u64,
        owner: HookWaitOwner,
        attempt: Option<String>,
    ) {
        let mut guard = tasty_utils::poison::recover_mutex(
            self.inner.lock(),
            HOOK_WAIT_WHAT,
            &HOOK_WAIT_POISONED,
        );
        if guard
            .by_hook
            .values()
            .any(|wait| wait.workspace == workspace && wait.task == task)
        {
            return;
        }
        guard.restored.insert(
            (workspace, task.clone()),
            HookWait {
                workspace,
                task,
                deadline,
                owner: Some(owner),
                attempt,
            },
        );
    }

    fn insert(
        &self,
        hook_id: u64,
        workspace: u32,
        task: TaskId,
        deadline: u64,
        owner: Option<HookWaitOwner>,
        attempt: Option<String>,
    ) {
        let mut guard = tasty_utils::poison::recover_mutex(
            self.inner.lock(),
            HOOK_WAIT_WHAT,
            &HOOK_WAIT_POISONED,
        );
        // 새 회차가 hook을 걸면 이전 회차의 복원 기한은 더 이상 이 작업을 대표하지 않는다.
        guard.restored.remove(&(workspace, task.clone()));
        guard.by_hook.insert(
            hook_id,
            HookWait {
                workspace,
                task,
                deadline,
                owner,
                attempt,
            },
        );
    }

    /// deadline <= now_ms인 항목을 락 안에서 제거한다. 여러 러너가 호출해도 중복 회수하지 않는다.
    pub fn sweep_expired(&self, now_ms: u64) -> Vec<(u32, TaskId)> {
        self.take_expired(now_ms)
            .into_iter()
            .map(|wait| (wait.workspace, wait.task))
            .collect()
    }

    pub(crate) fn take_expired(&self, now_ms: u64) -> Vec<ExpiredWait> {
        let mut guard = tasty_utils::poison::recover_mutex(
            self.inner.lock(),
            HOOK_WAIT_WHAT,
            &HOOK_WAIT_POISONED,
        );
        let ids: Vec<_> = guard
            .by_hook
            .iter()
            .filter(|(_, wait)| wait.deadline <= now_ms)
            .map(|(id, _)| *id)
            .collect();
        let keys: Vec<_> = guard
            .restored
            .iter()
            .filter(|(_, wait)| wait.deadline <= now_ms)
            .map(|(key, _)| key.clone())
            .collect();
        let hooked = ids
            .into_iter()
            .filter_map(|id| guard.by_hook.remove(&id))
            .map(|wait| (wait, false))
            .collect::<Vec<_>>();
        let restored = keys
            .into_iter()
            .filter_map(|key| guard.restored.remove(&key))
            .map(|wait| (wait, true));
        hooked
            .into_iter()
            .chain(restored)
            .map(|(wait, restored)| ExpiredWait {
                workspace: wait.workspace,
                task: wait.task,
                owner: wait.owner,
                attempt: wait.attempt,
                restored,
            })
            .collect()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        let guard = tasty_utils::poison::recover_mutex(
            self.inner.lock(),
            HOOK_WAIT_WHAT,
            &HOOK_WAIT_POISONED,
        );
        guard.by_hook.len() + guard.restored.len()
    }

    /// `len` 이 시험 전용이므로 짝이 되는 이 메서드도 같은 게이트를 쓴다.
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
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
    fn owned_wait_retains_origin_until_consumed_or_replaced() {
        let waits = HookTaskWaits::new();
        let completion = Arc::new(TaskWakerHub::new());
        let agent_seq = Arc::new(AtomicU64::new(42));
        let owner = || HookWaitOwner {
            agent_seq: agent_seq.clone(),
            completion: completion.clone(),
        };
        waits.register_owned(1, 7, "original".into(), 100, owner(), None);
        let expired = waits.take_expired(100).pop().unwrap();
        assert_eq!((expired.workspace, expired.task.as_str()), (7, "original"));
        let original = expired.owner.unwrap();
        assert!(Arc::ptr_eq(&original.completion, &completion));
        assert!(Arc::ptr_eq(&original.agent_seq, &agent_seq));
        assert!(waits.take_expired(100).is_empty());
        drop(original);

        waits.register_owned(1, 7, "resolved".into(), 100, owner(), None);
        assert_eq!(waits.resolve(1), Some((7, "resolved".into())));
        assert_eq!(Arc::strong_count(&completion), 1);
        assert_eq!(Arc::strong_count(&agent_seq), 1);

        waits.register_owned(1, 7, "replaced".into(), 100, owner(), None);
        waits.register(1, 8, "legacy".into(), 100);
        assert_eq!(Arc::strong_count(&completion), 1);
        assert_eq!(Arc::strong_count(&agent_seq), 1);
        let legacy = waits.take_expired(100).pop().unwrap();
        assert_eq!((legacy.workspace, legacy.task.as_str()), (8, "legacy"));
        assert!(legacy.owner.is_none());
    }

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

    fn owner() -> HookWaitOwner {
        HookWaitOwner {
            agent_seq: Arc::new(AtomicU64::new(1)),
            completion: Arc::new(TaskWakerHub::new()),
        }
    }

    #[test]
    fn a_restored_deadline_expires_once_and_is_not_resolved_by_a_hook() {
        let waits = HookTaskWaits::new();
        waits.register_restored(1, "t-1".into(), 1000, owner(), Some("a1".into()));
        waits.register_restored(1, "t-1".into(), 1000, owner(), Some("a1".into()));
        assert_eq!(waits.len(), 1, "같은 작업의 재등록은 하나로 남아야 한다");
        assert_eq!(waits.resolve(0), None);
        assert!(waits.take_expired(999).is_empty());
        let expired = waits.take_expired(1000);
        assert_eq!(expired.len(), 1);
        assert!(expired[0].restored);
        assert_eq!(expired[0].attempt.as_deref(), Some("a1"));
        assert!(
            waits.take_expired(1000).is_empty(),
            "두 번 회수하면 안 된다"
        );
    }

    #[test]
    fn a_live_hook_wait_keeps_the_deadline_and_a_new_hook_replaces_a_restored_one() {
        let waits = HookTaskWaits::new();
        waits.register(5, 1, "t-1".into(), 2000);
        waits.register_restored(1, "t-1".into(), 1000, owner(), None);
        assert_eq!(waits.len(), 1, "hook 매핑이 있으면 기한을 따로 두지 않는다");

        waits.register_restored(1, "t-2".into(), 1000, owner(), None);
        waits.register(6, 1, "t-2".into(), 3000);
        assert!(
            waits.take_expired(1500).is_empty(),
            "새 회차의 hook 이 이전 회차의 복원 기한을 대신해야 한다"
        );
        assert_eq!(waits.len(), 2);
    }
}
