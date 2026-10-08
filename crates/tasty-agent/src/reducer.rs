//! Reducer — 여러 task 의 결과를 단일 값으로 합성.
//!
//! task 조회와 권한 검사는 호출자가 담당한다. custom 전략은 주입한 함수를 실행하며,
//! 기본 셸 실행 함수로 run_custom_shell을 제공한다.
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
//!
//! extract_paths는 선택한 JSON Pointer의 값만 합성하도록 입력을 추출한다.
//! 구조 전체가 필요한 전략도 있으므로 명시적으로 요청한 경우에만 적용한다.

use crate::task::types::TypedValue;
use serde_json::{Map, Value};

use crate::{
    AgentError, Result,
    task::{ReducerStrategy, TaskId},
};

/// 단일 task 결과 — reducer 입력. `output` 만 보고 합성하므로 `exit_code`/`error`
/// 는 호출자가 별도로 처리 (이 모듈에서는 `output` 만 사용).
#[derive(Debug, Clone, PartialEq)]
pub struct ReducerInput {
    /// 본 task 가 성공했는지 (`first_success` 분기용).
    pub succeeded: bool,
    /// 이 결과를 만든 task id — `extract_paths` 가 경로 누락 경고 메시지에 쓴다.
    pub task_id: TaskId,
    /// task 의 `result.output` (없으면 `Value::Null`).
    pub output: Value,
}

/// JSON Pointer가 있으면 해당 값만 추출한다. None이면 원래 입력을 반환한다.
/// 경로가 없는 입력은 Null로 바꾸고 경고를 반환한다. 호출자는 경고를 응답에 포함해야 한다.
pub fn extract_paths(
    inputs: &[ReducerInput],
    extract_path: Option<&str>,
) -> (Vec<ReducerInput>, Vec<String>) {
    let Some(path) = extract_path else {
        return (inputs.to_vec(), Vec::new());
    };
    let mut warnings = Vec::new();
    let extracted = inputs
        .iter()
        .enumerate()
        .map(|(i, input)| match input.output.pointer(path) {
            Some(v) => ReducerInput {
                succeeded: input.succeeded,
                task_id: input.task_id.clone(),
                output: v.clone(),
            },
            None => {
                warnings.push(format!(
                    "input #{i}(task {})에 경로 '{path}'가 없어 null로 처리했습니다",
                    input.task_id
                ));
                ReducerInput {
                    succeeded: input.succeeded,
                    task_id: input.task_id.clone(),
                    output: Value::Null,
                }
            }
        })
        .collect();
    (extracted, warnings)
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
            match serde_json::from_str::<Value>(stdout.trim()) {
                Ok(v) => Ok(v),
                Err(_) => Ok(Value::String(stdout)),
            }
        }
        _ => reduce_in_process(strategy, inputs),
    }
}

/// v2 reduce 입력 하나. 성공 출력이 없는 입력은 `has_output: false` 다.
#[derive(Debug, Clone, PartialEq)]
pub struct TypedReducerInput {
    pub task_id: TaskId,
    /// 입력 task 의 상태 이름([`crate::TaskState::name`]).
    pub state: &'static str,
    pub has_output: bool,
    /// 입력의 선언 타입을 아는 출력. v1 입력은 무타입이라 `json` 이다.
    pub output: TypedValue,
    /// 출력을 낸 입력 회차. 회차가 없는 v1 입력은 None.
    pub attempt_id: Option<String>,
}

impl TypedReducerInput {
    /// 입력 task 에서 만든다. v2 입력은 확정된 출력만, v1 입력은 성공했을 때의
    /// `result.output` 만 출력으로 본다.
    pub fn from_task(task: &crate::Task) -> Self {
        let succeeded = matches!(task.state, crate::TaskState::Succeeded);
        let (has_output, output) = match (&task.typed_result, &task.result) {
            (Some(t), _) => (succeeded && t.has_output, t.output.clone()),
            (None, Some(r)) => match &r.output {
                Some(v) if succeeded => (true, TypedValue::Json(v.clone())),
                _ => (false, TypedValue::Null),
            },
            (None, None) => (false, TypedValue::Null),
        };
        Self {
            task_id: task.id.clone(),
            state: task.state.name(),
            has_output,
            output: if has_output { output } else { TypedValue::Null },
            attempt_id: task.attempt.as_ref().map(|a| a.id.clone()),
        }
    }

    fn record(&self) -> Value {
        let mut m = Map::new();
        m.insert("task_id".into(), Value::String(self.task_id.clone()));
        m.insert("state".into(), Value::String(self.state.into()));
        m.insert("has_output".into(), Value::Bool(self.has_output));
        if let Some(a) = &self.attempt_id {
            m.insert("attempt_id".into(), Value::String(a.clone()));
        }
        // 레코드의 output 은 입력마다 타입이 달라 json 으로 선언되므로, 각 입력의 선언
        // 타입대로 직렬화한 값을 넣는다(int64 는 10진 문자열). custom reducer stdin 도 같다.
        if self.has_output {
            m.insert("output".into(), self.output.to_wire());
        }
        Value::Object(m)
    }

    /// 내부 계산용 JSON(int64 는 정수).
    fn require_output(&self) -> std::result::Result<Value, String> {
        if self.has_output {
            Ok(self.output.to_internal())
        } else {
            Err(format!(
                "input {} has no output (state {})",
                self.task_id, self.state
            ))
        }
    }
}

/// v2 reduce. 출력 타입 검사는 결과 확정 단계가 맡는다.
///
/// - `first_success`: 선언된 입력 순서에서 처음 성공한 입력의 출력. 성공이 없으면 오류.
/// - `all`: 입력 순서대로 `{task_id, state, has_output, output?}` 레코드 목록.
/// - `merge_json`: 모든 입력이 object 출력을 가져야 한다. 중첩 object 는 재귀 병합하고,
///   같은 경로에 서로 다른 값이 오면 `conflict` 정책을 따른다.
/// - `concat_text`: 모든 입력이 string 출력을 가져야 한다. 다른 값은 변환하지 않는다.
/// - `custom`: stdin 으로 `all` 과 같은 레코드 목록을 받고 stdout 의 JSON 한 값을
///   결과로 쓴다. JSON 이 아니면 오류다.
pub fn reduce_typed<F>(
    strategy: &ReducerStrategy,
    inputs: &[TypedReducerInput],
    conflict: crate::task::contract::MergeConflict,
    runner: F,
) -> std::result::Result<Value, String>
where
    F: FnOnce(&str, &str) -> std::io::Result<String>,
{
    match strategy {
        ReducerStrategy::FirstSuccess => inputs
            .iter()
            .find(|i| i.has_output)
            .map(|i| i.output.to_internal())
            .ok_or_else(|| "first_success: no input succeeded with an output".to_string()),
        ReducerStrategy::All => Ok(Value::Array(inputs.iter().map(|i| i.record()).collect())),
        ReducerStrategy::MergeJson => {
            let mut acc = Map::new();
            for input in inputs {
                let output = input.require_output()?;
                let Value::Object(map) = &output else {
                    return Err(format!(
                        "merge_json: input {} output is {}, not an object",
                        input.task_id,
                        type_name(&output)
                    ));
                };
                merge_typed(&mut acc, map, conflict, "")?;
            }
            Ok(Value::Object(acc))
        }
        ReducerStrategy::ConcatText => {
            let mut out = String::new();
            for input in inputs {
                match input.require_output()? {
                    Value::String(s) => out.push_str(&s),
                    other => {
                        return Err(format!(
                            "concat_text: input {} output is {}, not a string; convert it explicitly",
                            input.task_id,
                            type_name(&other)
                        ));
                    }
                }
            }
            Ok(Value::String(out))
        }
        ReducerStrategy::Custom { command } => {
            let stdin = Value::Array(inputs.iter().map(|i| i.record()).collect());
            let stdin_json = serde_json::to_string(&stdin).map_err(|e| e.to_string())?;
            let stdout = runner(command, &stdin_json)
                .map_err(|e| format!("custom reducer command failed: {e}"))?;
            serde_json::from_str::<Value>(stdout.trim())
                .map_err(|e| format!("custom reducer stdout is not one JSON value: {e}"))
        }
    }
}

fn merge_typed(
    dst: &mut Map<String, Value>,
    src: &Map<String, Value>,
    conflict: crate::task::contract::MergeConflict,
    path: &str,
) -> std::result::Result<(), String> {
    use crate::task::contract::MergeConflict;
    for (k, v) in src {
        let here = format!("{path}/{k}");
        match (dst.get_mut(k), v) {
            (Some(Value::Object(d)), Value::Object(s)) => merge_typed(d, s, conflict, &here)?,
            (Some(existing), _) if !json_equal(existing, v) => match conflict {
                MergeConflict::Overwrite => *existing = v.clone(),
                MergeConflict::Error => {
                    return Err(format!(
                        "merge_json: conflicting values at {here}: {existing} vs {v}"
                    ));
                }
            },
            (Some(_), _) => {}
            (None, _) => {
                dst.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(())
}

/// merge 충돌 판정용 같음. 숫자끼리는 표기가 아니라 수치로 비교한다(`1` == `1.0`).
fn json_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => number_equal(x, y),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_equal(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, p)| y.get(k).is_some_and(|q| json_equal(p, q)))
        }
        _ => a == b,
    }
}

/// 둘 다 정수 값이면 i128 로 정확히 비교한다. 2^53 을 넘는 정수를 f64 로 바꿔 같다고
/// 보지 않기 위해서다. 정수가 아닌 실수가 끼면 f64 로 비교한다.
fn number_equal(a: &serde_json::Number, b: &serde_json::Number) -> bool {
    fn exact(n: &serde_json::Number) -> Option<i128> {
        if let Some(i) = n.as_i64() {
            return Some(i128::from(i));
        }
        if let Some(u) = n.as_u64() {
            return Some(i128::from(u));
        }
        let f = n.as_f64()?;
        // 정수 모양이고 i128 범위 안이면 정확히 변환된다.
        (f.fract() == 0.0 && f.abs() < 1.7e38).then_some(f as i128)
    }
    match (exact(a), exact(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.as_f64() == b.as_f64(),
    }
}

/// custom 전략의 기본 셸 실행기. 입력 JSON을 stdin으로 보내고 stdout을 반환한다.
/// 셸의 환경은 작업 자식 규칙([`crate::child_env`])으로 거른다.
pub fn run_custom_shell(command: &str, stdin_json: &str) -> std::io::Result<String> {
    run_custom_shell_with_env(command, stdin_json, &[])
}

/// [`run_custom_shell`] 에 환경 변수를 더한다. 러너가 report 주소를 넘기는 데 쓴다.
pub fn run_custom_shell_with_env(
    command: &str,
    stdin_json: &str,
    extra_env: &[(std::ffi::OsString, std::ffi::OsString)],
) -> std::io::Result<String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    #[cfg(windows)]
    let mut cmd = {
        let mut c = Command::new("cmd");
        c.args(["/C", command]);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("sh");
        c.args(["-c", command]);
        c
    };

    tasty_utils::process::hide_console(&mut cmd);
    let mut child = cmd
        .env_clear()
        .envs(crate::child_env::inherited())
        .envs(extra_env.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut sin) = child.stdin.take() {
        sin.write_all(stdin_json.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
        return Err(std::io::Error::other(format!(
            "exit_code={}, stderr={}",
            out.status.code().unwrap_or(-1),
            stderr.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input(succeeded: bool, output: Value) -> ReducerInput {
        input_with_id(succeeded, "t", output)
    }

    fn input_with_id(succeeded: bool, task_id: &str, output: Value) -> ReducerInput {
        ReducerInput {
            succeeded,
            task_id: task_id.to_string(),
            output,
        }
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

    #[test]
    fn extract_paths_none_passes_through_unchanged() {
        let inputs = vec![input(true, json!({"stdout": {"text": "out1\n"}}))];
        let (extracted, warnings) = extract_paths(&inputs, None);
        assert_eq!(extracted, inputs);
        assert!(warnings.is_empty());
    }

    #[test]
    fn extract_paths_pulls_leaf_value_from_run_output() {
        let inputs = vec![
            input(true, json!({"pid": 1, "stdout": {"text": "out1\n"}})),
            input(true, json!({"pid": 2, "stdout": {"text": "out2\n"}})),
        ];
        let (extracted, warnings) = extract_paths(&inputs, Some("/stdout/text"));
        assert!(warnings.is_empty());
        let out = reduce_in_process(&ReducerStrategy::ConcatText, &extracted).unwrap();
        assert_eq!(out, json!("out1\nout2\n"));
    }

    #[test]
    fn extract_paths_missing_path_becomes_null_with_warning() {
        let inputs = vec![
            input_with_id(true, "t-run", json!({"stdout": {"text": "out1\n"}})),
            input_with_id(true, "t-custom", json!({"result": "no stdout here"})),
        ];
        let (extracted, warnings) = extract_paths(&inputs, Some("/stdout/text"));
        assert_eq!(extracted[0].output, json!("out1\n"));
        assert_eq!(extracted[1].output, Value::Null);
        assert_eq!(
            warnings,
            vec!["input #1(task t-custom)에 경로 '/stdout/text'가 없어 null로 처리했습니다"]
        );
        let out = reduce_in_process(&ReducerStrategy::ConcatText, &extracted).unwrap();
        assert_eq!(out, json!("out1\n"));
    }

    fn typed(id: &str, state: &'static str, output: Option<Value>) -> TypedReducerInput {
        TypedReducerInput {
            task_id: id.to_string(),
            state,
            has_output: output.is_some(),
            output: output.map(TypedValue::Json).unwrap_or_default(),
            attempt_id: None,
        }
    }

    use crate::task::contract::MergeConflict;

    fn no_shell(_: &str, _: &str) -> std::io::Result<String> {
        panic!("runner should not be called")
    }

    #[test]
    fn typed_all_returns_records_without_inventing_failed_outputs() {
        let inputs = vec![
            typed("a", "succeeded", Some(json!(1))),
            typed("b", "failed", None),
            typed("c", "succeeded", Some(json!(null))),
        ];
        let out = reduce_typed(
            &ReducerStrategy::All,
            &inputs,
            MergeConflict::Error,
            no_shell,
        )
        .unwrap();
        assert_eq!(
            out,
            json!([
                {"task_id": "a", "state": "succeeded", "has_output": true, "output": 1},
                {"task_id": "b", "state": "failed", "has_output": false},
                {"task_id": "c", "state": "succeeded", "has_output": true, "output": null},
            ])
        );
        let schema = crate::task::contract::reduce_all_record_list_schema();
        assert!(
            crate::task::types::TypeDefs::default()
                .validate(&schema, &out)
                .is_ok()
        );
    }

    #[test]
    fn records_and_custom_stdin_serialize_each_input_by_its_declared_type() {
        let big = 9_007_199_254_740_993_i64;
        let int_input = TypedReducerInput {
            task_id: "a".into(),
            state: "succeeded",
            has_output: true,
            output: TypedValue::Int64(big),
            attempt_id: None,
        };
        // json 으로 선언된 값 안의 정수는 바꾸지 않는다.
        let json_input = typed("b", "succeeded", Some(json!({"n": big})));
        let inputs = vec![int_input, json_input];
        let all = reduce_typed(
            &ReducerStrategy::All,
            &inputs,
            MergeConflict::Error,
            no_shell,
        )
        .unwrap();
        assert_eq!(all[0]["output"], json!("9007199254740993"));
        assert_eq!(all[1]["output"], json!({"n": big}));

        let strategy = ReducerStrategy::Custom {
            command: "x".into(),
        };
        reduce_typed(&strategy, &inputs, MergeConflict::Error, |_, stdin| {
            let v: Value = serde_json::from_str(stdin).unwrap();
            assert_eq!(v[0]["output"], json!("9007199254740993"));
            assert_eq!(v[1]["output"], json!({"n": big}));
            Ok("null".into())
        })
        .unwrap();

        // 내부 계산(first_success)은 정수 그대로다.
        let first = reduce_typed(
            &ReducerStrategy::FirstSuccess,
            &inputs,
            MergeConflict::Error,
            no_shell,
        )
        .unwrap();
        assert_eq!(first, json!(big));
    }

    #[test]
    fn typed_first_success_follows_declared_order_and_errors_without_success() {
        let inputs = vec![
            typed("a", "failed", None),
            typed("b", "succeeded", Some(json!("second"))),
            typed("c", "succeeded", Some(json!("third"))),
        ];
        let out = reduce_typed(
            &ReducerStrategy::FirstSuccess,
            &inputs,
            MergeConflict::Error,
            no_shell,
        )
        .unwrap();
        assert_eq!(out, json!("second"));
        let none = vec![typed("a", "failed", None), typed("b", "skipped", None)];
        assert!(
            reduce_typed(
                &ReducerStrategy::FirstSuccess,
                &none,
                MergeConflict::Error,
                no_shell
            )
            .is_err()
        );
    }

    #[test]
    fn typed_merge_json_reports_conflicts_unless_overwrite_is_chosen() {
        let inputs = vec![
            typed("a", "succeeded", Some(json!({"x": 1, "n": {"p": 1}}))),
            typed("b", "succeeded", Some(json!({"x": 2, "n": {"q": 2}}))),
        ];
        let e = reduce_typed(
            &ReducerStrategy::MergeJson,
            &inputs,
            MergeConflict::Error,
            no_shell,
        )
        .unwrap_err();
        assert!(e.contains("/x"), "{e}");
        let out = reduce_typed(
            &ReducerStrategy::MergeJson,
            &inputs,
            MergeConflict::Overwrite,
            no_shell,
        )
        .unwrap();
        assert_eq!(out, json!({"x": 2, "n": {"p": 1, "q": 2}}));
        // 같은 값은 충돌이 아니다.
        let same = vec![
            typed("a", "succeeded", Some(json!({"x": 1}))),
            typed("b", "succeeded", Some(json!({"x": 1}))),
        ];
        assert!(
            reduce_typed(
                &ReducerStrategy::MergeJson,
                &same,
                MergeConflict::Error,
                no_shell
            )
            .is_ok()
        );
        let failed = vec![typed("a", "failed", None)];
        assert!(
            reduce_typed(
                &ReducerStrategy::MergeJson,
                &failed,
                MergeConflict::Error,
                no_shell
            )
            .is_err()
        );
    }

    #[test]
    fn typed_merge_compares_numbers_by_value() {
        let inputs = vec![
            typed("a", "succeeded", Some(json!({"n": 1, "m": {"x": [2]}}))),
            typed("b", "succeeded", Some(json!({"n": 1.0, "m": {"x": [2.0]}}))),
        ];
        let out = reduce_typed(
            &ReducerStrategy::MergeJson,
            &inputs,
            MergeConflict::Error,
            no_shell,
        )
        .unwrap();
        // 같은 값이면 앞 입력의 표기를 유지한다.
        assert_eq!(out, json!({"n": 1, "m": {"x": [2]}}));
        // 2^53 + 1 과 그 f64 근사(2^53)는 다른 값이다.
        let near = vec![
            typed(
                "a",
                "succeeded",
                Some(json!({"n": 9_007_199_254_740_993_i64})),
            ),
            typed(
                "b",
                "succeeded",
                Some(json!({"n": 9_007_199_254_740_992.0_f64})),
            ),
        ];
        assert!(
            reduce_typed(
                &ReducerStrategy::MergeJson,
                &near,
                MergeConflict::Error,
                no_shell
            )
            .is_err()
        );
        let different = vec![
            typed("a", "succeeded", Some(json!({"n": 1}))),
            typed("b", "succeeded", Some(json!({"n": 1.5}))),
        ];
        assert!(
            reduce_typed(
                &ReducerStrategy::MergeJson,
                &different,
                MergeConflict::Error,
                no_shell
            )
            .is_err()
        );
    }

    #[test]
    fn typed_concat_text_refuses_non_strings_and_missing_outputs() {
        let ok = vec![
            typed("a", "succeeded", Some(json!("a"))),
            typed("b", "succeeded", Some(json!("b"))),
        ];
        assert_eq!(
            reduce_typed(
                &ReducerStrategy::ConcatText,
                &ok,
                MergeConflict::Error,
                no_shell
            )
            .unwrap(),
            json!("ab")
        );
        let num = vec![typed("a", "succeeded", Some(json!(42)))];
        assert!(
            reduce_typed(
                &ReducerStrategy::ConcatText,
                &num,
                MergeConflict::Error,
                no_shell
            )
            .is_err()
        );
        let failed = vec![typed("a", "failed", None)];
        assert!(
            reduce_typed(
                &ReducerStrategy::ConcatText,
                &failed,
                MergeConflict::Error,
                no_shell
            )
            .is_err()
        );
    }

    #[test]
    fn typed_custom_reducer_gets_records_and_rejects_non_json_stdout() {
        let inputs = vec![typed("a", "succeeded", Some(json!(1)))];
        let strategy = ReducerStrategy::Custom {
            command: "x".into(),
        };
        let out = reduce_typed(&strategy, &inputs, MergeConflict::Error, |_, stdin| {
            let v: Value = serde_json::from_str(stdin).unwrap();
            assert_eq!(
                v,
                json!([{"task_id": "a", "state": "succeeded", "has_output": true, "output": 1}])
            );
            Ok(" 7 \n".to_string())
        })
        .unwrap();
        assert_eq!(out, json!(7));
        let e = reduce_typed(&strategy, &inputs, MergeConflict::Error, |_, _| {
            Ok("not json".to_string())
        });
        assert!(e.is_err(), "v2 는 문자열로 조용히 바꾸지 않는다");
    }

    #[test]
    fn extract_paths_preserves_succeeded_and_task_id() {
        let inputs = vec![input_with_id(false, "t-1", json!({"a": {"b": 1}}))];
        let (extracted, _) = extract_paths(&inputs, Some("/a/b"));
        assert!(!extracted[0].succeeded);
        assert_eq!(extracted[0].task_id, "t-1");
        assert_eq!(extracted[0].output, json!(1));
    }
}
