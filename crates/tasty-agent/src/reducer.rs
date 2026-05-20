//! Reducer — 여러 task 의 결과를 단일 값으로 합성.
//!
//! 본 모듈은 합성 로직만 책임진다. task lookup 과 권한 검사는 호출 측의 일이며,
//! `Custom { command }` 의 실제 shell 실행도 호출자(호스트)가 closure 로 주입한다
//! (테스트가 쉽고, tasty-agent 가 shell-free 유지).
//!
//! 4종 in-process 전략:
//! - `first_success`: 첫 `Succeeded` task 의 `output` (없으면 `error: "no_success"`)
//! - `all`: 모든 결과의 `output` 을 순서대로 JSON 배열로 — `Succeeded`/`Failed` 무관
//! - `merge_json`: 모든 결과의 `output` (JSON object) 을 left-to-right 로 deep merge
//! - `concat_text`: 모든 결과의 `output` 을 text 로 이어 붙임 (string 은 그대로,
//!   다른 타입은 `serde_json::to_string` 으로 직렬화)
//!
//! 1종 host-bridged 전략:
//! - `custom { command }`: 호출 측이 제공한 closure 로 명령 실행. closure 는
//!   stdin 에 `[result1, result2, ...]` JSON 배열을 받고 stdout 을 결과로 반환.

use serde_json::{Map, Value};

use crate::{AgentError, Result, task::ReducerStrategy};

/// 단일 task 결과 — reducer 입력. `output` 만 보고 합성하므로 `exit_code`/`error`
/// 는 호출자가 별도로 처리 (이 모듈에서는 `output` 만 사용).
#[derive(Debug, Clone, PartialEq)]
pub struct ReducerInput {
    /// 본 task 가 성공했는지 (`first_success` 분기용).
    pub succeeded: bool,
    /// task 의 `result.output` (없으면 `Value::Null`).
    pub output: Value,
}

/// In-process 4종 전략 합성.
pub fn reduce_in_process(strategy: &ReducerStrategy, inputs: &[ReducerInput]) -> Result<Value> {
    match strategy {
        ReducerStrategy::FirstSuccess => first_success(inputs),
        ReducerStrategy::All => Ok(Value::Array(
            inputs.iter().map(|i| i.output.clone()).collect(),
        )),
        ReducerStrategy::MergeJson => merge_json(inputs),
        ReducerStrategy::ConcatText => concat_text(inputs),
        ReducerStrategy::Custom { .. } => Err(AgentError::InvalidArgument(
            "Custom reducer requires host-side shell bridge; use `reduce_with_custom`".into(),
        )),
    }
}

fn first_success(inputs: &[ReducerInput]) -> Result<Value> {
    inputs
        .iter()
        .find(|i| i.succeeded)
        .map(|i| i.output.clone())
        .ok_or_else(|| AgentError::InvalidArgument("no successful input for first_success".into()))
}

fn merge_json(inputs: &[ReducerInput]) -> Result<Value> {
    let mut acc = Map::<String, Value>::new();
    for (i, input) in inputs.iter().enumerate() {
        match &input.output {
            Value::Object(map) => deep_merge(&mut acc, map),
            Value::Null => {}
            _ => {
                return Err(AgentError::InvalidArgument(format!(
                    "merge_json: input #{i} is not a JSON object (got {})",
                    type_name(&input.output)
                )));
            }
        }
    }
    Ok(Value::Object(acc))
}

fn deep_merge(dst: &mut Map<String, Value>, src: &Map<String, Value>) {
    for (k, v) in src {
        match (dst.get_mut(k), v) {
            (Some(Value::Object(d)), Value::Object(s)) => deep_merge(d, s),
            _ => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
}

fn concat_text(inputs: &[ReducerInput]) -> Result<Value> {
    let mut out = String::new();
    for input in inputs {
        match &input.output {
            Value::String(s) => out.push_str(s),
            Value::Null => {}
            other => out.push_str(&serde_json::to_string(other)?),
        }
    }
    Ok(Value::String(out))
}

fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "bool",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

/// `Custom { command }` 용 host-bridged 실행. `runner` 가 (command, stdin_json) 을
/// 받아 stdout 문자열을 반환한다. stdout 은 JSON 으로 파싱; 실패 시 그대로 문자열
/// value 로 반환.
pub fn reduce_with_custom<F>(
    strategy: &ReducerStrategy,
    inputs: &[ReducerInput],
    runner: F,
) -> Result<Value>
where
    F: FnOnce(&str, &str) -> std::io::Result<String>,
{
    match strategy {
        ReducerStrategy::Custom { command } => {
            let stdin_value = Value::Array(inputs.iter().map(|i| i.output.clone()).collect());
            let stdin_json = serde_json::to_string(&stdin_value)?;
            let stdout = runner(command, &stdin_json).map_err(|e| {
                AgentError::InvalidArgument(format!("custom reducer command failed: {e}"))
            })?;
            // stdout 을 JSON 으로 시도 → 실패하면 string value.
            match serde_json::from_str::<Value>(stdout.trim()) {
                Ok(v) => Ok(v),
                Err(_) => Ok(Value::String(stdout)),
            }
        }
        _ => reduce_in_process(strategy, inputs),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(succeeded: bool, output: Value) -> ReducerInput {
        ReducerInput { succeeded, output }
    }

    #[test]
    fn first_success_returns_first_succeeded_output() {
        let inputs = vec![
            input(false, json!({"err": "fail"})),
            input(true, json!({"hello": "world"})),
            input(true, json!({"another": "ok"})),
        ];
        let out = reduce_in_process(&ReducerStrategy::FirstSuccess, &inputs).unwrap();
        assert_eq!(out, json!({"hello": "world"}));
    }

    #[test]
    fn first_success_errors_when_none_succeeded() {
        let inputs = vec![input(false, json!(null)), input(false, json!(null))];
        let err = reduce_in_process(&ReducerStrategy::FirstSuccess, &inputs).unwrap_err();
        assert!(matches!(err, AgentError::InvalidArgument(_)));
    }

    #[test]
    fn all_collects_outputs_in_order_regardless_of_status() {
        let inputs = vec![
            input(true, json!(1)),
            input(false, json!("two")),
            input(true, json!({"three": 3})),
        ];
        let out = reduce_in_process(&ReducerStrategy::All, &inputs).unwrap();
        assert_eq!(out, json!([1, "two", {"three": 3}]));
    }

    #[test]
    fn merge_json_deep_merges_objects() {
        let inputs = vec![
            input(true, json!({"a": 1, "nested": {"x": 1}})),
            input(true, json!({"b": 2, "nested": {"y": 2}})),
            input(true, json!({"a": 99})),
        ];
        let out = reduce_in_process(&ReducerStrategy::MergeJson, &inputs).unwrap();
        assert_eq!(out, json!({"a": 99, "b": 2, "nested": {"x": 1, "y": 2}}));
    }

    #[test]
    fn merge_json_rejects_non_object_input() {
        let inputs = vec![input(true, json!({"a": 1})), input(true, json!("oops"))];
        let err = reduce_in_process(&ReducerStrategy::MergeJson, &inputs).unwrap_err();
        assert!(matches!(err, AgentError::InvalidArgument(_)));
    }

    #[test]
    fn merge_json_skips_null_input() {
        let inputs = vec![input(true, json!({"a": 1})), input(true, json!(null))];
        let out = reduce_in_process(&ReducerStrategy::MergeJson, &inputs).unwrap();
        assert_eq!(out, json!({"a": 1}));
    }

    #[test]
    fn concat_text_joins_strings_directly() {
        let inputs = vec![input(true, json!("hello ")), input(true, json!("world"))];
        let out = reduce_in_process(&ReducerStrategy::ConcatText, &inputs).unwrap();
        assert_eq!(out, json!("hello world"));
    }

    #[test]
    fn concat_text_serializes_non_string_values() {
        let inputs = vec![
            input(true, json!("count=")),
            input(true, json!(42)),
            input(true, json!({"x": 1})),
        ];
        let out = reduce_in_process(&ReducerStrategy::ConcatText, &inputs).unwrap();
        assert_eq!(out, json!("count=42{\"x\":1}"));
    }

    #[test]
    fn custom_in_in_process_path_errors() {
        let inputs = vec![input(true, json!(null))];
        let strategy = ReducerStrategy::Custom {
            command: "noop".into(),
        };
        let err = reduce_in_process(&strategy, &inputs).unwrap_err();
        assert!(matches!(err, AgentError::InvalidArgument(_)));
    }

    #[test]
    fn custom_via_runner_returns_stdout_value() {
        let inputs = vec![input(true, json!(1)), input(true, json!(2))];
        let strategy = ReducerStrategy::Custom {
            command: "doubled".into(),
        };
        let out = reduce_with_custom(&strategy, &inputs, |_cmd, stdin| {
            // 검증: stdin 이 정확한 배열로 들어왔는지.
            let v: Value = serde_json::from_str(stdin).unwrap();
            assert_eq!(v, json!([1, 2]));
            Ok("[2, 4]".to_string())
        })
        .unwrap();
        assert_eq!(out, json!([2, 4]));
    }

    #[test]
    fn custom_via_runner_falls_back_to_string_on_invalid_json() {
        let inputs = vec![input(true, json!(null))];
        let strategy = ReducerStrategy::Custom {
            command: "echo".into(),
        };
        let out =
            reduce_with_custom(&strategy, &inputs, |_, _| Ok("not json".to_string())).unwrap();
        assert_eq!(out, json!("not json"));
    }

    #[test]
    fn reduce_with_custom_dispatches_non_custom_to_in_process() {
        let inputs = vec![input(true, json!("hi"))];
        let out = reduce_with_custom(&ReducerStrategy::All, &inputs, |_, _| {
            panic!("runner should not be called for non-custom strategies")
        })
        .unwrap();
        assert_eq!(out, json!(["hi"]));
    }
}
