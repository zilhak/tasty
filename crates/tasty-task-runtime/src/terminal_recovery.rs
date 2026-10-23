//! Reconcile committed terminal obligations without repeating the state mutation.
use std::sync::{Arc, Mutex};

use tasty_agent::task::terminal::{self, TerminalNotice};
use tasty_memory::writer::{MemoryAccess, MemoryRequest};
use tasty_memory::{HOST_OWNER, MemoryStorage};

use crate::runner_host::RunnerContext;
use crate::runner_thread::settle::CapturedSettlement;
use crate::task_waker::{DeliveryToken, TaskWakerHub, TerminalSnapshot};

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
                match out.into_value::<tasty_agent::Result<Confirmed>>() {
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
    token: DeliveryToken,
    current: Option<TerminalSnapshot>,
}

#[derive(Default)]
struct Confirmed {
    deliveries: Vec<Delivery>,
    settlements: Vec<(RunnerContext, u32, Vec<CapturedSettlement>)>,
}

fn load(mem: &dyn MemoryStorage, targets: Vec<Target>) -> tasty_agent::Result<Confirmed> {
    let mut confirmed = Confirmed::default();
    for target in targets {
        let notices = terminal::pending(mem, target.workspace)?;
        // Capture shared claims before this ordered read returns. An ack may execute before
        // its output is consumed; delayed outputs must keep the already-delivered claim.
        let tokens = target.hub.capture_deliveries(
            target.workspace,
            notices.iter().map(|notice| notice.id.as_str()),
        );
        for (notice, token) in notices.into_iter().zip(tokens) {
            let row = tasty_agent::TaskStore::read_record(mem, target.workspace, &notice.task.id)?;
            let current = row
                .filter(|(task, _)| task.state.is_terminal())
                .map(|(task, revision)| TerminalSnapshot::of(&task, Some(revision)));
            confirmed.deliveries.push(Delivery {
                target: target.clone(),
                notice,
                token,
                current,
            });
        }
        if let Some(ctx) = target.settle {
            let tasks = ended_handles(mem, target.workspace)?;
            if !tasks.is_empty() {
                confirmed.settlements.push((ctx, target.workspace, tasks));
            }
        }
    }
    Ok(confirmed)
}

/// Task state and the remaining handle retain the cleanup obligation independently of notice ack.
/// Only cancellation requests without a live runner supply a settlement context. Read current rows,
/// never an obsolete notice snapshot: another continuation may already have acknowledged the notice.
fn ended_handles(
    mem: &dyn MemoryStorage,
    workspace: u32,
) -> tasty_agent::Result<Vec<CapturedSettlement>> {
    let entries = mem.list(
        &tasty_memory::Scope::Workspace(workspace),
        &tasty_memory::ListOpts {
            prefix: Some(crate::runner_host::HANDLE_KEY_PREFIX.into()),
            ..Default::default()
        },
    )?;
    let mut tasks = Vec::new();
    for entry in entries {
        if let Some(id) = entry
            .key
            .strip_prefix(crate::runner_host::HANDLE_KEY_PREFIX)
            && let Some((task, revision)) =
                tasty_agent::TaskStore::read_record(mem, workspace, &id.into())?
            && task.state.is_terminal()
        {
            tasks.push(CapturedSettlement::new(task, revision, entry));
        }
    }
    Ok(tasks)
}

fn deliver(confirmed: Confirmed, access: &MemoryAccess) {
    for Delivery {
        target,
        notice,
        token,
        current,
    } in confirmed.deliveries
    {
        target.hub.fire_once(
            &token,
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
        // This id names an immutable obligation. A lost ack may replay the read, but fire_once
        // suppresses another event within this hub; it does not discharge handle cleanup.
        let id = notice.id;
        let workspace = target.workspace;
        let hub = target.hub;
        access.submit_detached(
            MemoryRequest::unit(0, move |mem| {
                terminal::acknowledge(mem, HOST_OWNER, workspace, &id)
            }),
            "terminal obligation acknowledgement",
            Some(Box::new(move |out| {
                if out.into_value::<()>().is_some() {
                    hub.confirm_ack(workspace, &token);
                } else {
                    tracing::error!("unexpected terminal acknowledgement output");
                }
            })),
        );
    }
    for (ctx, workspace, tasks) in confirmed.settlements {
        crate::runner_thread::settle_ended_tasks_in_background(ctx, workspace, tasks);
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
        Ok(output) => match output.into_value::<tasty_agent::Result<Confirmed>>() {
            Some(Ok(deliveries)) => deliver(deliveries, &ctx.memory),
            Some(Err(error)) => tracing::warn!(%error, "terminal obligations remain pending"),
            None => tracing::error!("unexpected terminal confirmation output"),
        },
        Err(error) => tracing::warn!(?error, "terminal reconciliation could not be confirmed"),
    }
}

#[cfg(test)]
mod lifetime_tests;
