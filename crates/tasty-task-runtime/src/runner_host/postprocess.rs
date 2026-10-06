//! 후처리 실행의 host 쪽 — 시작 기록, 프로세스 보관, 결과 영속, 취소, 재시작 복원.
//!
//! 프로세스 실행과 출력 수집은 작업 스레드에서 하며 저장소 잠금을 잡지 않는다. 잠금은 시작
//! 기록과 결과 영속에만 짧게 쓴다.

mod process;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use tasty_agent::task::postprocess::{
    PostprocessCause, PostprocessPhase, PostprocessReport, stdin_document,
};

use super::*;

/// 후처리 실행 결과 키. 재시작 때 `Started` 실행의 결과를 찾는 데 쓴다.
pub(crate) const POSTPROCESS_RESULT_KEY_PREFIX: &str = "tasty.agent.postprocess_result.";

pub(crate) fn postprocess_result_key(task_id: &str) -> String {
    format!("{POSTPROCESS_RESULT_KEY_PREFIX}{task_id}")
}

/// 실행 중인 후처리 하나.
struct Entry {
    task_id: TaskId,
    stop: Arc<AtomicU8>,
    report: Arc<Mutex<Option<PostprocessReport>>>,
    /// 작업 스레드가 프로세스를 정리하고 보고를 만들었다.
    done: Arc<AtomicBool>,
    _watcher: thread::JoinHandle<()>,
}

/// executor 가 소유한 후처리 실행들.
#[derive(Default)]
pub(crate) struct PostprocessRuns {
    /// pid 별 실행 중인 후처리.
    active: HashMap<u32, Entry>,
    /// 취소를 요청했고 종료를 기다리는 실행. 종료를 확인한 뒤에야 점유 자원을 놓는다.
    cancelling: Vec<Entry>,
}

static POSTPROCESS_CELL_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl PostprocessRuns {
    pub(crate) fn poll(&mut self, pid: u32, run: u32) -> PollOutcome {
        let Some(entry) = self.active.get(&pid) else {
            return PollOutcome::Postprocessed(PostprocessReport::failed(
                run,
                PostprocessCause::OutcomeUnknown,
                format!("postprocess pid {pid} is not tracked by this host"),
            ));
        };
        let taken = tasty_utils::poison::recover_mutex(
            entry.report.lock(),
            "agent postprocess result cell",
            &POSTPROCESS_CELL_POISON_REPORTED,
        )
        .take();
        match taken {
            Some(report) => {
                self.active.remove(&pid);
                PollOutcome::Postprocessed(report)
            }
            None => PollOutcome::Active,
        }
    }

    /// task 가 종결됐다. 진행 중인 실행이 있으면 취소를 요청하고 `true` 를 돌려준다. 그때는
    /// 종료를 확인할 때까지 점유 자원을 놓지 않는다([`Self::terminated`]).
    pub(crate) fn cancel(&mut self, task_id: &TaskId) -> bool {
        let Some(pid) = self
            .active
            .iter()
            .find(|(_, e)| &e.task_id == task_id)
            .map(|(pid, _)| *pid)
        else {
            return false;
        };
        let Some(entry) = self.active.remove(&pid) else {
            return false;
        };
        if entry.done.load(Ordering::Acquire) {
            return false;
        }
        entry.stop.store(process::STOP_CANCELLED, Ordering::Release);
        self.cancelling.push(entry);
        true
    }

    /// 취소한 실행 중 종료를 확인한 task.
    pub(crate) fn terminated(&mut self) -> Vec<TaskId> {
        let (done, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut self.cancelling)
            .into_iter()
            .partition(|e| e.done.load(Ordering::Acquire));
        self.cancelling = waiting;
        done.into_iter().map(|e| e.task_id).collect()
    }
}

/// runner 가 멈출 때 후처리 작업 스레드가 프로세스 그룹을 끝내고 보고를 저장하기를 기다리는
/// 상한. 그룹 종료 뒤 파이프 EOF 를 기다리는 2초에 저장 시간을 더한 값이다. 보통은 수십 ms 안에
/// 끝나며, 넘으면 기다리지 않고 경고한다.
const RUNNER_STOP_WAIT: Duration = Duration::from_secs(3);

impl Drop for PostprocessRuns {
    /// runner 가 멈추면 이 executor 가 띄운 후처리를 모두 중단하고, 그룹 종료와 보고 저장을
    /// [`RUNNER_STOP_WAIT`] 까지 기다린다. 앱 종료 때 프로세스가 끝나기 전에 그룹을 끝내기
    /// 위해서다. 중단한 실행은 자동으로 다시 실행하지 않으며, 보고가 저장되기 전에 호스트가
    /// 끝나면 재시작 뒤 결과 불명으로 끝난다.
    fn drop(&mut self) {
        for e in self.active.values() {
            e.stop.store(process::STOP_RUNNER, Ordering::Release);
        }
        let pending = || {
            self.active
                .values()
                .chain(&self.cancelling)
                .filter(|e| !e.done.load(Ordering::Acquire))
                .count()
        };
        let deadline = std::time::Instant::now() + RUNNER_STOP_WAIT;
        let mut left = pending();
        while left != 0 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
            left = pending();
        }
        if left != 0 {
            tracing::warn!(
                left,
                "postprocess runs did not stop before the runner shutdown wait ran out"
            );
        }
    }
}

impl HostExecutor {
    /// 예약된 후처리 실행을 시작한다. 시작을 기록한 뒤에만 프로세스를 띄운다.
    pub(super) fn start_postprocess_run(
        &mut self,
        task: &Task,
        attempt_id: &str,
        run: u32,
    ) -> DispatchHandle {
        let ws = task.workspace_id;
        let resolved = |report: PostprocessReport| DispatchHandle::PostprocessResolved(report);
        let begun = self.ctx.with_memory(|mem| {
            let seq = self.ctx.agent_seq.clone();
            tasty_agent::TaskStore::new(mem, HOST_OWNER, seq.as_ref()).begin_postprocess_run(
                ws,
                &task.id,
                attempt_id,
                run,
                now_ms(),
            )
        });
        let (current, spec, execution) = match begun {
            Ok(v) => v,
            Err(e) => {
                return resolved(PostprocessReport::failed(
                    run,
                    PostprocessCause::Spawn,
                    format!("could not record the start: {e}"),
                ));
            }
        };
        let stdin = match stdin_document(&current, &spec, &execution) {
            Ok(doc) => doc.to_string().into_bytes(),
            Err((cause, message)) => {
                return self.resolved_after_start(ws, &task.id, attempt_id, run, cause, message);
            }
        };
        let cwd = spec
            .cwd
            .as_ref()
            .map(PathBuf::from)
            .or_else(|| match &task.command {
                TaskCommand::Run { cwd, .. } => cwd.clone(),
                _ => None,
            });
        let request = process::ProcessRequest {
            command: spec.command.clone(),
            cwd,
            stdin,
            stdout: spec.stdout.clone(),
            timeout: Duration::from_millis(spec.timeout_ms),
            run,
            env: process::child_env(std::env::vars_os()),
        };
        let started = match process::spawn(request) {
            Ok(s) => s,
            Err(report) => {
                persist_report(&self.ctx.memory, ws, &task.id, attempt_id, &report);
                return resolved(report);
            }
        };
        let pid = started.pid;
        let stop = Arc::new(AtomicU8::new(process::STOP_NONE));
        let cell: Arc<Mutex<Option<PostprocessReport>>> = Arc::new(Mutex::new(None));
        let done = Arc::new(AtomicBool::new(false));
        let watcher = {
            let (stop, cell, done) = (stop.clone(), cell.clone(), done.clone());
            let memory = self.ctx.memory.clone();
            let (task_id, attempt) = (task.id.clone(), attempt_id.to_string());
            thread::Builder::new()
                .name(format!("agent-postprocess-pid{pid}"))
                .spawn(move || {
                    let report = started.wait(&stop);
                    persist_report(&memory, ws, &task_id, &attempt, &report);
                    *tasty_utils::poison::recover_mutex(
                        cell.lock(),
                        "agent postprocess result cell",
                        &POSTPROCESS_CELL_POISON_REPORTED,
                    ) = Some(report);
                    done.store(true, Ordering::Release);
                })
        };
        let watcher = match watcher {
            Ok(w) => w,
            // 작업 스레드를 만들지 못했으면 프로세스는 Started 의 drop 에서 정리된다.
            Err(e) => {
                let message = format!("postprocess watcher: {e}");
                return self.resolved_after_start(
                    ws,
                    &task.id,
                    attempt_id,
                    run,
                    PostprocessCause::Spawn,
                    message,
                );
            }
        };
        self.postprocess.active.insert(
            pid,
            Entry {
                task_id: task.id.clone(),
                stop,
                report: cell,
                done,
                _watcher: watcher,
            },
        );
        let handle = DispatchHandle::PostprocessProcess { pid, run };
        self.persist_handle(ws, &task.id, &handle, Some(attempt_id));
        handle
    }

    fn resolved_after_start(
        &self,
        ws: u32,
        task_id: &TaskId,
        attempt_id: &str,
        run: u32,
        cause: PostprocessCause,
        message: String,
    ) -> DispatchHandle {
        let report = PostprocessReport::failed(run, cause, message);
        persist_report(&self.ctx.memory, ws, task_id, attempt_id, &report);
        DispatchHandle::PostprocessResolved(report)
    }
}

/// 실행 결과를 회차·실행 번호와 함께 저장한다. 실패하면 경고만 한다(재시작 뒤에는 결과 불명이 된다).
fn persist_report(
    memory: &Arc<Mutex<dyn MemoryStorage>>,
    workspace_id: u32,
    task_id: &str,
    attempt_id: &str,
    report: &PostprocessReport,
) {
    let value = MemoryValue::Json(json!({
        "attempt_id": attempt_id,
        "report": report,
    }));
    let res = {
        let mut guard = tasty_utils::poison::recover_mutex(
            memory.lock(),
            "agent runner memory",
            &MEMORY_POISON_REPORTED,
        );
        guard.put(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &postprocess_result_key(task_id),
            &value,
            &PutOpts::default(),
        )
    };
    if let Err(e) = res {
        tracing::warn!("persist postprocess result {task_id}: {e}");
    }
}

fn load_report(
    ctx: &RunnerContext,
    workspace_id: u32,
    task_id: &str,
    attempt_id: &str,
    run: u32,
) -> Option<PostprocessReport> {
    let value = ctx.with_memory(|mem| {
        mem.get(
            &Scope::Workspace(workspace_id),
            &postprocess_result_key(task_id),
        )
        .ok()
        .flatten()
        .map(|e| e.value)
    })?;
    let MemoryValue::Json(v) = value else {
        return None;
    };
    if v.get("attempt_id").and_then(|a| a.as_str()) != Some(attempt_id) {
        return None;
    }
    let report: PostprocessReport = serde_json::from_value(v.get("report")?.clone()).ok()?;
    (report.run == run).then_some(report)
}

pub(crate) fn evict_postprocess_result(ctx: &RunnerContext, workspace_id: u32, task_id: &str) {
    let res = ctx.with_memory(|mem| {
        mem.delete(
            HOST_OWNER,
            &Scope::Workspace(workspace_id),
            &postprocess_result_key(task_id),
            None,
        )
    });
    if let Err(e) = res {
        tracing::warn!("evict postprocess result {task_id}: {e}");
    }
}

/// 재시작 때 후처리 단계에 있는 Running task 의 handle. 저장된 handle 대신 회차의 진행으로 정한다.
///
/// 시작을 기록했는데 결과가 저장되지 않았으면 같은 실행을 다시 하지 않고 결과 불명으로 끝낸다.
/// 살아 있는 PID 가 있어도 같은 프로세스라는 증거가 아니므로 쓰지 않는다.
pub(crate) fn restored_handle(
    ctx: &RunnerContext,
    workspace_id: u32,
    task: &Task,
) -> Option<DispatchHandle> {
    let attempt = task.attempt.as_ref()?;
    let progress = attempt.postprocess.as_ref()?;
    match progress.phase {
        PostprocessPhase::Pending { run, not_before_ms } => {
            Some(DispatchHandle::PostprocessPending { run, not_before_ms })
        }
        PostprocessPhase::Started { run, .. } => Some(DispatchHandle::PostprocessResolved(
            load_report(ctx, workspace_id, &task.id, &attempt.id, run).unwrap_or_else(|| {
                PostprocessReport::failed(
                    run,
                    PostprocessCause::OutcomeUnknown,
                    "the host restarted before the postprocess result was recorded; \
                     it is not run again automatically",
                )
            }),
        )),
        PostprocessPhase::Finished { .. } => None,
    }
}

#[cfg(test)]
#[path = "postprocess_tests.rs"]
mod tests;
