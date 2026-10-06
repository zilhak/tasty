//! Resolve lease placeholders and frozen upstream outputs before dispatch.

use crate::task_output_ref;
use std::collections::HashMap;
use tasty_agent::{TaskCommand, TaskId};

/// 실행 전에 lease로 받은 자원을 넣는 표식. Run 인자·cwd와 Custom의 문자열 값에 적용한다.
const LEASE_RESOURCE_PLACEHOLDER: &str = "${lease.resource}";

/// Run의 cwd가 없으면 lease 자원으로 채우고, 있으면 표식만 치환한다.
/// Custom의 JSON 문자열 값도 치환하며 Reduce·WaitBarrier는 바꾸지 않는다.
pub(super) fn substitute_lease_resource(command: &mut TaskCommand, resource: &str) {
    match command {
        TaskCommand::Run { command, cwd, .. } => {
            for arg in command.iter_mut() {
                if arg.contains(LEASE_RESOURCE_PLACEHOLDER) {
                    *arg = arg.replace(LEASE_RESOURCE_PLACEHOLDER, resource);
                }
            }
            match cwd {
                None => *cwd = Some(std::path::PathBuf::from(resource)),
                Some(existing) => {
                    if let Some(s) = existing.to_str()
                        && s.contains(LEASE_RESOURCE_PLACEHOLDER)
                    {
                        *cwd = Some(std::path::PathBuf::from(
                            s.replace(LEASE_RESOURCE_PLACEHOLDER, resource),
                        ));
                    }
                }
            }
        }
        TaskCommand::Custom { params, .. } => {
            substitute_lease_resource_in_json(params, resource);
        }
        _ => {}
    }
}

fn substitute_lease_resource_in_json(value: &mut serde_json::Value, resource: &str) {
    match value {
        serde_json::Value::String(s) => {
            if s.contains(LEASE_RESOURCE_PLACEHOLDER) {
                *s = s.replace(LEASE_RESOURCE_PLACEHOLDER, resource);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                substitute_lease_resource_in_json(v, resource);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                substitute_lease_resource_in_json(v, resource);
            }
        }
        _ => {}
    }
}

/// JSON 문자열 전체가 표식 하나이면 원래 값의 타입을 유지한다. 다른 글자와 섞이면 문자열로 보간한다.
/// Run 인자·cwd는 문자열로 보간하며 바뀐 값이 있으면 true다. 문법은 task_output_ref가 담당한다.
pub(super) fn substitute_task_outputs(
    command: &mut TaskCommand,
    outputs: &HashMap<TaskId, serde_json::Value>,
) -> Result<bool, String> {
    let mut changed = false;
    match command {
        TaskCommand::Run { command, cwd, .. } => {
            for arg in command.iter_mut() {
                if let Some(next) = interpolate_string(arg, outputs)? {
                    *arg = next;
                    changed = true;
                }
            }
            if let Some(p) = cwd.as_ref()
                && let Some(s) = p.to_str()
                && let Some(next) = interpolate_string(s, outputs)?
            {
                *cwd = Some(std::path::PathBuf::from(next));
                changed = true;
            }
        }
        TaskCommand::Custom { params, .. } => {
            substitute_task_outputs_in_json(params, outputs, &mut changed)?;
        }
        _ => {}
    }
    Ok(changed)
}

fn substitute_task_outputs_in_json(
    value: &mut serde_json::Value,
    outputs: &HashMap<TaskId, serde_json::Value>,
    changed: &mut bool,
) -> Result<(), String> {
    match value {
        serde_json::Value::String(s) => {
            let refs = task_output_ref::parse_refs(s).map_err(|e| e.0)?;
            if refs.is_empty() {
                return Ok(());
            }
            if refs.len() == 1 && refs[0].0.start == 0 && refs[0].0.end == s.len() {
                *value = resolve(&refs[0].1, outputs)?.clone();
            } else if let Some(next) = interpolate_string(s, outputs)? {
                *s = next;
            }
            *changed = true;
        }
        serde_json::Value::Array(arr) => {
            for v in arr.iter_mut() {
                substitute_task_outputs_in_json(v, outputs, changed)?;
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values_mut() {
                substitute_task_outputs_in_json(v, outputs, changed)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// 문자열 결과는 내용만, 나머지 값은 compact JSON으로 넣는다.
/// 원문 표식만 한 번 치환하며 주입한 값에 든 표식을 다시 해석하지 않는다.
fn interpolate_string(
    s: &str,
    outputs: &HashMap<TaskId, serde_json::Value>,
) -> Result<Option<String>, String> {
    let refs = task_output_ref::parse_refs(s).map_err(|e| e.0)?;
    if refs.is_empty() {
        return Ok(None);
    }
    let mut out = String::with_capacity(s.len());
    let mut cursor = 0usize;
    for (range, r) in &refs {
        out.push_str(&s[cursor..range.start]);
        let v = resolve(r, outputs)?;
        match v {
            serde_json::Value::String(text) => out.push_str(text),
            other => out.push_str(&other.to_string()),
        }
        cursor = range.end;
    }
    out.push_str(&s[cursor..]);
    Ok(Some(out))
}

fn resolve<'a>(
    r: &task_output_ref::TaskOutputRef,
    outputs: &'a HashMap<TaskId, serde_json::Value>,
) -> Result<&'a serde_json::Value, String> {
    let output = outputs.get(&r.task_id).ok_or_else(|| {
        format!(
            "task output reference '{}': upstream task has no result output yet",
            r.task_id
        )
    })?;
    output.pointer(&r.pointer).ok_or_else(|| {
        format!(
            "task output reference '{}': JSON pointer '{}' not found in output {}",
            r.task_id,
            if r.pointer.is_empty() { "" } else { &r.pointer },
            output
        )
    })
}
