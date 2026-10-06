//! v2 task 계약 — 종류별 기본 입출력, 생성 시 검증, 결과 확정.
//!
//! 계약이 없는 task 는 v1 이다. v1 의 결과 구조·reducer 관례·저장 형식은 바뀌지
//! 않으며 이 모듈의 규칙을 소급하지 않는다.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::binding::{InputBinding, InputMapping};
use super::postprocess::{PostprocessRaw, PostprocessSpec, StdoutFormat, check_spec};
use super::types::{FieldSchema, TypeDefs, TypeError, TypeKind, TypeSchema, TypedValue};
use super::{OnFailure, ReducerStrategy, Task, TaskCommand, TaskId, TaskResult};

/// 현재 지원하는 계약 버전.
pub const TASK_CONTRACT_V2: u32 = 2;

/// 한 task 의 v2 계약.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskContract {
    /// 계약 버전. [`TASK_CONTRACT_V2`] 만 받는다.
    pub contract_version: u32,
    /// 입력·출력 스키마가 `{"ref": ..}` 로 참조하는 이름 있는 타입.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub types: BTreeMap<String, TypeSchema>,
    /// 생략하면 unit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<TypeSchema>,
    /// 최종 공개 출력의 타입. 생략하면 종류별 기본값([`default_output_schema`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<TypeSchema>,
    /// run 전용. 성공으로 받는 종료 코드. 생략하면 `[0]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_exit_codes: Option<Vec<i32>>,
    /// reduce `merge_json` 전용. 같은 키에 다른 값이 오면 어떻게 할지. 생략하면 오류.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_conflict: Option<MergeConflict>,
    /// 입력 object 필드별 값의 출처. source task 는 데이터 의존성이 된다.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub bindings: BTreeMap<String, InputBinding>,
    /// 검증된 입력을 실행 인자로 넘기는 자리.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_mapping: Option<InputMapping>,
    /// 본 작업 뒤 실행할 CLI. 있으면 최종 출력은 그 stdout 에서 수집한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postprocess: Option<PostprocessSpec>,
    /// 성공한 출력으로 후속 경로를 고르는 선언. 그래프 제출로만 정한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transitions: Option<super::route::Transitions>,
}

/// `merge_json` 의 동일 키 충돌 정책.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeConflict {
    /// 같은 경로에 서로 다른 값이 오면 오류.
    Error,
    /// 뒤 입력의 값으로 덮는다.
    Overwrite,
}

impl TaskContract {
    /// 이름 있는 타입 모음.
    pub fn defs(&self) -> TypeDefs {
        TypeDefs::new(self.types.clone())
    }

    /// 입력 스키마. 생략하면 unit.
    pub fn input_schema(&self) -> TypeSchema {
        self.input_schema.clone().unwrap_or_else(TypeSchema::unit)
    }

    /// 최종 출력 스키마. 선언이 없으면 종류별 기본값이고, 후처리가 있으면 stdout 형식에 따라
    /// json(json 형식) 또는 string(text 형식)이다.
    pub fn output_schema(&self, command: &TaskCommand) -> TypeSchema {
        if let Some(s) = &self.output_schema {
            return s.clone();
        }
        match &self.postprocess {
            Some(p) if p.stdout.format == StdoutFormat::Text => TypeSchema::string(),
            Some(_) => TypeSchema::json(),
            None => default_output_schema(command),
        }
    }

    /// run 이 성공으로 받는 종료 코드.
    pub fn allowed_exit_codes(&self) -> Vec<i32> {
        self.allowed_exit_codes.clone().unwrap_or_else(|| vec![0])
    }

    pub fn merge_conflict(&self) -> MergeConflict {
        self.merge_conflict.unwrap_or(MergeConflict::Error)
    }
}

/// 종류 이름. 결과의 provenance 와 오류에 쓴다.
pub fn command_kind(command: &TaskCommand) -> &'static str {
    match command {
        TaskCommand::Run { .. } => "run",
        TaskCommand::Custom { .. } => "custom",
        TaskCommand::Reduce { .. } => "reduce",
        TaskCommand::WaitBarrier { .. } => "wait_barrier",
        TaskCommand::Agent { .. } => "agent",
    }
}

/// v2 종류별 기본 최종 출력.
///
/// - run: 종료 코드 int64. stdout·stderr 는 raw 로만 남는다.
/// - custom: 최종 IPC 결과 json.
/// - wait_barrier: unit. 완료 사실을 boolean 으로 복제하지 않는다.
/// - agent: 그 회차의 최종 답변 string.
/// - reduce: `concat_text` string, `all` 결과 레코드 list, 나머지는 json.
pub fn default_output_schema(command: &TaskCommand) -> TypeSchema {
    match command {
        TaskCommand::Run { .. } => TypeSchema::int64(),
        TaskCommand::Custom { .. } => TypeSchema::json(),
        TaskCommand::WaitBarrier { .. } => TypeSchema::unit(),
        TaskCommand::Agent { .. } => TypeSchema::string(),
        TaskCommand::Reduce { strategy, .. } => match strategy {
            ReducerStrategy::ConcatText => TypeSchema::string(),
            ReducerStrategy::All => reduce_all_record_list_schema(),
            ReducerStrategy::FirstSuccess
            | ReducerStrategy::MergeJson
            | ReducerStrategy::Custom { .. } => TypeSchema::json(),
        },
    }
}

/// reduce `all` 레코드의 `state` 값. task 상태 이름과 같다.
pub const REDUCE_ALL_RECORD_STATES: &[&str] = &[
    "waiting",
    "ready",
    "running",
    "succeeded",
    "failed",
    "cancelled",
    "skipped",
    "unknown",
];

/// reduce `all` 의 v2 출력. 입력 순서대로 `{task_id, state, has_output, output?}`
/// 레코드를 담는다. 성공 출력이 없는 입력은 `has_output: false` 이고 `output` 이 없다.
pub fn reduce_all_record_list_schema() -> TypeSchema {
    let mut fields = BTreeMap::new();
    fields.insert(
        "task_id".to_string(),
        FieldSchema {
            schema: TypeSchema::string(),
            optional: false,
            default: None,
        },
    );
    fields.insert(
        "state".to_string(),
        FieldSchema {
            schema: TypeSchema::new(TypeKind::Enum {
                values: REDUCE_ALL_RECORD_STATES
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            }),
            optional: false,
            default: None,
        },
    );
    fields.insert(
        "has_output".to_string(),
        FieldSchema {
            schema: TypeSchema::new(TypeKind::Boolean),
            optional: false,
            default: None,
        },
    );
    fields.insert(
        "output".to_string(),
        FieldSchema {
            schema: TypeSchema::json(),
            optional: true,
            default: None,
        },
    );
    fields.insert(
        "attempt_id".to_string(),
        FieldSchema {
            schema: TypeSchema::string(),
            optional: true,
            default: None,
        },
    );
    TypeSchema::new(TypeKind::List {
        items: Box::new(TypeSchema::new(TypeKind::Object { fields })),
        max_len: None,
    })
}

// ── 결과 ─────────────────────────────────────────────────────────────────────

/// v2 task 의 결과. `has_output` 이 false 면 `output` 은 의미가 없다. unit 출력은
/// `has_output: true, output: null` 이라 "아직 출력이 없음" 과 구별된다.
///
/// 역직렬화에는 출력 스키마가 필요하다([`TypedResult::from_wire`]). `Task` 의 역직렬화가
/// 계약에서 스키마를 얻어 부른다.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TypedResult {
    pub has_output: bool,
    /// 최종 출력. `null` 도 그대로 직렬화해 부재와 섞이지 않게 한다.
    pub output: TypedValue,
    /// 본 작업의 원본 결과. 최종 출력과 따로 보존한다.
    #[serde(default, skip_serializing_if = "RawResult::is_empty")]
    pub raw: RawResult,
    /// 산출물 참조. 생산자는 아직 없다.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<ArtifactRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<TaskFailure>,
    pub provenance: Provenance,
}

/// 본 작업의 원본 결과.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RawResult {
    /// 프로세스 종료 코드. 숫자 코드가 없으면(신호 종료 등) 비어 있다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// 실행 응답 원문(Run 의 stdout·stderr tail, Custom 의 IPC 응답 등).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<Value>,
    /// 후처리 CLI 의 원본 결과. 후처리가 없거나 실행 전에 끝났으면 비어 있다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub postprocess: Option<PostprocessRaw>,
}

impl RawResult {
    pub fn is_empty(&self) -> bool {
        self.exit_code.is_none() && self.execution.is_none() && self.postprocess.is_none()
    }
}

/// 산출물 참조. 경로만으로 영속·무결성을 보증하지 않으므로 식별 정보를 함께 둔다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub name: String,
    pub uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

/// 실패 단계.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    Input,
    Execution,
    Postprocess,
    OutputValidation,
    Persistence,
    /// 성공한 출력으로 경로를 고르지 못했다(조건 평가 오류, exclusive 다중 참).
    Route,
}

/// 같은 단계 안에서 실패를 구별하는 이유. 지금은 agent task 만 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCode {
    /// 턴은 끝났지만 출력 타입이 요구하는 결과가 없다(string 이 아닌 출력인데 제출이 없음 등).
    ResultMissing,
    /// 턴이 끝나기 전에 agent 세션이 종료됐다.
    AgentExited,
    /// provider 가 턴을 오류로 끝냈다(API 오류 등).
    AgentTurnError,
    /// 턴이 기한 안에 끝나지 않았다.
    TimedOut,
    /// provider 가 결과 수집을 지원하지 않거나 세션에 지시를 보내지 못했다.
    AgentUnavailable,
}

impl FailureCode {
    pub const ALL: [FailureCode; 5] = [
        FailureCode::ResultMissing,
        FailureCode::AgentExited,
        FailureCode::AgentTurnError,
        FailureCode::TimedOut,
        FailureCode::AgentUnavailable,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            FailureCode::ResultMissing => "result_missing",
            FailureCode::AgentExited => "agent_exited",
            FailureCode::AgentTurnError => "agent_turn_error",
            FailureCode::TimedOut => "timed_out",
            FailureCode::AgentUnavailable => "agent_unavailable",
        }
    }

    /// 실행 실패 문자열(`<code>: <설명>`)로 싣는다. runner 의 실패 보고는 문자열 하나만 나른다.
    pub fn message(self, detail: impl std::fmt::Display) -> String {
        format!("{}: {detail}", self.as_str())
    }

    /// [`FailureCode::message`] 로 만든 문자열에서 이유를 읽는다.
    pub fn parse_message(message: &str) -> Option<FailureCode> {
        let (head, _) = message.split_once(": ")?;
        FailureCode::ALL.into_iter().find(|c| c.as_str() == head)
    }
}

/// 실패 사유. 타입 오류면 task·경로·기대·실제 타입을 함께 싣는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskFailure {
    pub stage: FailureStage,
    /// 같은 단계 안의 구별. 없으면 단계와 메시지로만 설명한다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<FailureCode>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<TaskId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub type_error: Option<TypeError>,
    /// 제출한 정의 안의 오류 위치(JSON Pointer, 예: `/tasks/1/bindings/count`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<String>,
}

impl TaskFailure {
    pub fn new(stage: FailureStage, message: impl Into<String>) -> Self {
        Self {
            stage,
            code: None,
            message: message.into(),
            task_id: None,
            type_error: None,
            location: None,
        }
    }

    pub fn typed(stage: FailureStage, task_id: Option<TaskId>, e: TypeError) -> Self {
        let message = match &task_id {
            Some(id) => format!("task {id}: {e}"),
            None => e.to_string(),
        };
        Self {
            stage,
            code: None,
            message,
            task_id,
            type_error: Some(e),
            location: None,
        }
    }
}

impl std::fmt::Display for TaskFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// 결과의 출처.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    pub contract_version: u32,
    pub kind: String,
    /// 최종 출력을 만든 자리(예: `run.exit_code`, `custom.response`).
    pub output_source: String,
}

// ── 생성 시 검증 ─────────────────────────────────────────────────────────────

fn contract_error(message: impl Into<String>) -> TaskFailure {
    TaskFailure::new(FailureStage::Input, message)
}

/// 생성 시 계약을 검사한다. `lookup` 은 reduce 입력 task 를 찾는다.
pub fn check_contract<'a>(
    contract: &TaskContract,
    command: &TaskCommand,
    on_failure: &OnFailure,
    lookup: impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    if contract.contract_version != TASK_CONTRACT_V2 {
        return Err(contract_error(format!(
            "unsupported contract_version {} (supported: {TASK_CONTRACT_V2})",
            contract.contract_version
        )));
    }
    let defs = contract.defs();
    let typed = |e: TypeError| TaskFailure::typed(FailureStage::Input, None, e);
    defs.check().map_err(typed)?;
    let input = contract.input_schema();
    let output = contract.output_schema(command);
    defs.check_schema_at(&input, "/input_schema")
        .map_err(typed)?;
    defs.check_schema_at(&output, "/output_schema")
        .map_err(typed)?;

    if let OnFailure::Fallback {
        inline: Some(_), ..
    } = on_failure
    {
        return Err(contract_error(
            "inline fallback is not supported for contract_version 2; declare the fallback task up front",
        ));
    }
    if contract.allowed_exit_codes.is_some() && !matches!(command, TaskCommand::Run { .. }) {
        return Err(contract_error("allowed_exit_codes applies to run only"));
    }
    if matches!(&contract.allowed_exit_codes, Some(codes) if codes.is_empty()) {
        return Err(contract_error("allowed_exit_codes must not be empty"));
    }
    let is_merge = matches!(
        command,
        TaskCommand::Reduce {
            strategy: ReducerStrategy::MergeJson,
            ..
        }
    );
    if contract.merge_conflict.is_some() && !is_merge {
        return Err(contract_error(
            "merge_conflict applies to reduce merge_json only",
        ));
    }

    if let TaskCommand::Agent {
        provider,
        instruction,
        timeout_ms,
        ..
    } = command
    {
        super::agent::check_command(provider, instruction, *timeout_ms).map_err(contract_error)?;
    }
    if let Some(spec) = &contract.postprocess {
        check_spec(spec)?;
        if !matches!(
            command,
            TaskCommand::Run { .. } | TaskCommand::Custom { .. } | TaskCommand::Agent { .. }
        ) {
            return Err(contract_error(
                "postprocess applies to run, custom and agent",
            ));
        }
        // 후처리가 최종 출력을 만든다. 종류별 출력 제한은 본 작업의 값에만 적용된다.
        return Ok(());
    }

    let declared = contract.output_schema.is_some();
    let out_kind = defs.resolve(&output).map_err(typed)?.kind.clone();
    match command {
        TaskCommand::Run { .. } => {
            if !matches!(out_kind, TypeKind::Int64) || output.nullable {
                return Err(contract_error(
                    "run output is the int64 exit code; other outputs need an explicit extraction step",
                ));
            }
        }
        TaskCommand::WaitBarrier { .. } => {
            if !matches!(out_kind, TypeKind::Unit) {
                return Err(contract_error("wait_barrier output is unit"));
            }
        }
        // string 이 아닌 출력은 같은 회차의 명시 제출로만 채운다.
        TaskCommand::Custom { .. } | TaskCommand::Agent { .. } => {}
        TaskCommand::Reduce { inputs, strategy } => {
            check_reduce(&defs, &output, declared, inputs, strategy, &lookup)?;
        }
    }
    Ok(())
}

/// 생성할 v2 task 하나를 검사한다. 계약, 입력 binding·mapping, 그리고 다른 task 와의
/// 관계(데이터 입력과 실패 정책)를 본다. `lookup` 은 같은 그래프에서 함께 만드는 task 와
/// 기존 task 를 찾는다. `at` 은 오류 위치의 접두사다.
pub fn check_task<'a>(
    task: &Task,
    at: &str,
    lookup: impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    let Some(contract) = &task.contract else {
        return Ok(());
    };
    let locate = |mut f: TaskFailure, loc: &str| {
        if f.location.is_none() {
            f.location = Some(format!("{at}{loc}"));
        }
        if f.task_id.is_none() {
            f.task_id = Some(task.id.clone());
        }
        f
    };
    // 계약 검사의 위치는 task 기준 상대 위치다(없으면 task 자체).
    check_contract(contract, &task.command, &task.on_failure, &lookup).map_err(|mut f| {
        let relative = f.location.take().unwrap_or_default();
        locate(f, &relative)
    })?;
    super::binding::check_inputs(&task.id, contract, &task.command, at, &lookup)?;

    let data_sources: Vec<&TaskId> = contract
        .bindings
        .values()
        .filter_map(|b| match b {
            InputBinding::FromTask { source, .. } => Some(&source.from_task),
            _ => None,
        })
        .collect();
    if !data_sources.is_empty() && matches!(task.on_failure, OnFailure::ContinueDownstream) {
        return Err(locate(
            contract_error(
                "continue_downstream cannot run a task whose input producer failed; the input would have no value",
            ),
            "/on_failure",
        ));
    }
    // 순서 의존성(depends_on)은 main 이 실패해도 fallback 성공으로 진행하지만, main 의
    // 출력을 읽는 binding 에는 값이 없다. 값 출처를 명시하지 않은 이 조합은 거절한다.
    for dep in &task.depends_on {
        let Some(main) = lookup(dep) else { continue };
        if let OnFailure::Fallback { task: Some(fb), .. } = &main.on_failure
            && data_sources.contains(&dep)
        {
            return Err(locate(
                contract_error(format!(
                    "task {}: depends_on {dep} accepts its fallback {fb}, but a binding reads {dep}'s own output, \
                     which does not exist when the fallback ran; bind with one_of [{dep}, {fb}] instead",
                    task.id
                )),
                "/bindings",
            ));
        }
    }
    if let OnFailure::Fallback { task: Some(fb), .. } = &task.on_failure {
        match lookup(fb) {
            Some(t) if t.is_typed() => {}
            Some(_) => {
                return Err(locate(
                    contract_error(format!(
                        "fallback {fb} is a v1 task; a v2 fallback must be a typed task declared up front"
                    )),
                    "/on_failure/task",
                ));
            }
            None => {}
        }
    }
    Ok(())
}

/// reduce `all` 결과에서 `input` 의 레코드 출력을 그 입력의 출력 타입으로 읽는다. 레코드의
/// `output` 은 입력마다 타입이 달라 json(wire 형식)으로 저장되므로, 구체 타입으로 쓰려면 입력
/// task 를 지목해 그 스키마로 다시 읽는다. 입력이 출력을 내지 않았으면 `None` 이다.
///
/// `reducer` 가 확정된 `all` 결과가 아니거나 `input` 의 레코드가 없거나 값이 입력의 출력 타입에
/// 맞지 않으면 오류다.
pub fn reduce_all_record_output(
    reducer: &Task,
    input: &Task,
) -> Result<Option<TypedValue>, TaskFailure> {
    let failure = |m: String| TaskFailure {
        task_id: Some(reducer.id.clone()),
        ..TaskFailure::new(FailureStage::Input, m)
    };
    let is_all = matches!(
        &reducer.command,
        TaskCommand::Reduce {
            strategy: ReducerStrategy::All,
            ..
        }
    );
    let records = match (&reducer.typed_result, is_all) {
        (
            Some(TypedResult {
                has_output: true,
                output: TypedValue::List(records),
                ..
            }),
            true,
        ) => records,
        _ => {
            return Err(failure(format!(
                "task {} has no settled reduce all output",
                reducer.id
            )));
        }
    };
    let record = records
        .iter()
        .find_map(|r| match r {
            TypedValue::Object(m)
                if matches!(m.get("task_id"), Some(TypedValue::String(id)) if *id == input.id) =>
            {
                Some(m)
            }
            _ => None,
        })
        .ok_or_else(|| {
            failure(format!(
                "reduce all output of {} has no record for {}",
                reducer.id, input.id
            ))
        })?;
    if !matches!(record.get("has_output"), Some(TypedValue::Bool(true))) {
        return Ok(None);
    }
    let raw = match record.get("output") {
        Some(TypedValue::Json(v)) => v.clone(),
        _ => serde_json::Value::Null,
    };
    let (defs, schema) = input_output_schema(input);
    defs.typed_value(&schema, &raw)
        .map(Some)
        .map_err(|e| TaskFailure::typed(FailureStage::Input, Some(input.id.clone()), e))
}

fn input_output_schema(task: &Task) -> (TypeDefs, TypeSchema) {
    match &task.contract {
        Some(c) => (c.defs(), c.output_schema(&task.command)),
        // v1 출력은 구조가 정해지지 않은 json 이다.
        None => (TypeDefs::default(), TypeSchema::json()),
    }
}

fn check_reduce<'a>(
    defs: &TypeDefs,
    output: &TypeSchema,
    declared: bool,
    inputs: &[TaskId],
    strategy: &ReducerStrategy,
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    let typed_for =
        |id: &TaskId, e: TypeError| TaskFailure::typed(FailureStage::Input, Some(id.clone()), e);
    let each_input = |f: &dyn Fn(&TaskId, &TypeDefs, &TypeSchema) -> Result<(), TaskFailure>| {
        for id in inputs {
            let Some(t) = lookup(id) else {
                return Err(contract_error(format!("reduce input task not found: {id}")));
            };
            let (idefs, ischema) = input_output_schema(t);
            f(id, &idefs, &ischema)?;
        }
        Ok(())
    };
    match strategy {
        ReducerStrategy::All => {
            if declared && *output != reduce_all_record_list_schema() {
                return Err(contract_error(
                    "reduce all output is the fixed record list; omit output_schema",
                ));
            }
            Ok(())
        }
        ReducerStrategy::ConcatText => {
            let resolved = defs
                .resolve(output)
                .map_err(|e| TaskFailure::typed(FailureStage::Input, None, e))?;
            if !matches!(resolved.kind, TypeKind::String { .. }) || resolved.nullable {
                return Err(contract_error("reduce concat_text output is string"));
            }
            each_input(&|id, idefs, ischema| {
                super::types::check_assignable(
                    idefs,
                    ischema,
                    &TypeDefs::default(),
                    &TypeSchema::string(),
                )
                .map_err(|e| typed_for(id, e))
            })
        }
        ReducerStrategy::FirstSuccess => each_input(&|id, idefs, ischema| {
            super::types::check_assignable(idefs, ischema, defs, output)
                .map_err(|e| typed_for(id, e))
        }),
        ReducerStrategy::MergeJson => {
            let resolved = defs
                .resolve(output)
                .map_err(|e| TaskFailure::typed(FailureStage::Input, None, e))?;
            if !matches!(resolved.kind, TypeKind::Object { .. } | TypeKind::Json) {
                return Err(contract_error("reduce merge_json output is an object"));
            }
            each_input(&|id, idefs, ischema| {
                let r = idefs.resolve(ischema).map_err(|e| typed_for(id, e))?;
                if matches!(r.kind, TypeKind::Object { .. } | TypeKind::Json) {
                    Ok(())
                } else {
                    Err(contract_error(format!(
                        "reduce merge_json input {id} outputs {}, not an object",
                        ischema.describe()
                    )))
                }
            })
        }
        ReducerStrategy::Custom { .. } => Ok(()),
    }
}

// ── 결과 확정 ────────────────────────────────────────────────────────────────

/// 실행이 보고한 v1 형식 결과를 v2 결과로 확정한다. 출력 검증에 실패하면
/// `has_output: false` 와 `output_validation` 실패를 담는다.
pub fn finalize_result(task: &Task, contract: &TaskContract, reported: &TaskResult) -> TypedResult {
    let kind = command_kind(&task.command);
    let provenance = |source: &str| Provenance {
        contract_version: contract.contract_version,
        kind: kind.to_string(),
        output_source: source.to_string(),
    };
    let raw = RawResult {
        exit_code: reported.exit_code,
        execution: reported.output.clone(),
        postprocess: None,
    };
    let failed = |stage: FailureStage, failure: TaskFailure, source: &str| TypedResult {
        has_output: false,
        output: TypedValue::Null,
        raw: raw.clone(),
        artifacts: Vec::new(),
        error: Some(TaskFailure { stage, ..failure }),
        provenance: provenance(source),
    };

    if let Some(error) = &reported.error {
        let code = matches!(task.command, TaskCommand::Agent { .. })
            .then(|| FailureCode::parse_message(error))
            .flatten();
        let stage = match code {
            Some(FailureCode::ResultMissing) => FailureStage::OutputValidation,
            _ => FailureStage::Execution,
        };
        return failed(
            stage,
            TaskFailure {
                code,
                ..TaskFailure::new(stage, error.clone())
            },
            "none",
        );
    }

    let (candidate, source) = match &task.command {
        TaskCommand::Run { .. } => match reported.exit_code {
            Some(code) => (Value::from(code), "run.exit_code"),
            None => {
                return failed(
                    FailureStage::Execution,
                    TaskFailure::new(
                        FailureStage::Execution,
                        "run finished without a numeric exit code",
                    ),
                    "run.exit_code",
                );
            }
        },
        TaskCommand::WaitBarrier { .. } => (Value::Null, "wait_barrier.closed"),
        TaskCommand::Agent { .. } => {
            let needs_submission = super::agent::needs_submission(contract, &task.command);
            match super::agent::candidate(reported.output.as_ref(), needs_submission) {
                Some(found) => found,
                None => {
                    let (why, source) = if needs_submission {
                        (
                            "the turn ended without submitting the required result (tasty agent task-submit)",
                            "agent.submitted",
                        )
                    } else {
                        (
                            "the agent turn ended without a final answer",
                            "agent.final_answer",
                        )
                    };
                    return failed(
                        FailureStage::OutputValidation,
                        TaskFailure {
                            code: Some(FailureCode::ResultMissing),
                            ..TaskFailure::new(
                                FailureStage::OutputValidation,
                                FailureCode::ResultMissing.message(why),
                            )
                        },
                        source,
                    );
                }
            }
        }
        TaskCommand::Custom { .. } | TaskCommand::Reduce { .. } => {
            let source = if matches!(task.command, TaskCommand::Custom { .. }) {
                "custom.response"
            } else {
                "reduce.value"
            };
            match &reported.output {
                Some(v) => (v.clone(), source),
                None => {
                    return failed(
                        FailureStage::OutputValidation,
                        TaskFailure::new(FailureStage::OutputValidation, "no output was produced"),
                        source,
                    );
                }
            }
        }
    };
    let defs = contract.defs();
    let schema = contract.output_schema(&task.command);
    match defs.validate_typed(&schema, &candidate) {
        Ok(output) => TypedResult {
            has_output: true,
            output,
            raw: match &task.command {
                // reduce 의 실행 응답은 출력 그 자체라 raw 에 다시 싣지 않는다.
                TaskCommand::Reduce { .. } => RawResult {
                    exit_code: raw.exit_code,
                    execution: None,
                    postprocess: None,
                },
                _ => raw.clone(),
            },
            artifacts: Vec::new(),
            error: None,
            provenance: provenance(source),
        },
        Err(e) => failed(
            FailureStage::OutputValidation,
            TaskFailure::typed(FailureStage::OutputValidation, Some(task.id.clone()), e),
            source,
        ),
    }
}

/// v1 형식 결과 필드로 투영한다. 기존 조회·표시 경로가 v2 task 도 같은 필드로 읽는다.
pub fn project_v1(typed: &TypedResult) -> TaskResult {
    TaskResult {
        exit_code: typed.raw.exit_code,
        // v1 의 output 은 무타입 JSON 이라 wire 형식(int64 는 10진 문자열)으로 둔다.
        output: typed.has_output.then(|| typed.output.to_wire()),
        error: typed.error.as_ref().map(|e| e.message.clone()),
    }
}

/// 직렬화된 [`TypedResult`]. 출력은 스키마를 받아 [`TypedResult::from_wire`] 에서 typed 값이 된다.
#[derive(Deserialize)]
pub(crate) struct TypedResultWire {
    has_output: bool,
    #[serde(default)]
    output: Value,
    #[serde(default)]
    raw: RawResult,
    #[serde(default)]
    artifacts: Vec<ArtifactRef>,
    #[serde(default)]
    error: Option<TaskFailure>,
    provenance: Provenance,
}

impl TypedResult {
    /// 계약의 출력 스키마로 직렬화된 결과를 읽는다.
    pub(crate) fn from_wire(
        wire: TypedResultWire,
        defs: &TypeDefs,
        schema: &TypeSchema,
    ) -> Result<Self, TypeError> {
        let output = if wire.has_output {
            defs.typed_value(schema, &wire.output)?
        } else {
            TypedValue::Null
        };
        Ok(TypedResult {
            has_output: wire.has_output,
            output,
            raw: wire.raw,
            artifacts: wire.artifacts,
            error: wire.error,
            provenance: wire.provenance,
        })
    }
}

#[cfg(test)]
#[path = "contract_tests.rs"]
mod tests;
