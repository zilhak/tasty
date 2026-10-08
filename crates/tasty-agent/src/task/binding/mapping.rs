//! 입력을 실행 인자로 넘기는 자리(`input_mapping`) — 생성 시 검사와 실행 직전 해석.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::super::contract::{TaskContract, TaskFailure};
use super::super::types::{TypeDefs, TypeKind, TypeSchema, TypedValue};
use super::super::{Task, TaskCommand, TaskId};
use super::{escape, input_failure, pointer_tokens, schema_at, typed_failure, value_at};

/// 검증된 입력을 실행 인자로 넘기는 자리. 지정하지 않은 자리에는 입력을 넣지 않는다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputMapping {
    /// run 전용. 입력의 JSON Pointer 목록. 각 값을 argv 끝에 요소 하나씩 붙인다.
    /// string·enum·int64·boolean 만 받는다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// run 전용. 입력 전체를 JSON 한 문서(wire 형식)로 stdin 에 쓰고 닫는다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stdin: bool,
    /// custom 전용. params 안의 JSON Pointer → 입력의 JSON Pointer. 타입을 유지해 넣는다.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub params: BTreeMap<String, String>,
    /// agent 전용. 입력 전체를 지시 뒤의 구조화된 입력 블록(JSON, wire 형식)으로 넣는다.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub input_block: bool,
    /// wait_barrier 전용. 기다릴 barrier 이름을 담은 입력의 JSON Pointer(string). command 의
    /// `name` 과 함께 쓸 수 없다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub barrier: Option<String>,
}

impl InputMapping {
    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
            && !self.stdin
            && self.params.is_empty()
            && !self.input_block
            && self.barrier.is_none()
    }
}

/// 입력에서 만든 실행 인자.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ResolvedExecution {
    /// run argv 끝에 붙인 요소.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// stdin 에 입력 전체를 썼는가.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stdin: bool,
    /// custom 에 실제로 넘긴 params.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
    /// agent 에 실제로 보낸 지시(입력 블록 포함).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    /// wait_barrier 가 입력에서 받은 barrier 이름.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub barrier: Option<String>,
}

impl ResolvedExecution {
    pub fn is_empty(&self) -> bool {
        self.args.is_empty()
            && !self.stdin
            && self.params.is_none()
            && self.instruction.is_none()
            && self.barrier.is_none()
    }
}

pub(super) fn check_mapping(
    task_id: &TaskId,
    defs: &TypeDefs,
    input: &TypeSchema,
    contract: &TaskContract,
    command: &TaskCommand,
    at: &str,
) -> Result<(), TaskFailure> {
    let mapping = contract.input_mapping.clone().unwrap_or_default();
    let loc = format!("{at}/input_mapping");
    let fail = |msg: String, l: String| input_failure(format!("task {task_id}: {msg}"), l);
    let kind = super::super::contract::command_kind(command);
    let is_run = matches!(command, TaskCommand::Run { .. });
    let is_custom = matches!(command, TaskCommand::Custom { .. });
    let is_agent = matches!(command, TaskCommand::Agent { .. });
    if let TaskCommand::WaitBarrier { name } = command {
        check_barrier_source(name.as_deref(), mapping.barrier.is_some(), at, &fail)?;
    } else if mapping.barrier.is_some() {
        return Err(fail(
            format!("input_mapping barrier applies to wait_barrier, not {kind}"),
            format!("{loc}/barrier"),
        ));
    }
    if mapping.input_block && !is_agent {
        return Err(fail(
            format!("input_mapping input_block applies to agent, not {kind}"),
            loc,
        ));
    }
    if (!mapping.args.is_empty() || mapping.stdin) && !is_run {
        return Err(fail(
            format!("input_mapping args/stdin apply to run, not {kind}"),
            loc,
        ));
    }
    if !mapping.params.is_empty() && !is_custom {
        return Err(fail(
            format!("input_mapping params apply to custom, not {kind}"),
            loc,
        ));
    }
    let input_is_unit = matches!(
        defs.resolve(input).map(|r| r.kind.clone()),
        Ok(TypeKind::Unit)
    );
    if !input_is_unit && is_agent && !mapping.input_block {
        return Err(fail(
            "agent input reaches the session only through input_mapping input_block".into(),
            loc,
        ));
    }
    if mapping.input_block && input_is_unit {
        return Err(fail(
            "input_mapping input_block needs a non-unit input".into(),
            format!("{loc}/input_block"),
        ));
    }
    if !input_is_unit && !is_run && !is_custom && !is_agent && mapping.barrier.is_none() {
        return Err(fail(
            format!("{kind} has no input mapping; its input_schema must be unit"),
            format!("{at}/input_schema"),
        ));
    }
    if mapping.stdin && input_is_unit {
        return Err(fail(
            "input_mapping stdin needs a non-unit input".into(),
            format!("{loc}/stdin"),
        ));
    }
    let present_at = |pointer: &str, l: String| -> Result<TypeSchema, TaskFailure> {
        let (s, may_be_absent) = schema_at(defs, input, pointer).map_err(|e| {
            let msg = format!("task {task_id}: input pointer '{pointer}': {e}");
            typed_failure(Some(task_id), e, l.clone(), Some(msg))
        })?;
        if may_be_absent {
            return Err(fail(
                format!(
                    "input pointer '{pointer}' may be absent; mapped inputs must always be present"
                ),
                l,
            ));
        }
        Ok(s)
    };
    for (i, pointer) in mapping.args.iter().enumerate() {
        let l = format!("{loc}/args/{i}");
        let s = present_at(pointer, l.clone())?;
        let r = defs
            .resolve(&s)
            .map_err(|e| typed_failure(Some(task_id), e, l.clone(), None))?;
        let scalar = matches!(
            r.kind,
            TypeKind::String { .. } | TypeKind::Enum { .. } | TypeKind::Int64 | TypeKind::Boolean
        ) && !r.nullable;
        if !scalar {
            return Err(fail(
                format!(
                    "argv value at '{pointer}' is {}; argv takes string, enum, int64 or boolean (pass structured input through stdin)",
                    s.describe()
                ),
                l,
            ));
        }
    }
    if let Some(pointer) = &mapping.barrier {
        let l = format!("{loc}/barrier");
        let s = present_at(pointer, l.clone())?;
        let r = defs
            .resolve(&s)
            .map_err(|e| typed_failure(Some(task_id), e, l.clone(), None))?;
        if !matches!(r.kind, TypeKind::String { .. }) || r.nullable {
            return Err(fail(
                format!(
                    "barrier name at '{pointer}' is {}; a barrier name is a string",
                    s.describe()
                ),
                l,
            ));
        }
    }
    if let TaskCommand::Custom { params, .. } = command {
        for (param_ptr, input_ptr) in &mapping.params {
            let l = format!("{loc}/params/{}", escape(param_ptr));
            present_at(input_ptr, l.clone())?;
            let tokens = pointer_tokens(param_ptr).map_err(|m| fail(m, l.clone()))?;
            let Some((_, parent)) = tokens.split_last() else {
                return Err(fail(
                    "a params pointer must name a field, not the whole params".into(),
                    l,
                ));
            };
            let mut cur = params;
            for t in parent {
                cur = match cur.get(t.as_str()) {
                    Some(v) => v,
                    None => {
                        return Err(fail(
                            format!(
                                "params pointer '{param_ptr}': parent object is missing in params"
                            ),
                            l,
                        ));
                    }
                };
            }
            if !(cur.is_object() || (parent.is_empty() && cur.is_null())) {
                return Err(fail(
                    format!("params pointer '{param_ptr}': parent is not an object"),
                    l,
                ));
            }
        }
    }
    Ok(())
}

/// wait_barrier 의 이름 출처가 정확히 하나인지, 정적 이름이 barrier 이름 규칙에 맞는지 본다.
fn check_barrier_source(
    name: Option<&str>,
    from_input: bool,
    at: &str,
    fail: &dyn Fn(String, String) -> TaskFailure,
) -> Result<(), TaskFailure> {
    match (name, from_input) {
        (Some(_), true) => Err(fail(
            "wait_barrier takes its barrier name from command name or input_mapping barrier, not both"
                .into(),
            format!("{at}/input_mapping/barrier"),
        )),
        (None, false) => Err(fail(
            "wait_barrier needs a barrier name: set command name or input_mapping barrier".into(),
            format!("{at}/command"),
        )),
        (Some(n), false) => crate::barrier::check_barrier_name(n)
            .map_err(|e| fail(e.to_string(), format!("{at}/command/name"))),
        (None, true) => Ok(()),
    }
}

pub(super) fn resolve_execution(
    task: &Task,
    contract: &TaskContract,
    value: &TypedValue,
    base_params: Option<&Value>,
) -> Result<ResolvedExecution, TaskFailure> {
    let Some(mapping) = contract.input_mapping.as_ref().filter(|m| !m.is_empty()) else {
        return Ok(ResolvedExecution::default());
    };
    let internal = value.to_internal();
    let fail = |msg: String, loc: String| input_failure(format!("task {}: {msg}", task.id), loc);
    let mut out = ResolvedExecution {
        stdin: mapping.stdin,
        ..Default::default()
    };
    for (i, pointer) in mapping.args.iter().enumerate() {
        let loc = format!("/input_mapping/args/{i}");
        let arg = match value_at(&internal, pointer) {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Number(n)) if n.is_i64() => n.to_string(),
            other => {
                return Err(fail(
                    format!(
                        "argv value at '{pointer}' is not a string, int64 or boolean: {other:?}"
                    ),
                    loc,
                ));
            }
        };
        out.args.push(arg);
    }
    if !mapping.params.is_empty() {
        let mut params = base_params.cloned().unwrap_or(Value::Null);
        if params.is_null() {
            params = Value::Object(Map::new());
        }
        for (param_ptr, input_ptr) in &mapping.params {
            let loc = format!("/input_mapping/params/{}", escape(param_ptr));
            let v = value_at(&internal, input_ptr)
                .cloned()
                .ok_or_else(|| fail(format!("input has nothing at '{input_ptr}'"), loc.clone()))?;
            let tokens = pointer_tokens(param_ptr).map_err(|m| fail(m, loc.clone()))?;
            let Some((last, parent)) = tokens.split_last() else {
                return Err(fail("empty params pointer".into(), loc));
            };
            let mut cur = &mut params;
            for t in parent {
                cur = cur
                    .get_mut(t.as_str())
                    .ok_or_else(|| fail(format!("params has no '{t}'"), loc.clone()))?;
            }
            let Value::Object(m) = cur else {
                return Err(fail(
                    format!("params pointer '{param_ptr}': parent is not an object"),
                    loc,
                ));
            };
            m.insert(last.clone(), v);
        }
        out.params = Some(params);
    }
    if let Some(pointer) = &mapping.barrier {
        let loc = "/input_mapping/barrier".to_string();
        let name = match value_at(&internal, pointer) {
            Some(Value::String(s)) => s.clone(),
            other => {
                return Err(fail(
                    format!("barrier name at '{pointer}' is not a string: {other:?}"),
                    loc,
                ));
            }
        };
        crate::barrier::check_barrier_name(&name).map_err(|e| fail(e.to_string(), loc))?;
        out.barrier = Some(name);
    }
    if mapping.input_block
        && let TaskCommand::Agent { instruction, .. } = &task.command
    {
        out.instruction = Some(super::super::agent::compose_instruction(
            instruction,
            Some(&value.to_wire()),
        ));
    }
    Ok(out)
}
