//! Reconcile committed terminal obligations without repeating the state mutation.
use std::sync::{Arc, Mutex};

use tasty_agent::task::terminal::{self, TerminalNotice};
use tasty_memory::writer::{MemoryAccess, MemoryRequest};
use tasty_memory::{HOST_OWNER, MemoryStorage};

use crate::runner_host::RunnerContext;
use crate::task_waker::{TaskWakerHub, TerminalSnapshot};

#[derive(Clone)]
struct Target {
    hub: Arc<TaskWakerHub>,
    workspace: u32,
    settle: Option<RunnerContext>,
}

/// Request-local delivery destinations survive the loss of that request's output.
#[derive(Clone, Default)]
pub struct TerminalRecovery(Arc<Mutex<Vec<Target>>>);

static RECOVERY_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

impl TerminalRecovery {
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Target>> {
        tasty_utils::poison::recover_mutex(
            self.0.lock(),
            "terminal recovery targets",
            &RECOVERY_POISONED,
        )
    }
    pub(crate) fn track(
        &self,
        hub: &Arc<TaskWakerHub>,
        workspace: u32,
        settle: Option<RunnerContext>,
    ) {
        let mut targets = self.lock();
        if let Some(old) = targets
            .iter_mut()
            .find(|t| t.workspace == workspace && Arc::ptr_eq(&t.hub, hub))
        {
            if old.settle.is_none() {
                old.settle = settle;
            }
        } else {
            targets.push(Target {
                hub: hub.clone(),
                workspace,
                settle,
            });
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.targets().is_empty()
    }

    fn targets(&self) -> Vec<Target> {
        self.lock().clone()
    }

    /// Queue a strong read after the submitted mutation, including an unanswered mutation.
    /// Targets are read when this request executes, after the mutation has populated them.
    pub fn schedule(&self, access: &MemoryAccess) {
        self.schedule_attempt(access, 3);
    }

    fn schedule_attempt(&self, access: &MemoryAccess, remaining: u8) {
        let plan = self.clone();
        let retry = self.clone();
        let access_done = access.clone();
        access.submit_detached(
            MemoryRequest::read(0, move |mem| Ok(load(mem, plan.targets()))),
            "terminal obligation reconciliation",
            Some(Box::new(move |out| {
                match out.into_value::<tasty_agent::Result<Vec<Delivery>>>() {
                    Some(Ok(deliveries)) => deliver(deliveries, &access_done),
                    Some(Err(tasty_agent::AgentError::Memory(error))) if remaining > 1 => {
                        tracing::warn!(%error, remaining, "retrying terminal confirmation read");
                        retry.schedule_attempt(&access_done, remaining - 1);
                    }
                    Some(Err(error)) => {
                        tracing::warn!(%error, "terminal obligations remain pending")
                    }
                    None => tracing::error!("unexpected terminal reconciliation output"),
                }
            })),
        );
    }
}

struct Delivery {
    target: Target,
    notice: TerminalNotice,
    current: Option<TerminalSnapshot>,
    settle_task: Option<tasty_agent::Task>,
}

fn load(mem: &dyn MemoryStorage, targets: Vec<Target>) -> tasty_agent::Result<Vec<Delivery>> {
    let mut deliveries = Vec::new();
    for target in targets {
        for notice in terminal::pending(mem, target.workspace)? {
            let row = tasty_agent::TaskStore::read_record(mem, target.workspace, &notice.task.id)?;
            let same_record = match row.as_ref() {
                Some((task, revision)) => {
                    *revision == notice.revision && terminal::TerminalTask::of(task) == notice.task
                }
                None => false,
            };
            let settle_task = row
                .as_ref()
                .filter(|_| same_record)
                .map(|(task, _)| task.clone());
            let current = row
                .filter(|(task, _)| task.state.is_terminal())
                .map(|(task, revision)| TerminalSnapshot::of(&task, Some(revision)));
            deliveries.push(Delivery {
                target: target.clone(),
                notice,
                current,
                settle_task,
            });
        }
    }
    Ok(deliveries)
}

fn deliver(deliveries: Vec<Delivery>, access: &MemoryAccess) {
    for Delivery {
        target,
        notice,
        current,
        settle_task,
    } in deliveries
    {
        let first = target.hub.fire_once(
            &notice.id,
            target.workspace,
            &notice.task.id,
            TerminalSnapshot {
                state: notice.task.state.clone(),
                result: notice.task.result.clone(),
                attempt_id: notice.task.attempt.as_ref().map(|a| a.id.clone()),
                skip: notice.task.skip.clone(),
                revision: Some(notice.revision),
            },
            current,
        );
        if first
            && let Some(task) = settle_task
            && let Some(ctx) = target.settle
        {
            crate::runner_thread::settle_ended_tasks_in_background(
                ctx,
                target.workspace,
                vec![task],
            );
        }
        // This id names an immutable obligation. A lost ack may replay the read, but fire_once
        // suppresses another event/settle within this hub; no newer task round can be erased.
        let id = notice.id;
        access.submit_detached(
            MemoryRequest::unit(0, move |mem| {
                terminal::acknowledge(mem, HOST_OWNER, target.workspace, &id)
            }),
            "terminal obligation acknowledgement",
            None,
        );
    }
}

pub(crate) fn reconcile(ctx: &RunnerContext, workspace: u32) {
    let target = Target {
        hub: ctx.task_waker_hub.clone(),
        workspace,
        settle: None,
    };
    let mut ticket = match ctx.memory.submit(MemoryRequest::read(0, move |mem| {
        Ok(load(mem, vec![target.clone()]))
    })) {
        Ok(ticket) => ticket,
        Err(error) => {
            tracing::warn!(%error, "terminal confirmation read refused");
            return;
        }
    };
    let completion = loop {
        if let Some(completion) = ticket.wait_timeout(std::time::Duration::from_secs(1)) {
            break completion;
        }
    };
    match tasty_memory::writer::settle(completion) {
        Ok(output) => match output.into_value::<tasty_agent::Result<Vec<Delivery>>>() {
            Some(Ok(deliveries)) => deliver(deliveries, &ctx.memory),
            Some(Err(error)) => tracing::warn!(%error, "terminal obligations remain pending"),
            None => tracing::error!("unexpected terminal confirmation output"),
        },
        Err(error) => tracing::warn!(?error, "terminal reconciliation could not be confirmed"),
    }
}
