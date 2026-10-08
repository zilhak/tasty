//! dispatch 한 handle 의 완료 확인.

use serde_json::json;
use tasty_agent::runner::{DispatchHandle, PollOutcome};
use tasty_agent::{BarrierState, BarrierStore, TaskResult};
use tasty_memory::HOST_OWNER;

use super::{
    HostExecutor, INJECTOR_GRACE_MS, RUN_RESULT_LOST, RUN_RESULT_POISON_REPORTED,
    is_injector_not_initialized, now_ms, summarize_poll_response,
};

impl HostExecutor {
    pub(super) fn poll_handle(&mut self, handle: &DispatchHandle) -> PollOutcome {
        match handle {
            DispatchHandle::PolledDispatch {
                poll_method,
                poll_params,
                state_field,
                terminal_states,
                failure_states,
                deadline_ms,
                ..
            } => {
                let resp = match self.ctx.dispatch_plugin(poll_method, poll_params.clone()) {
                    Ok(v) => {
                        self.injector_grace_deadline_ms = None;
                        v
                    }
                    Err(e) if is_injector_not_initialized(&e) => {
                        let now = now_ms();
                        let deadline = *self
                            .injector_grace_deadline_ms
                            .get_or_insert(now + INJECTOR_GRACE_MS);
                        if now < deadline {
                            return PollOutcome::Active;
                        }
                        return PollOutcome::Failed(format!(
                            "{poll_method}: injector grace expired ({INJECTOR_GRACE_MS}ms)"
                        ));
                    }
                    Err(e) => return PollOutcome::Failed(format!("{poll_method}: {e}")),
                };
                let state = resp.get(state_field).and_then(|v| v.as_str()).unwrap_or("");
                // 성공·실패 목록에 모두 있으면 실패를 우선한다. 없는 산출물로 후속 작업을 진행하지 않게 한다.
                if failure_states.iter().any(|s| s == state) {
                    return PollOutcome::Failed(format!(
                        "{poll_method}: failure state '{state}' — {}",
                        summarize_poll_response(&resp)
                    ));
                }
                if terminal_states.iter().any(|s| s == state) {
                    PollOutcome::Done(TaskResult {
                        exit_code: None,
                        output: Some(resp),
                        error: None,
                    })
                } else {
                    // 미완료 응답에서 기한을 확인한다. 한 번의 IPC 대기 자체를 이 기한으로 중단하지는 않는다.
                    if let Some(deadline) = deadline_ms
                        && now_ms() >= *deadline
                    {
                        return PollOutcome::Failed(format!("{poll_method}: poll timeout"));
                    }
                    PollOutcome::Active
                }
            }
            DispatchHandle::ReduceImmediate(r) | DispatchHandle::CustomImmediate(r) => {
                PollOutcome::Done(r.clone())
            }
            DispatchHandle::ShellProcess { pid } => {
                if let Some(entry) = self.shell_children.get(pid) {
                    let taken = tasty_utils::poison::recover_mutex(
                        entry.result.lock(),
                        "agent run result cell",
                        &RUN_RESULT_POISON_REPORTED,
                    )
                    .take();
                    if let Some(outcome) = taken {
                        self.shell_children.remove(pid);
                        return outcome;
                    }
                    return PollOutcome::Active;
                }
                // 이 executor의 watcher가 없으면(재시작 뒤 복원한 handle) 저장한 PID·시작 시각으로
                // 같은 프로세스가 살아 있는지 본다. 살아 있는 동안은 permit 을 쥐고 기다린다.
                // 끝난 뒤에는 그 종료 코드를 받을 수 없으므로(부모가 아니다) 결과 불명으로 둔다.
                if self.restored_run_alive(*pid) {
                    return PollOutcome::Active;
                }
                PollOutcome::Lost(format!(
                    "{RUN_RESULT_LOST}: pid {pid} ended after a host restart and its exit status could not be collected"
                ))
            }
            DispatchHandle::ImmediateFail(err) => PollOutcome::Failed(err.clone()),
            DispatchHandle::BarrierPoll { workspace_id, name } => {
                let now = now_ms();
                let res = self.ctx.with_memory(|mem| {
                    let mut store = BarrierStore::new(mem, HOST_OWNER);
                    store.state(*workspace_id, name, now)
                });
                match res {
                    Ok(b) => match b.state {
                        BarrierState::Open => PollOutcome::Active,
                        BarrierState::Closed => PollOutcome::Done(TaskResult {
                            exit_code: Some(0),
                            output: Some(json!({
                                "barrier": name,
                                "count_signaled": b.count_signaled,
                                "count_required": b.count_required,
                            })),
                            error: None,
                        }),
                        BarrierState::TimedOut => {
                            PollOutcome::Failed(format!("barrier '{name}' timed out"))
                        }
                    },
                    Err(e) => PollOutcome::Failed(format!("barrier poll '{name}': {e}")),
                }
            }
            // 외부 훅·만료 처리가 store를 종결시키면 다음 러너 tick이 handle과 점유 자원을 정리한다.
            DispatchHandle::AwaitExternal { .. } => PollOutcome::Active,
            // 예약·확정된 후처리 handle 은 runner 가 직접 다룬다.
            DispatchHandle::PostprocessPending { .. } => PollOutcome::Active,
            DispatchHandle::PostprocessResolved(report) => {
                PollOutcome::Postprocessed(report.clone())
            }
            DispatchHandle::PostprocessProcess { pid, run } => self.poll_postprocess(*pid, *run),
            DispatchHandle::AgentTurn { .. } => self.poll_agent(handle),
        }
    }
}
