//! 회차 시작: 실행 인자 준비와 command 종류별 dispatch.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::json;
use tasty_agent::runner::{DispatchHandle, PollOutcome};
use tasty_agent::task::report::ReportSource;
use tasty_agent::{
    ReducerInput, Task, TaskCommand, TaskId, TaskResult, TypedReducerInput, reduce_typed,
    reduce_with_custom, run_custom_shell, run_custom_shell_with_env,
};
use tasty_memory::HOST_OWNER;

use super::{
    HostExecutor, RUN_RESULT_POISON_REPORTED, RunProc, ShellChildEntry, child_env,
    dispatch_attempt, drain_capped, drain_capped_observed, now_ms, persist_run_result, report,
    run_group, shell_outcome_from_status, typed_inputs,
};
use crate::task_output_ref;

impl HostExecutor {
    /// 저장소 락 안에서는 선행 작업 결과만 읽고 문자열 치환은 락 밖에서 수행한다.
    pub(super) fn collect_task_outputs(
        &mut self,
        task: &Task,
    ) -> Result<HashMap<TaskId, serde_json::Value>, String> {
        let ids = task_output_ref::referenced_tasks(&task.command).map_err(|e| e.0)?;
        if ids.is_empty() {
            return Ok(HashMap::new());
        }
        let ws = task.workspace_id;
        let seq = self.ctx.agent_seq.clone();
        self.ctx.with_memory(|mem| {
            use tasty_agent::TaskStore;
            let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let mut out = HashMap::with_capacity(ids.len());
            for tid in ids {
                let t = store
                    .get(ws, &tid)
                    .map_err(|e| e.to_string())?
                    .ok_or_else(|| format!("task output reference '{tid}': task not found"))?;
                // 생성 단계에서 막지만, 저장된 레코드가 v2 를 가리키면 실행하지 않는다.
                if t.is_typed() {
                    return Err(format!(
                        "task output reference '{tid}': typed v2 results cannot be read by output placeholders"
                    ));
                }
                // 결과가 아직 없으면 null을 넣지 않고 참조 해석 실패로 알린다.
                let output = t.result.and_then(|r| r.output).ok_or_else(|| {
                    format!("task output reference '{tid}': upstream task has no result output yet")
                })?;
                out.insert(tid, output);
            }
            Ok(out)
        })
    }

    /// 조회에도 실제 실행할 치환값이 보이도록 command를 저장한다. 저장 실패는 경고하고 실행은 계속한다.
    pub(super) fn persist_substituted_command(&mut self, ws: u32, task: &Task) {
        let seq = self.ctx.agent_seq.clone();
        let res: Result<(), String> = self.ctx.with_memory(|mem| {
            use tasty_agent::TaskStore;
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let Some(mut stored) = store.get(ws, &task.id).map_err(|e| e.to_string())? else {
                return Ok(());
            };
            stored.command = task.command.clone();
            store.put(&stored).map_err(|e| e.to_string())
        });
        if let Err(e) = res {
            tracing::warn!("persist lease-substituted command for {}: {e}", task.id);
        }
    }

    pub(super) fn dispatch_command(&mut self, task: &Task) -> Result<DispatchHandle, String> {
        match &task.command {
            TaskCommand::Reduce { inputs, strategy } if task.is_typed() => {
                let collected: Result<Vec<TypedReducerInput>, String> =
                    self.ctx.with_memory(|mem| {
                        let seq = self.ctx.agent_seq.clone();
                        let store = tasty_agent::TaskStore::new(mem, HOST_OWNER, seq.as_ref());
                        inputs
                            .iter()
                            .map(|tid| {
                                store
                                    .get(task.workspace_id, tid)
                                    .map_err(|e| e.to_string())?
                                    .map(|t| TypedReducerInput::from_task(&t))
                                    .ok_or_else(|| format!("input task not found: {tid}"))
                            })
                            .collect()
                    });
                let collected = collected?;
                let conflict = task
                    .contract
                    .as_ref()
                    .map(|c| c.merge_conflict())
                    .unwrap_or(tasty_agent::task::contract::MergeConflict::Error);
                // 사용자 reduce 작업은 저장소 락 밖에서 실행한다.
                let env = report::report_env(task, ReportSource::ReduceCustom);
                let value = reduce_typed(strategy, &collected, conflict, |command, stdin| {
                    run_custom_shell_with_env(command, stdin, &env)
                })?;
                Ok(DispatchHandle::ReduceImmediate(TaskResult {
                    exit_code: None,
                    output: Some(value),
                    error: None,
                }))
            }
            TaskCommand::Reduce { inputs, strategy } => {
                let collected: Result<Vec<ReducerInput>, String> = self.ctx.with_memory(|mem| {
                    use tasty_agent::{TaskState, TaskStore};
                    let seq = self.ctx.agent_seq.clone();
                    let store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
                    let mut out: Vec<ReducerInput> = Vec::with_capacity(inputs.len());
                    for tid in inputs {
                        let t = store
                            .get(task.workspace_id, tid)
                            .map_err(|e| e.to_string())?
                            .ok_or_else(|| format!("input task not found: {tid}"))?;
                        // 생성 단계에서 막지만, 저장된 v1 reduce 가 v2 를 가리키면 실행하지 않는다.
                        if t.is_typed() {
                            return Err(format!(
                                "reduce input '{tid}': typed v2 results cannot be read by a v1 reduce"
                            ));
                        }
                        let succeeded = matches!(t.state, TaskState::Succeeded);
                        let output = t
                            .result
                            .and_then(|r| r.output)
                            .unwrap_or(serde_json::Value::Null);
                        out.push(ReducerInput {
                            succeeded,
                            task_id: tid.clone(),
                            output,
                        });
                    }
                    Ok(out)
                });
                let collected = collected?;
                // 사용자 reduce 작업은 저장소 락 밖에서 실행한다.
                let value = reduce_with_custom(strategy, &collected, run_custom_shell)
                    .map_err(|e| e.to_string())?;
                Ok(DispatchHandle::ReduceImmediate(TaskResult {
                    exit_code: Some(0),
                    output: Some(value),
                    error: None,
                }))
            }
            TaskCommand::Run { command, cwd, .. } => {
                if command.is_empty() {
                    return Err("Run: empty command".to_string());
                }
                let argv = typed_inputs::run_argv(task, command);
                let stdin_payload = typed_inputs::run_stdin(task);
                let (program, args) = argv.split_first().expect("non-empty");
                let mut cmd = std::process::Command::new(program);
                tasty_utils::process::hide_console(&mut cmd);
                cmd.args(args)
                    .env_clear()
                    .envs(child_env::inherited())
                    .envs(report::report_env(task, ReportSource::Run));
                if let Some(c) = cwd {
                    cmd.current_dir(c);
                }
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());
                run_group::configure(&mut cmd);
                if stdin_payload.is_some() {
                    cmd.stdin(std::process::Stdio::piped());
                }
                let mut child = cmd
                    .spawn()
                    .map_err(|e| format!("Run spawn '{program}': {e}"))?;
                let pid = child.id();
                // 자식을 회수하기 전이라 PID 가 다른 프로세스에 쓰이지 않는다.
                let started_at = run_group::adopt(pid);
                self.run_procs
                    .insert(task.id.clone(), RunProc { pid, started_at });
                if let (Some(payload), Some(mut pipe)) = (stdin_payload, child.stdin.take()) {
                    // 자식이 stdin 을 읽지 않아도 실행이 막히지 않도록 별도 스레드에서 쓰고 닫는다.
                    thread::Builder::new()
                        .name(format!("agent-shell-stdin-pid{pid}"))
                        .spawn(move || {
                            use std::io::Write;
                            if let Err(e) = pipe.write_all(&payload) {
                                tracing::warn!("Run stdin write for pid {pid}: {e}");
                            }
                        })
                        .map_err(|e| format!("Run stdin writer spawn '{program}': {e}"))?;
                }
                // 자식이 파이프를 채운 채 종료를 기다리지 않도록 stdout·stderr를 wait와 동시에 읽는다.
                let stdout_pipe = child.stdout.take().expect("stdout piped");
                let stderr_pipe = child.stderr.take().expect("stderr piped");
                let stdout_thread = thread::Builder::new()
                    .name(format!("agent-shell-stdout-pid{pid}"))
                    .spawn(move || drain_capped(stdout_pipe))
                    .map_err(|e| format!("Run stdout drain spawn '{program}': {e}"))?;
                // stderr 의 표지 줄은 그 자리에서 이 회차 report 에 쓴다. 결과 확정은 이 스레드가
                // 끝난 뒤라 표지 줄은 모두 블록이 닫히기 전에 들어간다.
                let mut markers =
                    report::report_address(task, ReportSource::StderrMarker).map(|addr| {
                        report::MarkerSink::new(
                            self.ctx.memory.clone(),
                            self.ctx.agent_seq.clone(),
                            self.ctx.report_limits.clone(),
                            addr,
                        )
                    });
                let stderr_thread = thread::Builder::new()
                    .name(format!("agent-shell-stderr-pid{pid}"))
                    .spawn(move || {
                        let drained = drain_capped_observed(stderr_pipe, |chunk| {
                            if let Some(m) = markers.as_mut() {
                                m.observe(chunk);
                            }
                        });
                        if let Some(m) = markers.as_mut() {
                            m.finish();
                        }
                        drained
                    })
                    .map_err(|e| format!("Run stderr drain spawn '{program}': {e}"))?;
                let result_cell: Arc<Mutex<Option<PollOutcome>>> = Arc::new(Mutex::new(None));
                let cell_clone = result_cell.clone();
                let mem_clone = self.ctx.memory.clone();
                let task_id_clone = task.id.clone();
                let ws = task.workspace_id;
                // v2 는 계약의 허용 종료 코드로 성공을 판정한다. 숫자 코드가 없으면 실패다.
                let allowed_exit = task.contract.as_ref().map(|c| c.allowed_exit_codes());
                // 자식 종료 뒤에도 상속된 파이프가 열려 있으면 drain join은 계속 기다릴 수 있다.
                let watcher = thread::Builder::new()
                    .name(format!("agent-shell-watcher-pid{pid}"))
                    .spawn(move || {
                        let status = child.wait();
                        let stdout = stdout_thread.join().unwrap_or_default();
                        let stderr = stderr_thread.join().unwrap_or_default();
                        // 리더와 출력이 모두 끝나야 Run 이 끝난다. 그 전의 취소는 그룹을 끝낸다.
                        run_group::forget(pid);
                        let outcome = match status {
                            Ok(status) => {
                                let success = match &allowed_exit {
                                    None => status.success(),
                                    Some(codes) => {
                                        status.code().is_some_and(|c| codes.contains(&c))
                                    }
                                };
                                shell_outcome_from_status(
                                    pid,
                                    status.code(),
                                    success,
                                    stdout,
                                    stderr,
                                )
                            }
                            Err(e) => PollOutcome::Failed(format!("Run wait: {e}")),
                        };
                        persist_run_result(&mem_clone, ws, &task_id_clone, &outcome);
                        // 결과를 기록하지 못하면 poll은 계속 Active라 poison을 알리고 cell을 사용한다.
                        *tasty_utils::poison::recover_mutex(
                            cell_clone.lock(),
                            "agent run result cell",
                            &RUN_RESULT_POISON_REPORTED,
                        ) = Some(outcome);
                    })
                    .map_err(|e| format!("Run watcher spawn '{program}': {e}"))?;
                self.shell_children.insert(
                    pid,
                    ShellChildEntry {
                        result: result_cell,
                        _watcher: watcher,
                    },
                );
                Ok(DispatchHandle::ShellProcess { pid })
            }
            TaskCommand::Custom {
                ipc_method,
                params,
                poll,
            } => {
                let params = &typed_inputs::custom_params(task, params);
                // IPC를 먼저 실행한 뒤 완료 전략을 해석한다. 전략 해석 실패가 이미 실행한 요청을 되돌리지는 않는다.
                // poll 미지정 시 기본 전략을 사용하고 그것도 없으면 응답으로 즉시 끝낸다.
                let value = self
                    .ctx
                    .dispatch_plugin(ipc_method, params.clone())
                    .map_err(|e| format!("Custom '{ipc_method}': {e}"))?;
                // 완료를 따로 기다리는 custom 이면 이 응답은 접수 응답이라 결과와 따로 남긴다.
                if poll.is_some() || self.ctx.completion.default_for_method(ipc_method).is_some() {
                    self.record_accepted(task, &value);
                }
                use tasty_agent::PollSpecRef;
                let spec: tasty_agent::PollSpec = match poll.as_deref() {
                    Some(PollSpecRef::Inline(spec)) => spec.clone(),
                    Some(PollSpecRef::Named { strategy }) => {
                        let strat = self.ctx.completion.named(strategy).map_err(|e| {
                            format!("Custom '{ipc_method}' poll strategy '{strategy}': {e}")
                        })?;
                        match strat.kind {
                            crate::completion::CompletionKind::Poll(spec) => spec,
                            crate::completion::CompletionKind::Push {
                                notify_via,
                                timeout_ms,
                            } => {
                                return self.dispatch_push_strategy(
                                    task,
                                    ipc_method,
                                    params,
                                    strat.id.as_str(),
                                    &notify_via,
                                    timeout_ms,
                                );
                            }
                        }
                    }
                    None => match self.ctx.completion.default_for_method(ipc_method) {
                        Some(strat) => match strat.kind {
                            crate::completion::CompletionKind::Poll(spec) => spec,
                            crate::completion::CompletionKind::Push {
                                notify_via,
                                timeout_ms,
                            } => {
                                return self.dispatch_push_strategy(
                                    task,
                                    ipc_method,
                                    params,
                                    strat.id.as_str(),
                                    &notify_via,
                                    timeout_ms,
                                );
                            }
                        },
                        None => {
                            return Ok(DispatchHandle::CustomImmediate(TaskResult {
                                exit_code: Some(0),
                                output: Some(value),
                                error: None,
                            }));
                        }
                    },
                };
                let spec = &spec;
                // 같은 poll 인자를 매핑하면 응답값이 요청값을 덮는다.
                let mut poll_params = serde_json::Map::new();
                for (req_key, poll_key) in &spec.map_from_request {
                    if let Some(v) = params.get(req_key) {
                        poll_params.insert(poll_key.clone(), v.clone());
                    }
                }
                for (resp_key, poll_key) in &spec.map_from_response {
                    if let Some(v) = value.get(resp_key) {
                        poll_params.insert(poll_key.clone(), v.clone());
                    }
                }
                let deadline_ms = spec.timeout_ms.map(|t| now_ms() + t);
                Ok(DispatchHandle::PolledDispatch {
                    workspace_id: task.workspace_id,
                    poll_method: spec.poll_method.clone(),
                    poll_params: serde_json::Value::Object(poll_params),
                    state_field: spec.state_field.clone(),
                    terminal_states: spec.terminal_states.clone(),
                    failure_states: spec.failure_states.clone(),
                    interval_ms: spec.interval_ms,
                    deadline_ms,
                })
            }
            TaskCommand::Agent { .. } => self.dispatch_agent(task),
            TaskCommand::WaitBarrier { .. } => Ok(DispatchHandle::BarrierPoll {
                workspace_id: task.workspace_id,
                name: task
                    .barrier_name()
                    .ok_or("WaitBarrier: no barrier name")?
                    .to_string(),
            }),
        }
    }

    /// params.surface_id에 command-completed 일회성 훅을 걸고 외부 완료를 기다린다.
    /// 현재 이벤트 종류는 고정이다. 훅 수신 또는 별도 만료 처리가 task를 종결하며 이 handle의 poll은 Active다.
    pub(super) fn dispatch_push_strategy(
        &mut self,
        task: &Task,
        ipc_method: &str,
        params: &serde_json::Value,
        strategy_id: &str,
        notify_via: &str,
        timeout_ms: u64,
    ) -> Result<DispatchHandle, String> {
        let surface_id = params
            .get("surface_id")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| {
                format!(
                    "Custom '{ipc_method}' push strategy '{strategy_id}': missing 'surface_id' \
                 param — push completion needs a target surface to bind the completion hook to"
                )
            })?;
        let hook_params = json!({
            "surface_id": surface_id,
            "event": "command-completed",
            "handler": notify_via,
            "once": true,
        });
        let hook_resp = self
            .ctx
            .dispatch_plugin("hook.set", hook_params)
            .map_err(|e| {
                format!("Custom '{ipc_method}' push strategy '{strategy_id}': hook.set failed: {e}")
            })?;
        let hook_id = hook_resp.get("hook_id").and_then(|v| v.as_u64()).ok_or_else(|| {
            format!(
                "Custom '{ipc_method}' push strategy '{strategy_id}': hook.set response missing 'hook_id'"
            )
        })?;
        let deadline_ms = now_ms() + timeout_ms;
        self.ctx.hook_task_waits.register_owned(
            hook_id,
            task.workspace_id,
            task.id.clone(),
            deadline_ms,
            crate::hook_wait::HookWaitOwner {
                agent_seq: self.ctx.agent_seq.clone(),
                completion: self.ctx.task_waker_hub.clone(),
            },
            dispatch_attempt(task),
        );
        // 훅 매핑은 재시작 때 사라져도 handle의 기한으로 reload에서 만료를 판단할 수 있게 한다.
        Ok(DispatchHandle::AwaitExternal {
            wait_key: hook_id.to_string(),
            deadline_ms,
        })
    }
}
