//! 제출한 그래프 JSON 을 [`TaskGraphSpec`] 으로 읽는다. 형식 오류(모르는 키, 필수 필드 누락,
//! 타입이 다른 값)에도 의미 검증 오류처럼 제출 정의 안의 위치(JSON Pointer)를 싣는다.
//!
//! serde 오류는 위치를 알려 주지 않는다. 그래서 실패한 정의에서 키나 원소를 하나씩 빼 보고,
//! 빼서 오류가 사라지는(또는 그 키만 없다는 오류로 바뀌는) 자리로 내려간다. 내려갈 수 없으면
//! 그 자리가 위치다. 필수 필드가 빠진 object 는 그 object 가 위치이고 메시지가 필드를 말한다.
//! 키가 둘 이상 틀렸으면 하나를 빼도 오류가 남아 그 위의 object 에서 멈춘다.

use serde::de::DeserializeOwned;
use serde_json::Value;

use super::super::TaskId;
use super::super::contract::{FailureStage, TaskFailure};
use super::TaskGraphSpec;
use super::graph_submit::GraphTaskSpec;
use crate::{AgentError, Result};

impl TaskGraphSpec {
    /// 그래프 JSON 을 읽는다. 실패하면 `AgentError::TypeContract` 로 위치와 task id 를 싣는다.
    pub fn from_json(value: &Value) -> Result<Self> {
        let error = match serde_json::from_value::<Self>(value.clone()) {
            Ok(spec) => return Ok(spec),
            Err(e) => e.to_string(),
        };
        // task 목록은 task 하나씩 읽어 틀린 task 를 찾는다. 그래프 전체를 task 수만큼 다시 읽지 않는다.
        let task_failure = value
            .get("tasks")
            .and_then(Value::as_array)
            .and_then(|tasks| {
                tasks.iter().enumerate().find_map(|(i, task)| {
                    let message = serde_json::from_value::<GraphTaskSpec>(task.clone())
                        .err()?
                        .to_string();
                    let path = narrow::<GraphTaskSpec>(task);
                    let task_id = task
                        .get("id")
                        .and_then(Value::as_str)
                        .map(|id| -> TaskId { id.to_string() });
                    Some((format!("/tasks/{i}{path}"), message, task_id))
                })
            });
        let (location, message, task_id) =
            task_failure.unwrap_or_else(|| (narrow::<Self>(value), error, None));
        Err(AgentError::TypeContract(Box::new(TaskFailure {
            location: Some(location.clone()),
            task_id,
            ..TaskFailure::new(
                FailureStage::Input,
                format!("invalid graph at '{location}': {message}"),
            )
        })))
    }
}

/// `root` 를 `T` 로 읽지 못하게 만드는 자리의 JSON Pointer(`root` 기준, 루트면 빈 문자열).
fn narrow<T: DeserializeOwned>(root: &Value) -> String {
    let fails = |v: &Value| {
        serde_json::from_value::<T>(v.clone())
            .err()
            .map(|e| e.to_string())
    };
    // 원래 오류가 필드 누락이면 다른 필수 필드를 빼서 생긴 누락 오류를 그 필드의 탓으로 보지 않는다.
    let missing_field_error = fails(root).is_some_and(|e| e.starts_with("missing field"));
    let mut path: Vec<String> = Vec::new();
    'descend: while let Some(node) = root.pointer(&pointer(&path)) {
        let children: Vec<String> = match node {
            Value::Object(map) => map.keys().cloned().collect(),
            Value::Array(items) => (0..items.len()).map(|i| i.to_string()).collect(),
            _ => break,
        };
        for child in children {
            let mut probe = root.clone();
            let removed = match probe.pointer_mut(&pointer(&path)) {
                Some(Value::Object(map)) => map.remove(&child).is_some(),
                Some(Value::Array(items)) => child
                    .parse::<usize>()
                    .ok()
                    .filter(|i| *i < items.len())
                    .map(|i| items.remove(i))
                    .is_some(),
                _ => false,
            };
            if !removed {
                continue;
            }
            let culprit = match fails(&probe) {
                None => true,
                Some(e) => !missing_field_error && e == format!("missing field `{child}`"),
            };
            if culprit {
                path.push(child);
                continue 'descend;
            }
        }
        break;
    }
    pointer(&path)
}

fn pointer(path: &[String]) -> String {
    path.iter()
        .map(|p| format!("/{}", p.replace('~', "~0").replace('/', "~1")))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn located(graph: Value) -> TaskFailure {
        match TaskGraphSpec::from_json(&graph) {
            Err(AgentError::TypeContract(f)) => *f,
            other => panic!("expected a located error, got {other:?}"),
        }
    }

    fn run_task(extra: Value) -> Value {
        let mut task =
            json!({"id": "x", "command": {"kind": "run", "workspace_id": 1, "command": ["true"]}});
        task.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        task
    }

    #[test]
    fn an_unknown_task_key_is_located_at_that_key() {
        let f = located(
            json!({"contract_version": 2, "tasks": [run_task(json!({})), run_task(json!({"id": "y", "bogus": 1}))]}),
        );
        assert_eq!(f.location.as_deref(), Some("/tasks/1/bogus"));
        assert_eq!(f.task_id.as_deref(), Some("y"));
        assert!(f.message.contains("unknown field `bogus`"), "{}", f.message);
    }

    #[test]
    fn a_missing_nested_field_is_located_at_its_object() {
        let f = located(
            json!({"contract_version": 2, "tasks": [run_task(json!({"postprocess": {"command": ["judge"]}}))]}),
        );
        assert_eq!(f.location.as_deref(), Some("/tasks/0/postprocess"));
        assert!(
            f.message.contains("missing field `timeout_ms`"),
            "{}",
            f.message
        );
    }

    #[test]
    fn an_unknown_nested_key_is_located_at_that_key() {
        let f = located(
            json!({"contract_version": 2, "tasks": [run_task(json!({"postprocess": {"command": ["judge"], "timeout_ms": 10, "bogus": true}}))]}),
        );
        assert_eq!(f.location.as_deref(), Some("/tasks/0/postprocess/bogus"));
    }

    #[test]
    fn a_wrong_value_type_in_a_required_field_is_located_at_that_field() {
        let f = located(json!({"contract_version": 2, "tasks": [{"id": "x", "command": 5}]}));
        assert_eq!(f.location.as_deref(), Some("/tasks/0/command"));
    }

    #[test]
    fn a_graph_level_error_is_located_outside_the_tasks() {
        let f = located(
            json!({"contract_version": 2, "tasks": [run_task(json!({}))], "durability": "sometimes"}),
        );
        assert_eq!(f.location.as_deref(), Some("/durability"));
        assert_eq!(f.task_id, None);
        let f = located(json!({"tasks": []}));
        assert_eq!(f.location.as_deref(), Some(""));
        assert!(
            f.message.contains("missing field `contract_version`"),
            "{}",
            f.message
        );
    }

    #[test]
    fn a_valid_graph_reads_as_before() {
        let graph = json!({"contract_version": 2, "tasks": [run_task(json!({}))]});
        assert_eq!(TaskGraphSpec::from_json(&graph).unwrap().tasks.len(), 1);
    }
}
