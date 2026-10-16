//! agent task 새 세션의 `<provider>.spawn`. 응답은 따로 띄운 스레드가 기다리고, 러너는 poll 마다
//! 확인만 한다. spawn 을 기다리는 동안에도 같은 러너의 다른 task 가 진행된다.
//!
//! 러너가 드는 handle 은 [`DispatchHandle::AwaitDispatchAnswer`] 다. 응답이 오면 Custom 의 늦은
//! 응답처럼 `AgentTurn` 으로 이어 가고 저장 handle 도 바꾼다. 응답 전에 재시작하면 같은 규칙으로
//! 결과 불명이다.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use serde_json::json;
use tasty_agent::runner::{DispatchHandle, PollOutcome};
use tasty_agent::task::FailureCode;
use tasty_agent::{Task, TaskId};
use tasty_ipc::host_call::HostIpcInjector;

use super::HostExecutor;
use super::late_answer::DISPATCH_OUTCOME_UNKNOWN;
use crate::agent_turns::{AgentTurns, TurnBinding};

/// provider spawn 의 응답 대기. spawn 은 셸을 띄우고 CLI 를 실행한 뒤 답하므로 일반 호출(5초)보다
/// 오래 걸린다(실측 약 4초, 동시 spawn 이면 5초 초과). 다시 보내면 세션이 하나 더 생기므로
/// 넉넉히 기다린다.
const SPAWN_TIMEOUT: Duration = Duration::from_secs(30);

/// task 가 포기한 뒤에도 spawn 응답을 기다리는 상한. 그 안에 세션이 뜨면 task 가 닫는다.
const SPAWN_CLEANUP_WAIT: Duration = Duration::from_secs(300);

/// spawn 응답을 넘겨받을 자리. task 가 기다리기를 그만두면 늦은 응답의 세션을 닫는다.
enum SpawnSlot {
    Waiting,
    Abandoned,
}

static SLOT_POISONED: AtomicBool = AtomicBool::new(false);

fn lock_slot(slot: &Mutex<SpawnSlot>) -> std::sync::MutexGuard<'_, SpawnSlot> {
    tasty_utils::poison::recover_mutex(slot.lock(), "agent spawn reply slot", &SLOT_POISONED)
}

/// 응답을 기다리는 spawn 하나. 버려지면(취소·종결·executor 정리) 기다리기를 그만두고, 이미 뜬
/// 세션은 묶음을 풀고 닫는다.
pub(super) struct PendingSpawn {
    method: String,
    slot: Arc<Mutex<SpawnSlot>>,
    /// 응답을 받아 회차에 묶은 surface, 또는 실패 사유.
    rx: mpsc::Receiver<Result<u32, String>>,
    /// 이때까지 답이 없으면 task 를 `agent_unavailable` 로 끝낸다.
    pub(super) give_up_at: Instant,
    /// surface 만 비운 `AgentTurn`.
    turn: DispatchHandle,
    workspace: u32,
    task: TaskId,
    attempt: String,
    inj: HostIpcInjector,
    turns: Arc<AgentTurns>,
}

impl PendingSpawn {
    /// `method` 를 보내고 응답을 기다리는 스레드를 띄운다. 응답이 오면 그 스레드가 바로 회차에
    /// 묶는다(턴 보고가 묶음보다 먼저 와 버려지지 않게).
    fn start(
        inj: &HostIpcInjector,
        turns: &Arc<AgentTurns>,
        method: String,
        params: serde_json::Value,
        binding: TurnBinding,
        turn: DispatchHandle,
    ) -> Self {
        let slot = Arc::new(Mutex::new(SpawnSlot::Waiting));
        let (tx, rx) = mpsc::channel();
        let pending = Self {
            method: method.clone(),
            slot: slot.clone(),
            rx,
            give_up_at: Instant::now() + SPAWN_TIMEOUT,
            turn,
            workspace: binding.workspace,
            task: binding.task.clone(),
            attempt: binding.attempt.clone(),
            inj: inj.clone(),
            turns: turns.clone(),
        };
        let (inj, turns) = (inj.clone(), turns.clone());
        std::thread::spawn(move || {
            let res = inj
                .dispatch(&method, params, SPAWN_CLEANUP_WAIT)
                .map_err(|e| e.to_string());
            let g = lock_slot(&slot);
            match *g {
                SpawnSlot::Waiting => {
                    let (workspace, task) = (binding.workspace, binding.task.clone());
                    let bound = res.and_then(|v| bind_new_session(&turns, &v, binding));
                    if let Err(mpsc::SendError(Ok(surface))) = tx.send(bound) {
                        drop(g);
                        turns.release(workspace, &task);
                        close_session(&inj, &method, surface);
                    }
                }
                SpawnSlot::Abandoned => {
                    drop(g);
                    if let Some(surface) = late_surface(&method, res) {
                        close_session(&inj, &method, surface);
                    }
                }
            }
        });
        pending
    }

    /// 상한이 지나 기다리기를 그만둔다. 잠금 사이에 응답이 왔으면 그것을 돌려준다.
    fn give_up(&self) -> Option<Result<u32, String>> {
        let mut g = lock_slot(&self.slot);
        if let Ok(res) = self.rx.try_recv() {
            return Some(res);
        }
        *g = SpawnSlot::Abandoned;
        None
    }

    fn turn_handle(&self, surface: u32) -> DispatchHandle {
        let mut handle = self.turn.clone();
        if let DispatchHandle::AgentTurn { surface_id, .. } = &mut handle {
            *surface_id = surface;
        }
        handle
    }
}

impl Drop for PendingSpawn {
    fn drop(&mut self) {
        let mut g = lock_slot(&self.slot);
        *g = SpawnSlot::Abandoned;
        let Ok(Ok(surface)) = self.rx.try_recv() else {
            return;
        };
        drop(g);
        // 넘겨받지 않은 세션은 이 task 의 것이지만 이제 아무 회차도 기다리지 않는다.
        self.turns.release(self.workspace, &self.task);
        let (inj, method) = (self.inj.clone(), self.method.clone());
        std::thread::spawn(move || close_session(&inj, &method, surface));
    }
}

/// spawn 응답의 새 세션을 회차에 묶는다. 새 세션에는 이전 턴이 없어 spawn 의 지시가 첫 턴이다.
fn bind_new_session(
    turns: &AgentTurns,
    reply: &serde_json::Value,
    binding: TurnBinding,
) -> Result<u32, String> {
    let surface = reply
        .get("child_surface_id")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .ok_or("no child_surface_id")?;
    turns
        .bind(surface, binding)
        .map_err(|other| format!("surface {surface} is bound to {other}"))?;
    Ok(surface)
}

/// 늦은 spawn 응답이 띄운 세션의 surface.
fn late_surface(method: &str, res: Result<serde_json::Value, String>) -> Option<u32> {
    match res {
        Ok(v) => v
            .get("child_surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32),
        Err(e) => {
            tracing::info!("{method}: late spawn did not start a session: {e}");
            None
        }
    }
}

/// 이 task 가 띄웠지만 회차에 묶지 못한 세션을 닫는다. 사용자가 연 세션은 여기 오지 않는다.
fn close_session(inj: &HostIpcInjector, method: &str, surface: u32) {
    let closed = inj.dispatch(
        "surface.close",
        json!({ "surface_id": surface }),
        Duration::from_secs(5),
    );
    match closed {
        Ok(_) => {
            tracing::info!("{method}: closed late session surface {surface} the task gave up on")
        }
        Err(e) => tracing::warn!("{method}: could not close late session surface {surface}: {e}"),
    }
}

impl HostExecutor {
    /// spawn 을 띄우고 응답을 기다리는 handle 을 돌려준다. `turn` 은 surface 만 비운 `AgentTurn` 이다.
    pub(super) fn start_agent_spawn(
        &mut self,
        task: &Task,
        inj: &HostIpcInjector,
        method: String,
        params: serde_json::Value,
        binding: TurnBinding,
        turn: DispatchHandle,
    ) -> DispatchHandle {
        let pending = PendingSpawn::start(
            inj,
            &self.ctx.agent_turns,
            method.clone(),
            params,
            binding,
            turn,
        );
        self.late.forget(&task.id);
        self.late.spawns.insert(task.id.clone(), pending);
        DispatchHandle::AwaitDispatchAnswer {
            workspace_id: task.workspace_id,
            task_id: task.id.clone(),
            ipc_method: method,
        }
    }

    /// 기다리는 spawn 이 있으면 그 poll 결과. 응답이 오면 `AgentTurn` 으로 이어 간다.
    pub(super) fn poll_agent_spawn(&mut self, task_id: &TaskId) -> Option<PollOutcome> {
        let pending = self.late.spawns.get(task_id)?;
        let unavailable = |detail: String| FailureCode::AgentUnavailable.message(detail);
        let got = match pending.rx.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) if Instant::now() < pending.give_up_at => {
                return Some(PollOutcome::Active);
            }
            Err(mpsc::TryRecvError::Empty) => match pending.give_up() {
                Some(got) => got,
                None => {
                    let method = pending.method.clone();
                    self.late.spawns.remove(task_id);
                    return Some(PollOutcome::Failed(unavailable(format!(
                        "{method}: no reply within {}s (a session that starts later is closed by the task)",
                        SPAWN_TIMEOUT.as_secs()
                    ))));
                }
            },
            // 기다리던 스레드가 답 없이 사라졌다. 세션이 떴는지 알 수 없다.
            Err(mpsc::TryRecvError::Disconnected) => {
                let method = pending.method.clone();
                self.late.spawns.remove(task_id);
                return Some(PollOutcome::Lost(format!(
                    "{DISPATCH_OUTCOME_UNKNOWN}: '{method}' started but its answer was lost; \
                     check whether a session started before retrying"
                )));
            }
        };
        let pending = self.late.spawns.remove(task_id)?;
        let surface = match got {
            Ok(surface) => surface,
            Err(e) => {
                return Some(PollOutcome::Failed(unavailable(format!(
                    "{}: {e}",
                    pending.method
                ))));
            }
        };
        self.held_turns.insert(task_id.clone(), pending.workspace);
        let handle = pending.turn_handle(surface);
        // 재시작 뒤에는 이어 간 handle 로 복원한다.
        self.persist_handle(pending.workspace, task_id, &handle, Some(&pending.attempt));
        self.late.continued.insert(task_id.clone(), handle.clone());
        Some(self.poll_handle(&handle))
    }
}
