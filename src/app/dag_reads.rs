//! App owns bounded DAG workers and validates live workspace ownership before publishing results.
use crate::runtime::dag_query::{
    BindingKey, PendingRead, ReadError, ReadResult, ReadTicket, WorkspaceSnapshot,
};
use crate::runtime::engine_session::EngineId;
use std::sync::{
    Arc, Mutex, TryLockError,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::thread::JoinHandle;
use std::time::Duration;

const MAX_RUNNING: usize = 4;
#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    workspace: u32,
    engine: EngineId,
    binding: BindingKey,
}
fn target(registry: &super::engine_registry::EngineRegistry, workspace: u32) -> Option<Target> {
    let owner = registry
        .all_sessions()
        .find(|session| session.core_state.has_workspace(workspace))?;
    Some(Target {
        workspace,
        engine: owner.id,
        binding: BindingKey::capture(owner.journal_binding.as_ref()),
    })
}
struct Job {
    engine: EngineId,
    targets: Vec<Target>,
    request: Option<PendingRead>,
    receiver: mpsc::Receiver<Option<ReadResult>>,
    handle: JoinHandle<()>,
}
impl Job {
    fn publish(&mut self, current: &impl Fn(&Target) -> Option<(bool, bool)>) -> bool {
        if self.request.is_none() {
            return false;
        }
        let result = match self.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("DAG read worker disconnected".into()))
            }
        };
        let request = self.request.take().expect("unconsumed DAG result");
        if let Some(result) = result {
            let result = result.and_then(|mut snapshots| {
                for target in &self.targets {
                    let runner = current(target).ok_or(ReadError::Retired)?;
                    if let Some(snapshot) = snapshots.iter_mut().find(|s| s.id == target.workspace)
                    {
                        snapshot.runner = runner;
                    }
                }
                Ok(snapshots)
            });
            request.complete(result);
        }
        true
    }
}
pub(crate) struct DagReads {
    jobs: Vec<Job>,
    stopping: Arc<AtomicBool>,
    wake: tasty_terminal::Waker,
    cursor: usize,
}
impl DagReads {
    pub(crate) fn new(wake: tasty_terminal::Waker) -> Self {
        Self {
            jobs: Vec::new(),
            stopping: Arc::new(AtomicBool::new(false)),
            wake,
            cursor: 0,
        }
    }
    fn available(&self, engine: EngineId) -> bool {
        !self.stopping.load(Ordering::Acquire)
            && self.jobs.len() < MAX_RUNNING
            && !self.jobs.iter().any(|job| job.engine == engine)
    }
    fn start(
        &mut self,
        engine: EngineId,
        request: PendingRead,
        memory: Arc<Mutex<dyn tasty_memory::MemoryStorage>>,
        sequence: Arc<AtomicU64>,
        targets: Vec<Target>,
    ) {
        if !self.available(engine) {
            request.complete(Err("DAG read capacity exhausted".into()));
            return;
        }
        let stopping = self.stopping.clone();
        let wake = self.wake.clone();
        let ticket = request.ticket();
        let workspaces = request.workspaces.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("dag-read".into())
            .spawn(move || {
                let result = read(
                    &ticket,
                    &workspaces,
                    memory.as_ref(),
                    sequence.as_ref(),
                    stopping.as_ref(),
                );
                if sender.send(result).is_err() {
                    tracing::debug!("DAG read owner closed before completion");
                }
                wake();
            }) {
            Ok(handle) => self.jobs.push(Job {
                engine,
                targets,
                request: Some(request),
                receiver,
                handle,
            }),
            Err(error) => request.complete(Err(error.to_string().into())),
        }
    }
    fn reap(&mut self, current: impl Fn(&Target) -> Option<(bool, bool)>) -> Vec<EngineId> {
        let mut completed = Vec::new();
        let mut i = 0;
        while i < self.jobs.len() {
            if self.jobs[i].publish(&current) {
                completed.push(self.jobs[i].engine);
            }
            // A result can race this first poll. Keep the join receipt until its result is consumed.
            if self.jobs[i].request.is_none() && self.jobs[i].handle.is_finished() {
                let job = self.jobs.swap_remove(i);
                if job.handle.join().is_err() {
                    tracing::warn!("DAG read worker panicked");
                }
            } else {
                i += 1;
            }
        }
        completed
    }
    pub(crate) fn begin_shutdown(&mut self) {
        self.stopping.store(true, Ordering::Release);
    }
    pub(crate) fn poll_shutdown(&mut self) -> usize {
        self.begin_shutdown();
        self.reap(|_| None);
        self.jobs.len()
    }
}
impl Drop for DagReads {
    fn drop(&mut self) {
        let remaining = self.poll_shutdown();
        if remaining != 0 {
            tracing::warn!(remaining, "DAG reads remain unjoined at App drop");
        }
    }
}
fn read(
    ticket: &ReadTicket,
    workspaces: &[u32],
    memory: &Mutex<dyn tasty_memory::MemoryStorage>,
    sequence: &AtomicU64,
    stopping: &AtomicBool,
) -> Option<ReadResult> {
    let mut snapshots = Vec::new();
    for workspace in workspaces {
        let tasks = match read_tasks(ticket, memory, sequence, stopping, *workspace)? {
            Ok(tasks) => tasks,
            Err(error) => return Some(Err(error.into())),
        };
        let dags = tasty_agent::group_tasks_into_dags(&tasks);
        snapshots.push(WorkspaceSnapshot {
            id: *workspace,
            tasks,
            dags,
            runner: (false, false),
        });
    }
    (!stopping.load(Ordering::Acquire)).then_some(Ok(snapshots))
}
fn read_tasks(
    ticket: &ReadTicket,
    memory: &Mutex<dyn tasty_memory::MemoryStorage>,
    sequence: &AtomicU64,
    stopping: &AtomicBool,
    workspace: u32,
) -> Option<Result<Vec<tasty_agent::Task>, String>> {
    let mut store = loop {
        if stopping.load(Ordering::Acquire) || !ticket.is_live() {
            return None;
        }
        match memory.try_lock() {
            Ok(guard) => break guard,
            Err(TryLockError::Poisoned(poison)) => {
                break tasty_utils::poison::recover_poisoned(
                    poison,
                    tasty_memory::STORE_LOCK_WHAT,
                    &tasty_memory::STORE_LOCK_POISONED,
                );
            }
            Err(TryLockError::WouldBlock) => {
                #[cfg(test)]
                ticket.waiting.store(true, Ordering::Release);
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    let tasks = tasty_agent::TaskStore::new(&mut *store, tasty_memory::HOST_OWNER, sequence)
        .list(workspace)
        .map_err(|error| error.to_string());
    drop(store);
    Some(tasks)
}
impl super::App {
    pub(crate) fn poll_dag_reads(&mut self) {
        let registry = &self.engines;
        let completed = self.dag_reads.reap(|old| {
            (target(registry, old.workspace).as_ref() == Some(old)).then(|| {
                registry
                    .all_sessions()
                    .find(|session| session.id == old.engine)
                    .expect("validated DAG owner")
                    .task_scope
                    .runner_liveness(old.workspace)
            })
        });
        for engine in completed {
            if let Some(window) = self.engines.window_of(engine)
                && let Some(view) = self.view.views.get_mut(&window)
            {
                view.mark_dirty();
            }
        }
        let mut sessions: Vec<_> = self.engines.all_sessions().collect();
        if !sessions.is_empty() {
            let offset = self.dag_reads.cursor % sessions.len();
            sessions.rotate_left(offset);
            self.dag_reads.cursor = offset + 1;
        }
        for session in sessions {
            if self.dag_reads.available(session.id)
                && let Some(request) = session
                    .runtime
                    .dag_reads
                    .take(session.journal_binding.as_ref())
            {
                let targets: Option<Vec<_>> = request
                    .workspaces
                    .iter()
                    .map(|id| target(&self.engines, *id))
                    .collect();
                if let Some(targets) = targets {
                    self.dag_reads.start(
                        session.id,
                        request,
                        self.services.memory_arc(),
                        session.task_scope.agent_seq().clone(),
                        targets,
                    );
                } else {
                    request.complete(Err(ReadError::Retired));
                }
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::dag_query::DagReadQueue;
    fn memory() -> Arc<Mutex<dyn tasty_memory::MemoryStorage>> {
        Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().unwrap(),
        ))
    }
    fn await_busy(waiting: &AtomicBool) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !waiting.load(Ordering::Acquire) {
            assert!(
                std::time::Instant::now() < deadline,
                "worker never observed the held store lock"
            );
            std::thread::yield_now();
        }
    }
    fn join(owner: &mut DagReads) {
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while owner.poll_shutdown() != 0 {
            assert!(
                std::time::Instant::now() < deadline,
                "DAG workers did not join"
            );
            std::thread::yield_now();
        }
    }
    #[test]
    fn a_locked_store_leaves_display_polling_free_and_shutdown_joins_without_unlocking_it() {
        let (_, engine) = crate::state::tests::test_state();
        let memory = memory();
        let held = memory.lock().unwrap();
        let queue = DagReadQueue::default();
        let source = queue.source(None);
        let mut query = source.request(vec![1]);
        let mut owner = DagReads::new(Arc::new(|| {}));
        let request = queue.take(None).unwrap();
        let waiting = request.waiting.clone();
        owner.start(
            engine.id,
            request,
            memory.clone(),
            Arc::new(AtomicU64::new(0)),
            Vec::new(),
        );
        await_busy(&waiting);
        assert!(query.poll(source).is_none());
        assert_eq!(owner.jobs.len(), 1);
        join(&mut owner);
        assert!(query.poll(source).unwrap().is_err());
        drop(held);
    }
    #[test]
    fn dropping_the_receipt_cancels_a_waiting_read_without_stopping_the_service() {
        let (_, engine) = crate::state::tests::test_state();
        let memory = memory();
        let held = memory.lock().unwrap();
        let queue = DagReadQueue::default();
        let query = queue.source(None).request(vec![1]);
        let mut owner = DagReads::new(Arc::new(|| {}));
        let request = queue.take(None).unwrap();
        let waiting = request.waiting.clone();
        owner.start(
            engine.id,
            request,
            memory.clone(),
            Arc::new(AtomicU64::new(0)),
            Vec::new(),
        );
        await_busy(&waiting);
        drop(query);
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while !owner.jobs.is_empty() {
            owner.reap(|_| Some((false, false)));
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert!(!owner.stopping.load(Ordering::Acquire));
        drop(held);
        let mut next = queue.source(None).request(vec![2]);
        owner.start(
            engine.id,
            queue.take(None).unwrap(),
            memory,
            Arc::new(AtomicU64::new(0)),
            Vec::new(),
        );
        let result = loop {
            owner.reap(|_| Some((false, false)));
            if let Some(result) = next.poll(queue.source(None)) {
                break result.unwrap();
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(result[0].id, 2);
        join(&mut owner);
    }
    #[test]
    fn admission_never_spawns_more_than_four_workers() {
        let memory = memory();
        let held = memory.lock().unwrap();
        let mut owner = DagReads::new(Arc::new(|| {}));
        let queue = DagReadQueue::default();
        let mut queries = Vec::new();
        for _ in 0..MAX_RUNNING + 1 {
            let (_, engine) = crate::state::tests::test_state();
            queries.push(queue.source(None).request(vec![1]));
            owner.start(
                engine.id,
                queue.take(None).unwrap(),
                memory.clone(),
                Arc::new(AtomicU64::new(0)),
                Vec::new(),
            );
        }
        assert_eq!(owner.jobs.len(), MAX_RUNNING);
        assert!(
            queries
                .last_mut()
                .unwrap()
                .poll(queue.source(None))
                .unwrap()
                .is_err()
        );
        join(&mut owner);
        drop(held);
    }
    #[test]
    fn dag_results_use_the_actual_owner_and_reject_retired_target_generations() {
        let (_, origin) = crate::state::tests::test_state();
        let (_, destination) = crate::state::tests::test_state();
        assert_ne!(origin.id, destination.id);
        let destination_id = destination.id;
        let workspace = destination.core_state.workspace_at(0).unwrap().id;
        let mut registry = super::super::engine_registry::EngineRegistry::default();
        assert!(registry.insert_pending(destination).is_ok());
        let destination_target = target(&registry, workspace).unwrap();
        assert_eq!(destination_target.engine, destination_id);
        let queue = DagReadQueue::default();
        let mut owner = DagReads::new(Arc::new(|| {}));
        for retired in [false, true] {
            let mut query = queue.source(None).request(vec![workspace]);
            owner.start(
                origin.id,
                queue.take(None).unwrap(),
                memory(),
                Arc::new(AtomicU64::new(0)),
                vec![destination_target.clone()],
            );
            // Wait for the worker, then change the ownership observation before App publishes.
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            while !owner.jobs[0].handle.is_finished() {
                assert!(std::time::Instant::now() < deadline);
                std::thread::yield_now();
            }
            if retired {
                assert!(registry.begin_retiring_pending(destination_id));
                assert!(registry.finish_retiring(destination_id).is_some());
            }
            owner.reap(|old| {
                (target(&registry, workspace).as_ref() == Some(old)).then_some((true, false))
            });
            let result = query.poll(queue.source(None)).unwrap();
            if retired {
                assert!(matches!(result, Err(ReadError::Retired)));
            } else {
                let snapshot = result.unwrap().remove(0);
                assert_eq!(snapshot.id, workspace);
                assert_eq!(snapshot.runner, (true, false));
            }
            assert!(owner.jobs.is_empty());
        }
        join(&mut owner);
    }
}
