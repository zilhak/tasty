//! Actual queued reads retain their delivery identity beyond confirmed acknowledgement.
use super::*;
use crate::{TaskScope, TaskService};
use std::sync::OnceLock;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tasty_agent::task::{Completion, TaskCreateOpts, TaskPurgeFilter};
use tasty_agent::{OnFailure, Task, TaskCommand, TaskResult, TaskState, TaskStore};
use tasty_memory::testing::faults::{Faults, FaultyStore};
use tasty_memory::writer::{MemoryWriter, WriterStore, settle};

const WAIT: Duration = Duration::from_secs(3);
struct Fixture {
    writer: MemoryWriter<WriterStore>,
    memory: Arc<Mutex<dyn MemoryStorage>>,
    faults: Arc<Faults>,
    service: TaskService,
    scope: TaskScope,
}
impl Fixture {
    fn new() -> Self {
        let faults = Arc::new(Faults::default().writing(terminal::PREFIX));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(FaultyStore::new(faults.clone())));
        let changes = Arc::new(tasty_memory::writer::ChangeFeed::default());
        let writer = tasty_memory::writer::spawn(
            WriterStore::shared(memory.clone()),
            changes.clone(),
            tasty_utils::batch_writer::Limits::default(),
            Arc::new(tasty_utils::batch_writer::SystemClock),
            Arc::new(|| {}),
        )
        .unwrap();
        let service = TaskService::new(
            MemoryAccess::writer(writer.handle(), changes),
            Arc::new(OnceLock::new()),
            Arc::new(crate::completion::fixture::Resolver::default()),
        );
        let scope = TaskScope::new(service.runner_registry().clone());
        Self {
            writer,
            memory,
            faults,
            service,
            scope,
        }
    }
    fn read(&self) -> Confirmed {
        self.read_result().unwrap()
    }
    fn read_result(&self) -> tasty_agent::Result<Confirmed> {
        let target = Target {
            hub: self.scope.waker_hub().clone(),
            workspace: 1,
            settle: None,
        };
        let mut ticket = self
            .service
            .memory_access()
            .submit(MemoryRequest::read(0, move |mem| {
                Ok(load(mem, vec![target.clone()]))
            }))
            .unwrap();
        settle(ticket.wait_timeout(WAIT).unwrap())
            .unwrap()
            .into_value::<tasty_agent::Result<Confirmed>>()
            .unwrap()
    }
    fn drain(&self) {
        let until = Instant::now() + WAIT;
        while self.service.memory_access().detached_len() != 0 {
            self.service.memory_access().reap_detached();
            assert!(
                Instant::now() < until,
                "owned acknowledgement did not settle"
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    fn pending(&self) -> usize {
        terminal::pending(&*self.memory.lock().unwrap(), 1)
            .unwrap()
            .len()
    }
    fn events(&self) -> usize {
        self.scope
            .event_queue()
            .take_pending()
            .0
            .into_iter()
            .filter(|e| matches!(e, crate::event_feed::AgentEvent::TaskFinished { .. }))
            .count()
    }
    fn seed(&self) {
        let mut mem = self.memory.lock().unwrap();
        let task: Task = serde_json::from_value(serde_json::json!({
            "id":"p", "workspace_id":1,"name":"p","created_at":0,
            "command":{"kind":"custom","ipc_method":"system.ping","params":{}},
            "state":{"kind":"succeeded"},"on_failure":{"kind":"abort"}
        }))
        .unwrap();
        TaskStore::new(&mut *mem, HOST_OWNER, self.scope.agent_seq())
            .put(&task)
            .unwrap();
        assert_eq!(terminal::pending(&*mem, 1).unwrap().len(), 1);
    }
    fn retained(&self) -> usize {
        self.scope.waker_hub().retained_delivery_count()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.writer.begin_shutdown();
        let until = Instant::now() + WAIT;
        while self.writer.poll_shutdown() != 0 {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn two_queued_read_outputs_share_claim_after_confirmed_ack() {
    let f = Fixture::new();
    f.seed();
    let earlier = f.read();
    let delayed = f.read();
    deliver(earlier, f.service.memory_access());
    f.drain();
    assert_eq!(f.pending(), 0);
    assert_eq!(f.events(), 1);
    // Ack confirmation must free the registry even while the delayed output lives.
    assert_eq!(f.retained(), 0);
    deliver(delayed, f.service.memory_access());
    f.drain();
    assert_eq!(
        f.events(),
        0,
        "old read must retain its already-delivered claim"
    );
    assert_eq!(f.retained(), 0);
}

#[test]
fn confirmed_complete_ack_and_purge_do_not_retain_history() {
    let f = Fixture::new();
    for n in 0..64 {
        let task = f
            .service
            .task_create(
                &f.scope,
                TaskCreateOpts {
                    workspace_id: 1,
                    name: format!("cycle-{n}"),
                    command: TaskCommand::Custom {
                        ipc_method: "system.ping".into(),
                        params: serde_json::json!({}),
                        poll: None,
                    },
                    depends_on: vec![],
                    on_failure: OnFailure::Abort,
                    metadata: serde_json::json!({}),
                    now_ms: 1,
                },
                false,
            )
            .unwrap();
        f.service
            .task_set_state(&f.scope, 1, &task.id, TaskState::Running, 2)
            .unwrap();
        f.service
            .task_complete(
                &f.scope,
                1,
                &task.id,
                Completion::succeeded(
                    None,
                    TaskResult {
                        exit_code: Some(0),
                        output: None,
                        error: None,
                    },
                ),
                3,
            )
            .unwrap();
        f.drain();
        assert_eq!(f.pending(), 0);
        assert_eq!(f.events(), 1);
        f.service
            .task_purge(
                &f.scope,
                1,
                TaskPurgeFilter {
                    states: Some(vec!["succeeded".into()]),
                    older_than_ms: None,
                    now_ms: 4,
                },
                false,
            )
            .unwrap();
        f.drain();
        assert!(f.service.task_list(&f.scope, 1).unwrap().is_empty());
    }
    assert_eq!(
        f.retained(),
        0,
        "acknowledged UUID history must be released"
    );
}

#[test]
fn failed_and_unknown_ack_keep_claim_until_ordered_confirmation() {
    for after_commit in [false, true] {
        let f = Fixture::new();
        f.seed();
        let earlier = f.read();
        let original = earlier.deliveries[0].notice.clone();
        let delayed = f.read();
        let fault = if after_commit {
            &f.faults.units_after_writing
        } else {
            &f.faults.units_before_body
        };
        Faults::arm(fault, 1);
        deliver(earlier, f.service.memory_access());
        f.drain();
        assert_eq!(fault.load(Ordering::SeqCst), 0);
        assert_eq!(f.events(), 1);
        assert_eq!(f.pending(), usize::from(!after_commit));
        assert_eq!(f.retained(), 1, "unconfirmed ack must retain claim");
        // A malformed pending row must not be interpreted as confirmed absence.
        let key = format!("{}{}", terminal::PREFIX, original.id);
        {
            let mut mem = f.memory.lock().unwrap();
            let mut unknown = original.clone();
            unknown.schema = 999;
            mem.put(
                HOST_OWNER,
                &tasty_memory::Scope::Workspace(1),
                &key,
                &tasty_memory::MemoryValue::Json(serde_json::to_value(unknown).unwrap()),
                &Default::default(),
            )
            .unwrap();
        }
        assert!(f.read_result().is_err());
        assert_eq!(f.retained(), 1);
        {
            let mut mem = f.memory.lock().unwrap();
            if after_commit {
                mem.delete(HOST_OWNER, &tasty_memory::Scope::Workspace(1), &key, None)
                    .unwrap();
            } else {
                mem.put(
                    HOST_OWNER,
                    &tasty_memory::Scope::Workspace(1),
                    &key,
                    &tasty_memory::MemoryValue::Json(serde_json::to_value(original).unwrap()),
                    &Default::default(),
                )
                .unwrap();
            }
        }
        // A strong read confirms absence after an unanswered committed ack, or shares the
        // pending identity before an idempotent ack. Neither may publish another event.
        let retry = f.read();
        deliver(retry, f.service.memory_access());
        f.drain();
        assert_eq!(f.pending(), 0);
        assert_eq!(f.retained(), 0);
        deliver(delayed, f.service.memory_access());
        f.drain();
        assert_eq!(f.events(), 0);
        assert_eq!(f.retained(), 0);
    }
}
