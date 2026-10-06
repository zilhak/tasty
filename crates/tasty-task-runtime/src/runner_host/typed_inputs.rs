//! v2 입력을 실행 직전에 해석해 snapshot 으로 고정하고 실행 인자에 넣는다.
//!
//! 해석은 lease·v1 placeholder 치환 뒤에 한다. 입력 값은 snapshot 의 `execution` 이 정한
//! 자리(run argv 요소·stdin, custom params)에 값 그대로 들어가며 다시 치환하지 않는다.
//! 원본 command 는 저장소에서 바꾸지 않는다.

use serde_json::Value;
use tasty_agent::task::binding::{needs_input, resolve_inputs};
use tasty_agent::{Task, TaskCommand, TaskStore};
use tasty_memory::HOST_OWNER;

use super::{HostExecutor, now_ms};

impl HostExecutor {
    /// v2 task 의 입력을 해석해 저장하고 `task` 에 붙인다. 해석 또는 저장에 실패하면 실행하지
    /// 않는다. 실패 사유는 snapshot 에 남아 결과의 실패 단계가 input 이 된다.
    pub(super) fn resolve_typed_inputs(&mut self, task: &mut Task) -> Result<(), String> {
        let Some(contract) = task.contract.clone() else {
            return Ok(());
        };
        if !needs_input(&contract) {
            return Ok(());
        }
        let base_params = match &task.command {
            TaskCommand::Custom { params, .. } => Some(params.clone()),
            _ => None,
        };
        let seq = self.ctx.agent_seq.clone();
        let ws = task.workspace_id;
        let snapshot = self.ctx.with_memory(|mem| {
            let mut store = TaskStore::new(mem, HOST_OWNER, seq.as_ref());
            let snapshot = {
                let lookup = |id: &String| store.get(ws, id).ok().flatten();
                resolve_inputs(task, &contract, base_params.as_ref(), now_ms(), &lookup)
            };
            store
                .set_input_snapshot(ws, &task.id, snapshot.clone())
                .map(|_| snapshot)
                .map_err(|e| format!("input snapshot could not be stored: {e}"))
        })?;
        if let Some(f) = &snapshot.failure {
            return Err(format!("input: {}", f.message));
        }
        task.input_snapshot = Some(snapshot);
        Ok(())
    }
}

/// run 의 argv. 입력에서 만든 요소를 원본 argv 뒤에 붙인다.
pub(super) fn run_argv(task: &Task, command: &[String]) -> Vec<String> {
    let mut argv = command.to_vec();
    if let Some(s) = &task.input_snapshot {
        argv.extend(s.execution.args.iter().cloned());
    }
    argv
}

/// run 의 stdin 에 쓸 입력 전체(wire 형식 JSON). 매핑하지 않았으면 없다.
pub(super) fn run_stdin(task: &Task) -> Option<Vec<u8>> {
    let s = task.input_snapshot.as_ref()?;
    if !s.execution.stdin {
        return None;
    }
    serde_json::to_vec(&s.value).ok()
}

/// custom 에 넘길 params. 입력을 매핑했으면 snapshot 의 값을 쓴다.
pub(super) fn custom_params(task: &Task, params: &Value) -> Value {
    task.input_snapshot
        .as_ref()
        .and_then(|s| s.execution.params.clone())
        .unwrap_or_else(|| params.clone())
}
