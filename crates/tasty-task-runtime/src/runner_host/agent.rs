//! agent task 실행. provider 플러그인의 spawn·tell·state 를 재사용하고, 턴의 귀속과 결과는
//! [`crate::agent_turns`] 의 표로 판단한다. 세션을 닫거나 사용자 프로세스를 끝내지 않는다.

use serde_json::json;
use tasty_agent::runner::{DispatchHandle, PollOutcome};
use tasty_agent::task::FailureCode;
use tasty_agent::task::agent::{self, AgentLink, AgentSession};
use tasty_agent::{Task, TaskCommand, TaskStore};
use tasty_memory::HOST_OWNER;

use super::{HostExecutor, INJECTOR_UNINIT_MSG, now_ms};
use crate::agent_turns::{TurnBinding, TurnPoll, decide};

/// provider spawn 의 응답 대기. spawn 은 셸을 띄우고 CLI 를 실행한 뒤 답하므로 일반 호출(5초)보다
/// 오래 걸린다(실측 약 4초, 동시 spawn 이면 5초 초과). 다시 보내면 세션이 하나 더 생기므로
/// 넉넉히 기다린다.
const SPAWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

impl HostExecutor {
    /// 새 세션은 spawn 으로 지시를 보내고 바로 회차에 묶는다. 기존 세션은 handle 에 지시를 담아
    /// 두고 poll 에서 세션이 비었을 때 보낸다.
    pub(super) fn dispatch_agent(&mut self, task: &Task) -> Result<DispatchHandle, String> {
        let TaskCommand::Agent {
            provider,
            workspace_id,
            session,
            instruction,
            timeout_ms,
        } = &task.command
        else {
            return Err("dispatch_agent: not an agent task".into());
        };
        let unavailable = |detail: String| FailureCode::AgentUnavailable.message(detail);
        if !agent::AGENT_PROVIDERS.contains(&provider.as_str()) {
            return Err(unavailable(format!(
                "provider '{provider}' does not support result collection"
            )));
        }
        let contract = task
            .contract
            .as_ref()
            .ok_or_else(|| unavailable(agent::AGENT_NEEDS_CONTRACT.into()))?;
        // dispatch 는 Running 전이 직전이다. 이 실행이 만들 회차가 턴의 회차다.
        let attempt_id = super::dispatch_attempt(task)
            .ok_or_else(|| unavailable(agent::AGENT_NEEDS_CONTRACT.into()))?;
        let needs_submission = agent::needs_submission(contract, &task.command);
        let mut text = task
            .input_snapshot
            .as_ref()
            .and_then(|s| s.execution.instruction.clone())
            .unwrap_or_else(|| instruction.clone());
        if needs_submission {
            let schema = serde_json::to_value(contract.output_schema(&task.command))
                .map_err(|e| unavailable(e.to_string()))?;
            text.push_str("\n\n");
            text.push_str(&agent::submission_instructions(
                task.workspace_id,
                &task.id,
                &attempt_id,
                &schema,
            ));
        }
        let deadline_ms = timeout_ms.map(|t| now_ms().saturating_add(t));
        // 회차 기록·제출·턴 표는 task 자신의 workspace 를 쓴다. 명령의 workspace_id 는 새 세션이
        // 뜨는 곳일 뿐이다.
        let handle = |surface_id: u32, pending: Option<String>| DispatchHandle::AgentTurn {
            workspace_id: task.workspace_id,
            task_id: task.id.clone(),
            attempt_id: attempt_id.clone(),
            provider: provider.clone(),
            surface_id,
            needs_submission,
            deadline_ms,
            pending_instruction: pending,
        };
        match session {
            AgentSession::Existing { surface_id } => Ok(handle(*surface_id, Some(text))),
            AgentSession::New {
                parent_surface,
                cwd,
            } => {
                let mut params = json!({
                    "surface": parent_surface,
                    "workspace": workspace_id.to_string(),
                    "prompt": text,
                });
                if let Some(cwd) = cwd {
                    params["cwd"] = json!(cwd);
                }
                let method = format!("{provider}.spawn");
                let inj = self
                    .ctx
                    .host_ipc
                    .get()
                    .ok_or_else(|| unavailable(format!("{method}: {INJECTOR_UNINIT_MSG}")))?;
                let resp = inj.dispatch(&method, params, SPAWN_TIMEOUT).map_err(|e| {
                    // 응답이 늦었을 뿐 spawn 이 실행됐을 수 있다. 그 세션은 이 회차에 묶이지 않는다.
                    unavailable(format!(
                        "{method}: {e} (the session may still start; it is not bound to this task)"
                    ))
                })?;
                let surface = resp
                    .get("child_surface_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .ok_or_else(|| unavailable(format!("{method}: no child_surface_id")))?;
                // 새 세션에는 이전 턴이 없다. spawn 의 지시가 이 회차의 첫 턴이다.
                let binding = TurnBinding::new(
                    task.workspace_id,
                    task.id.clone(),
                    attempt_id.clone(),
                    provider.clone(),
                    true,
                );
                self.ctx
                    .agent_turns
                    .bind(surface, binding)
                    .map_err(|other| {
                        unavailable(format!("surface {surface} is bound to {other}"))
                    })?;
                self.held_turns.insert(task.id.clone(), task.workspace_id);
                Ok(handle(surface, None))
            }
        }
    }

    pub(super) fn poll_agent(&mut self, handle: &DispatchHandle) -> PollOutcome {
        let DispatchHandle::AgentTurn {
            workspace_id,
            task_id,
            attempt_id,
            provider,
            surface_id,
            needs_submission,
            deadline_ms,
            pending_instruction,
        } = handle
        else {
            return PollOutcome::Failed("poll_agent: not an agent handle".into());
        };
        let now = now_ms();
        let state = self.provider_state(provider, *surface_id);
        let binding = match self.ctx.agent_turns.find(*workspace_id, task_id) {
            Some((_, b)) if b.attempt == *attempt_id => b,
            Some(_) => return PollOutcome::Failed("agent turn bound to another attempt".into()),
            None => {
                let Some(text) = pending_instruction else {
                    // 묶음이 사라졌다(재시작 등). 이 회차의 턴을 귀속할 수 없다.
                    return PollOutcome::Failed(
                        FailureCode::AgentUnavailable.message("the turn binding was lost"),
                    );
                };
                return self.send_pending(handle, text, state.as_deref(), now);
            }
        };
        match decide(
            &binding,
            *surface_id,
            *needs_submission,
            state.as_deref(),
            now,
            *deadline_ms,
        ) {
            TurnPoll::Active { awaiting_input } => {
                if let Some(since) =
                    self.ctx
                        .agent_turns
                        .note_awaiting(*workspace_id, task_id, awaiting_input, now)
                {
                    self.record_link(
                        *workspace_id,
                        task_id,
                        attempt_id,
                        provider,
                        *surface_id,
                        since,
                    );
                }
                PollOutcome::Active
            }
            TurnPoll::Done(r) => PollOutcome::Done(r),
            TurnPoll::Failed(e) => PollOutcome::Failed(e),
        }
    }

    /// 기존 세션이 비었으면 회차에 묶고 지시를 보낸다. 다른 회차가 묶었거나 세션이 바쁘면
    /// 다음 poll 에서 다시 본다.
    fn send_pending(
        &mut self,
        handle: &DispatchHandle,
        text: &str,
        state: Option<&str>,
        now: u64,
    ) -> PollOutcome {
        let DispatchHandle::AgentTurn {
            workspace_id,
            task_id,
            attempt_id,
            provider,
            surface_id,
            deadline_ms,
            ..
        } = handle
        else {
            return PollOutcome::Active;
        };
        match state {
            Some("exited") => {
                return PollOutcome::Failed(FailureCode::AgentExited.message(format!(
                    "surface {surface_id} exited before the instruction was sent"
                )));
            }
            Some("idle") => {}
            // 사용자의 턴이 진행 중이거나 입력을 기다린다. 끼어들지 않는다.
            _ => {
                if deadline_ms.is_some_and(|d| now >= d) {
                    return PollOutcome::Failed(
                        FailureCode::TimedOut.message("the session never became idle"),
                    );
                }
                return PollOutcome::Active;
            }
        }
        let binding = TurnBinding::new(
            *workspace_id,
            task_id.clone(),
            attempt_id.clone(),
            provider.clone(),
            false,
        );
        if self.ctx.agent_turns.bind(*surface_id, binding).is_err() {
            return PollOutcome::Active;
        }
        let method = format!("{provider}.tell");
        let sent = self.ctx.dispatch_plugin(
            &method,
            json!({ "surface_id": surface_id, "message": text }),
        );
        if let Err(e) = sent {
            self.ctx.agent_turns.release(*workspace_id, task_id);
            return PollOutcome::Failed(
                FailureCode::AgentUnavailable.message(format!("{method}: {e}")),
            );
        }
        self.held_turns.insert(task_id.clone(), *workspace_id);
        if let Some(since) = self
            .ctx
            .agent_turns
            .note_awaiting(*workspace_id, task_id, false, now)
        {
            self.record_link(
                *workspace_id,
                task_id,
                attempt_id,
                provider,
                *surface_id,
                since,
            );
        }
        PollOutcome::Active
    }

    /// `<provider>.state` 의 상태. 조회하지 못하면 `None` 이다(턴 판단은 보고로 한다).
    fn provider_state(&self, provider: &str, surface_id: u32) -> Option<String> {
        let method = format!("{provider}.state");
        match self
            .ctx
            .dispatch_plugin(&method, json!({ "surface_id": surface_id }))
        {
            Ok(v) => v.get("state").and_then(|s| s.as_str()).map(str::to_string),
            Err(e) => {
                tracing::debug!("agent task {method} s{surface_id}: {e}");
                None
            }
        }
    }

    fn record_link(
        &self,
        workspace_id: u32,
        task_id: &tasty_agent::TaskId,
        attempt_id: &str,
        provider: &str,
        surface_id: u32,
        awaiting_input_since: Option<u64>,
    ) {
        let link = AgentLink {
            provider: provider.to_string(),
            surface_id,
            awaiting_input_since,
        };
        let seq = self.ctx.agent_seq.clone();
        let res = self.ctx.with_memory(|mem| {
            TaskStore::new(mem, HOST_OWNER, seq.as_ref()).set_agent_link(
                workspace_id,
                task_id,
                attempt_id,
                link,
            )
        });
        if let Err(e) = res {
            tracing::warn!("agent task {task_id}: record session link: {e}");
        }
    }
}

#[cfg(test)]
#[path = "agent_tests.rs"]
mod tests;
