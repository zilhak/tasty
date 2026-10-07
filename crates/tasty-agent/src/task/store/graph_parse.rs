//! 제출한 그래프 JSON 을 [`TaskGraphSpec`] 으로 읽는다. 형식 오류(모르는 키, 필수 필드 누락,
//! 타입이 다른 값)에도 의미 검증 오류처럼 제출 정의 안의 위치(JSON Pointer)를 싣는다.
//!
//! serde 오류는 위치를 알려 주지 않는다. 그래서 정의를 조금씩 바꿔 다시 읽어 보며 위치를 좁힌다.
//! - 필수 필드 누락: 그 필드를 `null` 로 넣었을 때 오류가 바뀌는 가장 깊은 object 다.
//! - 그 밖: 키나 원소를 하나씩 빼 보고, 빼서 오류가 사라지는 자리로 내려간다. 빼서 그 키만
//!   없다는 오류로 바뀌는 필수 키는, 오류 메시지가 가리키는 값(모르는 키 이름, 틀린 값)을 그
//!   키 안에서 바꿨을 때 오류가 달라질 때만 내려간다. 태그 enum 의 `kind` 처럼 빼면 먼저
//!   걸리는 필수 키를 탓하지 않기 위해서다. 더 내려갈 수 없는 object 에서 메시지가 그 object 의
//!   키를 이름으로 가리키면(모르는 키) 그 키가 위치다. 아니면 그 object 에서 멈춘다.

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
        let task_failure = tasks(value).find_map(|(i, task)| {
            let message = serde_json::from_value::<GraphTaskSpec>(task.clone())
                .err()?
                .to_string();
            let path = narrow::<GraphTaskSpec>(task);
            Some((format!("/tasks/{i}{path}"), message, task_id(task)))
        });
        let (location, message, task_id) =
            task_failure.unwrap_or_else(|| (narrow::<Self>(value), error, None));
        Err(located(location, message, task_id))
    }
}

fn located(location: String, message: String, task_id: Option<TaskId>) -> AgentError {
    AgentError::TypeContract(Box::new(TaskFailure {
        location: Some(location.clone()),
        task_id,
        ..TaskFailure::new(
            FailureStage::Input,
            format!("invalid graph at '{location}': {message}"),
        )
    }))
}

fn tasks(value: &Value) -> impl Iterator<Item = (usize, &Value)> {
    value
        .get("tasks")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
}

fn task_id(task: &Value) -> Option<TaskId> {
    task.get("id").and_then(Value::as_str).map(str::to_string)
}

/// `root` 를 `T` 로 읽지 못하게 만드는 자리의 JSON Pointer(`root` 기준, 루트면 빈 문자열).
fn narrow<T: DeserializeOwned>(root: &Value) -> String {
    let fails = |v: &Value| {
        serde_json::from_value::<T>(v.clone())
            .err()
            .map(|e| e.to_string())
    };
    let Some(original) = fails(root) else {
        return String::new();
    };
    if let Some(field) = quoted_after(&original, "missing field ") {
        return missing_field_owner(root, &field, &fails);
    }
    let witness = Witness::parse(&original);
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
            let mut child_path = path.clone();
            child_path.push(child.clone());
            let culprit = match fails(&probe) {
                None => true,
                Some(e) => {
                    e == format!("missing field `{child}`")
                        && witness.implicates(root, &child_path, &original, &fails)
                }
            };
            if culprit {
                path = child_path;
                continue 'descend;
            }
        }
        // 다른 오류가 함께 있어 빼도 읽히지 않지만, 메시지가 이 object 의 키를 이름으로 가리킨다.
        if let (Witness::Key(key), Value::Object(map)) = (&witness, node)
            && map.contains_key(key)
        {
            path.push(key.clone());
        }
        break;
    }
    pointer(&path)
}

/// 필드 `field` 가 빠진 object. 그 필드를 `null` 로 넣어 오류가 누락이 아닌 것(타입 오류 등)으로
/// 바뀌는 가장 깊은 object 다. 그 필드를 모르는 object 는 무시하거나 모르는 키로 답한다.
fn missing_field_owner(
    root: &Value,
    field: &str,
    fails: &dyn Fn(&Value) -> Option<String>,
) -> String {
    let missing = format!("missing field `{field}`");
    let unknown = format!("unknown field `{field}`");
    let mut objects = object_paths(root);
    objects.sort_by_key(|path| std::cmp::Reverse(path.len()));
    objects
        .into_iter()
        .find(|path| {
            let mut probe = root.clone();
            let Some(Value::Object(map)) = probe.pointer_mut(&pointer(path)) else {
                return false;
            };
            if map.contains_key(field) {
                return false;
            }
            map.insert(field.to_string(), Value::Null);
            fails(&probe).is_none_or(|e| e != missing && !e.starts_with(&unknown))
        })
        .map(|path| pointer(&path))
        .unwrap_or_default()
}

/// `root` 안의 모든 object 의 경로. 바깥 object 가 앞에 온다.
fn object_paths(root: &Value) -> Vec<Vec<String>> {
    fn walk(value: &Value, path: &mut Vec<String>, out: &mut Vec<Vec<String>>) {
        let children: Vec<(String, &Value)> = match value {
            Value::Object(map) => {
                out.push(path.clone());
                map.iter().map(|(k, v)| (k.clone(), v)).collect()
            }
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect(),
            _ => return,
        };
        for (key, child) in children {
            path.push(key);
            walk(child, path, out);
            path.pop();
        }
    }
    let mut out = Vec::new();
    walk(root, &mut Vec::new(), &mut out);
    out
}

/// 오류 메시지가 가리키는 것. 그 키 안에서 이것을 바꿨을 때 오류가 달라지면 그 키가 원인이다.
enum Witness {
    /// `unknown field `k``: 이 이름의 키.
    Key(String),
    /// 틀린 값(`invalid type: string "x"`, `unknown variant `v`` 등).
    Value(Value),
    /// 값을 특정할 수 없다(`map`, `sequence`). 이 타입의 값이 그 키 안에 있으면 원인으로 본다.
    Kind(fn(&Value) -> bool),
    /// 메시지에서 아무것도 읽지 못했다. 그 키만 없다는 오류만으로 내려간다.
    Unknown,
}

impl Witness {
    fn parse(message: &str) -> Self {
        if let Some(key) = quoted_after(message, "unknown field ") {
            return Self::Key(key);
        }
        if let Some(variant) = quoted_after(message, "unknown variant ") {
            return Self::Value(Value::String(variant));
        }
        let Some(rest) = ["invalid type: ", "invalid value: "]
            .iter()
            .find_map(|p| message.strip_prefix(p))
        else {
            return Self::Unknown;
        };
        let found = rest.split(", expected").next().unwrap_or_default();
        if let Some(text) = found.strip_prefix("string ") {
            return serde_json::from_str::<String>(text)
                .map(|s| Self::Value(Value::String(s)))
                .unwrap_or(Self::Kind(Value::is_string));
        }
        let number = ["integer ", "floating point "]
            .iter()
            .find_map(|p| found.strip_prefix(p))
            .and_then(|n| serde_json::from_str::<Value>(n.trim_matches('`')).ok());
        if let Some(n) = number {
            return Self::Value(n);
        }
        match found {
            "boolean `true`" => Self::Value(Value::Bool(true)),
            "boolean `false`" => Self::Value(Value::Bool(false)),
            "null" | "unit value" => Self::Value(Value::Null),
            "map" => Self::Kind(Value::is_object),
            "sequence" => Self::Kind(Value::is_array),
            _ => Self::Unknown,
        }
    }

    /// `at` 의 값 안에 이것이 있어 원래 오류를 낸다고 볼 수 있는가.
    fn implicates(
        &self,
        root: &Value,
        at: &[String],
        original: &str,
        fails: &dyn Fn(&Value) -> Option<String>,
    ) -> bool {
        let mut probe = root.clone();
        let Some(target) = probe.pointer_mut(&pointer(at)) else {
            return false;
        };
        match self {
            Self::Unknown => true,
            Self::Kind(is) => contains(target, &|v| is(v)),
            Self::Key(key) => rename_key(target, key) && fails(&probe).as_deref() != Some(original),
            Self::Value(value) => {
                replace_value(target, value) && fails(&probe).as_deref() != Some(original)
            }
        }
    }
}

fn contains(value: &Value, is: &dyn Fn(&Value) -> bool) -> bool {
    is(value)
        || match value {
            Value::Object(map) => map.values().any(|v| contains(v, is)),
            Value::Array(items) => items.iter().any(|v| contains(v, is)),
            _ => false,
        }
}

/// `key` 키를 모두 다른 이름으로 바꾼다. 바꾼 것이 있으면 true.
fn rename_key(value: &mut Value, key: &str) -> bool {
    let mut renamed = false;
    if let Value::Object(map) = value
        && let Some(v) = map.remove(key)
    {
        map.insert(format!("{key}~"), v);
        renamed = true;
    }
    match value {
        Value::Object(map) => map
            .values_mut()
            .fold(renamed, |r, v| rename_key(v, key) | r),
        Value::Array(items) => items
            .iter_mut()
            .fold(renamed, |r, v| rename_key(v, key) | r),
        _ => renamed,
    }
}

/// `target` 과 같은 값을 모두 같은 타입의 다른 값으로 바꾼다. 바꾼 것이 있으면 true.
fn replace_value(value: &mut Value, target: &Value) -> bool {
    if same_value(value, target) {
        let other = match &*value {
            Value::String(s) => Value::String(format!("{s}~")),
            Value::Bool(b) => Value::Bool(!*b),
            Value::Number(n) => n
                .as_f64()
                .and_then(|f| serde_json::Number::from_f64(f + 1.0))
                .map_or(Value::Bool(false), Value::Number),
            _ => Value::Bool(false),
        };
        *value = other;
        return true;
    }
    match value {
        Value::Object(map) => map
            .values_mut()
            .fold(false, |r, v| replace_value(v, target) | r),
        Value::Array(items) => items
            .iter_mut()
            .fold(false, |r, v| replace_value(v, target) | r),
        _ => false,
    }
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// `message` 가 `prefix` 로 시작하면 그 뒤 백틱 안의 이름.
fn quoted_after(message: &str, prefix: &str) -> Option<String> {
    let rest = message.strip_prefix(prefix)?.strip_prefix('`')?;
    Some(rest[..rest.find('`')?].to_string())
}

fn pointer(path: &[String]) -> String {
    path.iter().map(|p| format!("/{}", escape(p))).collect()
}

fn escape(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
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

    fn graph(tasks: Value) -> Value {
        json!({"contract_version": 2, "tasks": tasks})
    }

    fn location(graph: Value) -> String {
        located(graph).location.expect("location")
    }

    /// 형식 오류 사례마다 위치를 고정한다. 태그 enum 안의 오류가 `kind` 를 가리키지 않는다.
    #[test]
    fn every_shape_error_is_located_where_it_is() {
        let run = |extra: Value| {
            let mut cmd = json!({"kind": "run", "workspace_id": 1, "command": ["true"]});
            cmd.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            cmd
        };
        let cases: Vec<(Value, &str)> = vec![
            (
                graph(json!([run_task(json!({"bogus": 1}))])),
                "/tasks/0/bogus",
            ),
            (
                json!({"contract_version": 2, "tasks": [], "bogus": 1}),
                "/bogus",
            ),
            (json!({"contract_version": 2, "tasks2": []}), "/tasks2"),
            (
                graph(json!([{"id": "x", "command": run(json!({"command": "true"}))}])),
                "/tasks/0/command/command",
            ),
            (
                graph(json!([{"id": "x", "command": {"kind": "runx", "workspace_id": 1}}])),
                "/tasks/0/command/kind",
            ),
            (
                graph(json!([run_task(json!({"depends_on": "a"}))])),
                "/tasks/0/depends_on",
            ),
            (
                graph(json!([run_task(
                    json!({"postprocess": {"command": ["j"], "timeout_ms": "10"}})
                )])),
                "/tasks/0/postprocess/timeout_ms",
            ),
            (
                graph(json!([run_task(
                    json!({"postprocess": {"command": ["j"], "timeout_ms": 10, "stdout": {"format": "yaml"}}})
                )])),
                "/tasks/0/postprocess/stdout/format",
            ),
            // 모르는 키가 둘이면 메시지가 말하는 첫 키다.
            (
                graph(json!([run_task(json!({"bogus": 1, "bogus2": 2}))])),
                "/tasks/0/bogus",
            ),
            (
                graph(
                    json!([{"id": "x", "command": {"kind": "reduce", "inputs": "a", "strategy": {"kind": "all"}}}]),
                ),
                "/tasks/0/command/inputs",
            ),
            // 태그 enum 안의 타입 오류. task id 가 틀린 값과 같아도 그 값을 가리킨다.
            (
                graph(json!([{"id": "x", "command": run(json!({"workspace_id": "x"}))}])),
                "/tasks/0/command/workspace_id",
            ),
            (
                graph(
                    json!([{"id": "x", "command": {"kind": "agent", "provider": "claude", "workspace_id": 1, "instruction": "i", "session": {"kind": "new", "bogus": 1}}}]),
                ),
                "/tasks/0/command/session/bogus",
            ),
            (
                graph(json!([run_task(
                    json!({"on_failure": {"kind": "fallback", "task": 5}})
                )])),
                "/tasks/0/on_failure/task",
            ),
            // 필수 object 안의 누락은 그 object 다.
            (
                graph(json!([{"id": "x", "command": {"kind": "run", "command": ["true"]}}])),
                "/tasks/0/command",
            ),
            (
                graph(json!([{"id": "x", "command": {"workspace_id": 1, "command": ["true"]}}])),
                "/tasks/0/command",
            ),
            (graph(json!([{"command": run(json!({}))}])), "/tasks/0"),
        ];
        for (input, expected) in cases {
            assert_eq!(location(input.clone()), expected, "{input}");
        }
    }

    #[test]
    fn a_valid_graph_reads_as_before() {
        let graph = json!({"contract_version": 2, "tasks": [run_task(json!({}))]});
        assert_eq!(TaskGraphSpec::from_json(&graph).unwrap().tasks.len(), 1);
    }
}
