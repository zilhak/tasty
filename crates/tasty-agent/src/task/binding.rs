//! v2 task 입력 binding — 선언 형식, 생성 시 검증, 실행 직전 해석.
//!
//! binding 은 입력 object 의 필드 하나를 상수(`literal`) 또는 선행 task 출력의 JSON
//! Pointer(`from_task` + `pointer`)로 채운다. task ID 와 pointer 를 별도 필드로 받으므로
//! ID 에 점이 있어도 모호하지 않고 문자열 placeholder 를 파싱하지 않는다. 해석한 값은
//! [`InputSnapshot`] 에 고정하며 실행 인자에는 [`InputMapping`] 이 지정한 자리에만 값으로
//! 넣는다. 값 안의 placeholder·셸 구문은 다시 해석하지 않는다.

use std::collections::BTreeMap;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};

use super::contract::{FailureStage, TaskContract, TaskFailure};
use super::types::{
    FieldSchema, TypeDefs, TypeError, TypeErrorKind, TypeKind, TypeSchema, TypedValue,
};
use super::{Task, TaskCommand, TaskId, TaskState};

/// 입력 필드 하나를 채우는 방법.
#[derive(Debug, Clone, PartialEq)]
pub enum InputBinding {
    /// 상수. 생성 시 대상 필드 타입으로 검증한다.
    Literal(Value),
    /// 선행 task 출력의 한 위치. 그 task 자신이 성공해야 값이 있다. fallback 의 성공은
    /// 이 task 의 출력을 대신하지 않는다.
    FromTask {
        source: BindingSource,
        convert: Option<Conversion>,
    },
    /// 여러 source 중 실제로 성공한 정확히 하나의 출력. main 과 fallback 처럼 둘 중
    /// 하나만 성공하는 경로에서 값을 받을 때 쓴다. 모든 source 가 종결된 뒤 해석한다.
    OneOf {
        sources: Vec<BindingSource>,
        convert: Option<Conversion>,
    },
}

/// binding 이 읽는 출력 위치. `pointer` 는 RFC 6901 JSON Pointer 이며 빈 문자열은 출력 전체다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingSource {
    pub from_task: TaskId,
    #[serde(default)]
    pub pointer: String,
}

/// 명시 변환. 변환 없이 받을 수 없는 타입은 생성 시 거절한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conversion {
    /// source 값을 실행 시 대상 타입으로 검증한다. `json` 출력을 구체 타입으로 받을 때 쓴다.
    Assert,
    /// string·enum·int64·boolean 을 문자열로 바꾼다. int64 는 10진 표기다.
    ToString,
    /// int64 를 float64 로 바꾼다. f64 로 정확히 표현되지 않는 값은 실행 시 오류다.
    Int64ToFloat64,
}

impl InputBinding {
    /// 이 binding 이 읽는 source 전체.
    pub fn sources(&self) -> &[BindingSource] {
        match self {
            InputBinding::Literal(_) => &[],
            InputBinding::FromTask { source, .. } => std::slice::from_ref(source),
            InputBinding::OneOf { sources, .. } => sources,
        }
    }

    fn convert(&self) -> Option<Conversion> {
        match self {
            InputBinding::Literal(_) => None,
            InputBinding::FromTask { convert, .. } | InputBinding::OneOf { convert, .. } => {
                *convert
            }
        }
    }
}

impl Serialize for InputBinding {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = Map::new();
        match self {
            InputBinding::Literal(v) => {
                m.insert("literal".into(), v.clone());
            }
            InputBinding::FromTask { source, convert } => {
                m.insert("from_task".into(), Value::String(source.from_task.clone()));
                m.insert("pointer".into(), Value::String(source.pointer.clone()));
                if let Some(c) = convert {
                    m.insert(
                        "convert".into(),
                        serde_json::to_value(c).map_err(serde::ser::Error::custom)?,
                    );
                }
            }
            InputBinding::OneOf { sources, convert } => {
                m.insert(
                    "one_of".into(),
                    serde_json::to_value(sources).map_err(serde::ser::Error::custom)?,
                );
                if let Some(c) = convert {
                    m.insert(
                        "convert".into(),
                        serde_json::to_value(c).map_err(serde::ser::Error::custom)?,
                    );
                }
            }
        }
        Value::Object(m).serialize(s)
    }
}

impl<'de> Deserialize<'de> for InputBinding {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut m = Map::<String, Value>::deserialize(d)?;
        let convert = match m.remove("convert") {
            None => None,
            Some(v) => Some(serde_json::from_value::<Conversion>(v).map_err(D::Error::custom)?),
        };
        let binding = if let Some(v) = m.remove("literal") {
            if convert.is_some() {
                return Err(D::Error::custom("a literal binding takes no 'convert'"));
            }
            InputBinding::Literal(v)
        } else if let Some(v) = m.remove("one_of") {
            let sources: Vec<BindingSource> =
                serde_json::from_value(v).map_err(D::Error::custom)?;
            InputBinding::OneOf { sources, convert }
        } else if let Some(v) = m.remove("from_task") {
            let Value::String(from_task) = v else {
                return Err(D::Error::custom("'from_task' must be a task id string"));
            };
            let pointer = match m.remove("pointer") {
                None => String::new(),
                Some(Value::String(p)) => p,
                Some(_) => return Err(D::Error::custom("'pointer' must be a string")),
            };
            InputBinding::FromTask {
                source: BindingSource { from_task, pointer },
                convert,
            }
        } else {
            return Err(D::Error::custom(
                "binding needs one of 'literal', 'from_task' or 'one_of'",
            ));
        };
        if let Some(k) = m.keys().next() {
            return Err(D::Error::custom(format!("unknown binding field '{k}'")));
        }
        Ok(binding)
    }
}

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
}

impl InputMapping {
    pub fn is_empty(&self) -> bool {
        self.args.is_empty() && !self.stdin && self.params.is_empty() && !self.input_block
    }
}

// ── 입력 snapshot ────────────────────────────────────────────────────────────

/// 한 실행 직전에 해석한 입력. 원본 정의(계약의 binding, command)와 따로 저장한다.
/// 실행 중 upstream 이 재시도돼도 이 값은 바뀌지 않는다.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InputSnapshot {
    pub resolved_at: u64,
    /// 검증을 마친 입력(default 적용). 해석에 실패하면 null 이다.
    pub value: TypedValue,
    /// 값을 읽은 producer 와 그 회차 식별.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<SourcePin>,
    /// 실행에 넘긴 값. 원본 command 는 바꾸지 않는다.
    #[serde(default, skip_serializing_if = "ResolvedExecution::is_empty")]
    pub execution: ResolvedExecution,
    /// 해석 실패. 있으면 실행하지 않았다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure: Option<TaskFailure>,
}

/// binding 이 읽은 producer 하나. producer 의 회차 id 로 어느 실행의 결과인지 고정한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourcePin {
    pub field: String,
    pub from_task: TaskId,
    pub pointer: String,
    /// 값을 읽은 producer 회차([`super::TaskAttempt::id`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub producer_attempt: Option<String>,
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
}

impl ResolvedExecution {
    pub fn is_empty(&self) -> bool {
        self.args.is_empty() && !self.stdin && self.params.is_none() && self.instruction.is_none()
    }
}

/// 직렬화된 [`InputSnapshot`]. 값은 계약의 입력 스키마로 typed 값이 된다.
#[derive(Deserialize)]
pub(crate) struct InputSnapshotWire {
    resolved_at: u64,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    sources: Vec<SourcePin>,
    #[serde(default)]
    execution: ResolvedExecution,
    #[serde(default)]
    failure: Option<TaskFailure>,
}

impl InputSnapshot {
    pub(crate) fn from_wire(
        wire: InputSnapshotWire,
        defs: &TypeDefs,
        schema: &TypeSchema,
    ) -> Result<Self, TypeError> {
        let value = if wire.failure.is_some() {
            TypedValue::Null
        } else {
            defs.typed_value(schema, &wire.value)?
        };
        Ok(Self {
            resolved_at: wire.resolved_at,
            value,
            sources: wire.sources,
            execution: wire.execution,
            failure: wire.failure,
        })
    }
}

/// 입력 필드가 빠져도 되는가(optional 이거나 default 가 있다).
pub fn field_may_be_absent(contract: &TaskContract, field: &str) -> bool {
    let defs = contract.defs();
    let input = contract.input_schema();
    match defs.resolve(&input).map(|r| r.kind) {
        Ok(TypeKind::Object { fields }) => fields
            .get(field)
            .is_some_and(|f| f.optional || f.default.is_some()),
        _ => false,
    }
}

/// 이 task 가 실행 전에 입력을 해석해야 하는가. unit 입력에 binding·mapping 이 없으면
/// 해석할 것이 없다.
pub fn needs_input(contract: &TaskContract) -> bool {
    !contract.bindings.is_empty()
        || contract
            .input_mapping
            .as_ref()
            .is_some_and(|m| !m.is_empty())
        || contract.input_schema.is_some()
}

// ── JSON Pointer ────────────────────────────────────────────────────────────

/// RFC 6901 pointer 를 토큰으로 나눈다. 빈 문자열은 전체다.
pub fn pointer_tokens(pointer: &str) -> Result<Vec<String>, String> {
    if pointer.is_empty() {
        return Ok(Vec::new());
    }
    let Some(rest) = pointer.strip_prefix('/') else {
        return Err(format!(
            "JSON pointer '{pointer}' must be empty or start with '/'"
        ));
    };
    rest.split('/')
        .map(|raw| {
            let mut out = String::with_capacity(raw.len());
            let mut chars = raw.chars();
            while let Some(c) = chars.next() {
                if c == '~' {
                    match chars.next() {
                        Some('0') => out.push('~'),
                        Some('1') => out.push('/'),
                        _ => {
                            return Err(format!(
                                "JSON pointer '{pointer}' has an invalid '~' escape"
                            ));
                        }
                    }
                } else {
                    out.push(c);
                }
            }
            Ok(out)
        })
        .collect()
}

fn is_array_index(token: &str) -> bool {
    !token.is_empty()
        && token.bytes().all(|b| b.is_ascii_digit())
        && (token == "0" || !token.starts_with('0'))
}

/// 스키마 안에서 pointer 가 가리키는 타입. 두 번째 값은 그 위치의 값이 빠질 수 있는지
/// (default 없는 optional 필드를 지난다)다. list 원소와 `json` 안쪽은 실행 시 확인한다.
pub fn schema_at(
    defs: &TypeDefs,
    schema: &TypeSchema,
    pointer: &str,
) -> Result<(TypeSchema, bool), TypeError> {
    let mismatch = |at: &str, expected: &str, actual: String| TypeError {
        kind: TypeErrorKind::TypeMismatch,
        path: at.to_string(),
        expected: expected.to_string(),
        actual,
    };
    let tokens = pointer_tokens(pointer).map_err(|m| mismatch(pointer, "a JSON pointer", m))?;
    let mut cur = schema.clone();
    let mut may_be_absent = false;
    let mut at = String::new();
    for token in tokens {
        at = format!("{at}/{}", token.replace('~', "~0").replace('/', "~1"));
        let resolved = defs.resolve(&cur)?;
        if resolved.nullable {
            may_be_absent = true;
        }
        let next = match resolved.kind {
            TypeKind::Object { fields } => match fields.get(&token) {
                Some(FieldSchema {
                    schema,
                    optional,
                    default,
                }) => {
                    if *optional && default.is_none() {
                        may_be_absent = true;
                    }
                    schema.clone()
                }
                None => {
                    return Err(TypeError {
                        kind: TypeErrorKind::MissingField,
                        path: at,
                        expected: "a field declared in the source output".into(),
                        actual: format!("\"{token}\""),
                    });
                }
            },
            TypeKind::List { items, .. } if is_array_index(&token) => (**items).clone(),
            TypeKind::Json => return Ok((TypeSchema::json(), may_be_absent)),
            other => {
                return Err(mismatch(
                    &at,
                    "an object field or list index",
                    format!(
                        "{} has no \"{token}\"",
                        TypeSchema::new(other.clone()).describe()
                    ),
                ));
            }
        };
        cur = next;
    }
    Ok((cur, may_be_absent))
}

/// 출력 스키마. v1 task 의 출력은 구조가 정해지지 않은 json 이다.
pub fn output_schema_of(task: &Task) -> (TypeDefs, TypeSchema) {
    match &task.contract {
        Some(c) => (c.defs(), c.output_schema(&task.command)),
        None => (TypeDefs::default(), TypeSchema::json()),
    }
}

// ── 생성 시 검증 ─────────────────────────────────────────────────────────────

fn input_failure(message: impl Into<String>, location: String) -> TaskFailure {
    TaskFailure {
        location: Some(location),
        ..TaskFailure::new(FailureStage::Input, message)
    }
}

fn typed_failure(
    task_id: Option<&TaskId>,
    e: TypeError,
    location: String,
    message: Option<String>,
) -> TaskFailure {
    let mut f = TaskFailure::typed(FailureStage::Input, task_id.cloned(), e);
    if let Some(m) = message {
        f.message = m;
    }
    f.location = Some(location);
    f
}

/// 입력 스키마·binding·mapping 을 검사한다. `lookup` 은 source task 를 찾는다(같은 그래프의
/// 새 task 포함). `at` 은 오류 위치의 접두사다(예: `/tasks/2`).
pub fn check_inputs<'a>(
    task_id: &TaskId,
    contract: &TaskContract,
    command: &TaskCommand,
    at: &str,
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    let defs = contract.defs();
    let input = contract.input_schema();
    let typed = |e: TypeError, loc: String| typed_failure(Some(task_id), e, loc, None);
    let resolved = defs
        .resolve(&input)
        .map_err(|e| typed(e, format!("{at}/input_schema")))?;
    let fields = match resolved.kind {
        TypeKind::Object { fields } => Some(fields),
        _ => None,
    };

    if fields.is_none() && !contract.bindings.is_empty() {
        return Err(input_failure(
            format!(
                "task {task_id}: bindings fill object input fields; input_schema is {}",
                input.describe()
            ),
            format!("{at}/bindings"),
        ));
    }
    match fields {
        Some(fields) => {
            for name in contract.bindings.keys() {
                if !fields.contains_key(name) {
                    return Err(input_failure(
                        format!("task {task_id}: binding for undeclared input field '{name}'"),
                        format!("{at}/bindings/{}", escape(name)),
                    ));
                }
            }
            for (name, field) in fields {
                let loc = format!("{at}/bindings/{}", escape(name));
                match contract.bindings.get(name) {
                    Some(b) => check_binding(task_id, &defs, field, b, &loc, lookup)?,
                    None if field.optional || field.default.is_some() => {}
                    None => {
                        return Err(input_failure(
                            format!(
                                "task {task_id}: required input field '{name}' ({}) has no binding or default",
                                field.schema.describe()
                            ),
                            loc,
                        ));
                    }
                }
            }
        }
        None => {
            // binding 이 없는 비object 입력은 기본값이 없으면 채울 수 없다(unit 은 null).
            defs.validate(&input, &Value::Null).map_err(|mut e| {
                e.path = format!("/input{}", e.path);
                typed(e, format!("{at}/input_schema"))
            })?;
        }
    }
    check_mapping(task_id, &defs, &input, contract, command, at)
}

fn escape(token: &str) -> String {
    token.replace('~', "~0").replace('/', "~1")
}

fn check_binding<'a>(
    task_id: &TaskId,
    defs: &TypeDefs,
    field: &FieldSchema,
    binding: &InputBinding,
    loc: &str,
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    let target = &field.schema;
    let field_may_be_absent = field.optional || field.default.is_some();
    match binding {
        InputBinding::Literal(v) => defs.validate(target, v).map(|_| ()).map_err(|mut e| {
            e.path = format!("/literal{}", e.path);
            typed_failure(Some(task_id), e, loc.to_string(), None)
        }),
        InputBinding::FromTask { source, convert } => check_source(
            task_id,
            defs,
            target,
            field_may_be_absent,
            source,
            *convert,
            loc,
            lookup,
        ),
        InputBinding::OneOf { sources, convert } => {
            if sources.len() < 2 {
                return Err(input_failure(
                    format!("task {task_id}: one_of needs at least two sources"),
                    format!("{loc}/one_of"),
                ));
            }
            for (i, s) in sources.iter().enumerate() {
                if sources[..i].iter().any(|p| p.from_task == s.from_task) {
                    return Err(input_failure(
                        format!(
                            "task {task_id}: one_of lists task {} more than once",
                            s.from_task
                        ),
                        format!("{loc}/one_of/{i}"),
                    ));
                }
                check_source(
                    task_id,
                    defs,
                    target,
                    field_may_be_absent,
                    s,
                    *convert,
                    &format!("{loc}/one_of/{i}"),
                    lookup,
                )?;
            }
            Ok(())
        }
    }
}

#[allow(clippy::too_many_arguments)] // reason: 오류 위치와 대상 필드 정보를 함께 넘기는 내부 검사
fn check_source<'a>(
    task_id: &TaskId,
    defs: &TypeDefs,
    target: &TypeSchema,
    field_may_be_absent: bool,
    source: &BindingSource,
    convert: Option<Conversion>,
    loc: &str,
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    let Some(producer) = lookup(&source.from_task) else {
        return Err(input_failure(
            format!(
                "task {task_id}: binding source task not found: {}",
                source.from_task
            ),
            format!("{loc}/from_task"),
        ));
    };
    if &producer.id == task_id {
        return Err(input_failure(
            format!("task {task_id}: a task cannot bind its own output"),
            format!("{loc}/from_task"),
        ));
    }
    let (sdefs, sschema) = output_schema_of(producer);
    let describe_source = || {
        format!(
            "task {task_id}: input from task {} at '{}'",
            source.from_task, source.pointer
        )
    };
    let (sub, may_be_absent) = schema_at(&sdefs, &sschema, &source.pointer).map_err(|e| {
        let msg = format!("{}: {e}", describe_source());
        typed_failure(Some(task_id), e, format!("{loc}/pointer"), Some(msg))
    })?;
    if may_be_absent && !field_may_be_absent {
        return Err(input_failure(
            format!(
                "{}: the source value may be absent (optional or nullable on the way); \
                 make the input field optional or give it a default",
                describe_source()
            ),
            format!("{loc}/pointer"),
        ));
    }
    let incompatible = |e: TypeError| {
        let msg = format!("{}: {e}", describe_source());
        typed_failure(Some(task_id), e, loc.to_string(), Some(msg))
    };
    let unsupported = |what: &str| {
        let e = TypeError {
            kind: TypeErrorKind::Incompatible,
            path: String::new(),
            expected: what.to_string(),
            actual: sub.describe(),
        };
        incompatible(e)
    };
    match convert {
        None => super::types::check_assignable(&sdefs, &sub, defs, target).map_err(incompatible),
        Some(Conversion::Assert) => Ok(()),
        Some(Conversion::ToString) => {
            let r = sdefs.resolve(&sub).map_err(incompatible)?;
            let ok = matches!(
                r.kind,
                TypeKind::String { .. }
                    | TypeKind::Enum { .. }
                    | TypeKind::Int64
                    | TypeKind::Boolean
            ) && !r.nullable;
            if !ok {
                return Err(unsupported("string, enum, int64 or boolean for to_string"));
            }
            super::types::check_assignable(
                &TypeDefs::default(),
                &TypeSchema::string(),
                defs,
                target,
            )
            .map_err(incompatible)
        }
        Some(Conversion::Int64ToFloat64) => {
            let r = sdefs.resolve(&sub).map_err(incompatible)?;
            if !matches!(r.kind, TypeKind::Int64) || r.nullable {
                return Err(unsupported("int64 for int64_to_float64"));
            }
            let t = defs.resolve(target).map_err(incompatible)?;
            if !matches!(t.kind, TypeKind::Float64 { .. }) {
                let e = TypeError {
                    kind: TypeErrorKind::Incompatible,
                    path: String::new(),
                    expected: "a float64 input field for int64_to_float64".into(),
                    actual: target.describe(),
                };
                return Err(incompatible(e));
            }
            Ok(())
        }
    }
}

fn check_mapping(
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
    let kind = super::contract::command_kind(command);
    let is_run = matches!(command, TaskCommand::Run { .. });
    let is_custom = matches!(command, TaskCommand::Custom { .. });
    let is_agent = matches!(command, TaskCommand::Agent { .. });
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
    if !input_is_unit && !is_run && !is_custom && !is_agent {
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

// ── 실행 직전 해석 ───────────────────────────────────────────────────────────

/// binding 을 해석해 입력 snapshot 을 만든다. `lookup` 은 저장소의 producer 를 찾는다.
/// `base_params` 는 lease·v1 placeholder 를 치환한 custom params 다.
pub fn resolve_inputs(
    task: &Task,
    contract: &TaskContract,
    base_params: Option<&Value>,
    now_ms: u64,
    lookup: &impl Fn(&TaskId) -> Option<Task>,
) -> InputSnapshot {
    let mut pins = Vec::new();
    match resolve_value(task, contract, &mut pins, lookup).and_then(|value| {
        let execution = resolve_execution(task, contract, &value, base_params)?;
        Ok((value, execution))
    }) {
        Ok((value, execution)) => InputSnapshot {
            resolved_at: now_ms,
            value,
            sources: pins,
            execution,
            failure: None,
        },
        Err(failure) => InputSnapshot {
            resolved_at: now_ms,
            value: TypedValue::Null,
            sources: pins,
            execution: ResolvedExecution::default(),
            failure: Some(failure),
        },
    }
}

fn producer_output(producer: &Task) -> Option<Value> {
    if !matches!(producer.state, TaskState::Succeeded) {
        return None;
    }
    match &producer.typed_result {
        Some(t) => t.has_output.then(|| t.output.to_internal()),
        None => producer.result.as_ref().and_then(|r| r.output.clone()),
    }
}

fn resolve_value(
    task: &Task,
    contract: &TaskContract,
    pins: &mut Vec<SourcePin>,
    lookup: &impl Fn(&TaskId) -> Option<Task>,
) -> Result<TypedValue, TaskFailure> {
    let defs = contract.defs();
    let input = contract.input_schema();
    let fail = |msg: String, loc: String| input_failure(format!("task {}: {msg}", task.id), loc);
    let fields = match defs.resolve(&input).map(|r| r.kind.clone()) {
        Ok(TypeKind::Object { fields }) => fields,
        _ => {
            return defs
                .validate_typed(&input, &Value::Null)
                .map_err(|e| typed_failure(Some(&task.id), e, "/input_schema".into(), None));
        }
    };
    let mut obj = Map::new();
    for (name, binding) in &contract.bindings {
        let loc = format!("/bindings/{}", escape(name));
        let field = &fields[name];
        let read =
            |s: &BindingSource, pins: &mut Vec<SourcePin>| -> Result<Option<Value>, TaskFailure> {
                let producer = lookup(&s.from_task).ok_or_else(|| {
                    fail(
                        format!("binding source task not found: {}", s.from_task),
                        loc.clone(),
                    )
                })?;
                let Some(output) = producer_output(&producer) else {
                    return Ok(None);
                };
                pins.push(SourcePin {
                    field: name.clone(),
                    from_task: s.from_task.clone(),
                    pointer: s.pointer.clone(),
                    producer_attempt: producer.attempt.as_ref().map(|a| a.id.clone()),
                });
                let tokens = pointer_tokens(&s.pointer).map_err(|m| fail(m, loc.clone()))?;
                let mut cur = &output;
                for t in &tokens {
                    let next = match cur {
                        Value::Object(m) => m.get(t.as_str()),
                        Value::Array(a) if is_array_index(t) => {
                            t.parse::<usize>().ok().and_then(|i| a.get(i))
                        }
                        _ => None,
                    };
                    match next {
                        Some(v) => cur = v,
                        None => {
                            return Err(fail(
                                format!(
                                    "output of task {} has nothing at '{}'",
                                    s.from_task, s.pointer
                                ),
                                loc.clone(),
                            ));
                        }
                    }
                }
                Ok(Some(cur.clone()))
            };
        let raw = match binding {
            InputBinding::Literal(v) => Some(v.clone()),
            // 경로가 선택되지 않은 source 의 값은 없다. 빠져도 되는 필드는 비워 둔다.
            InputBinding::FromTask { source, .. }
                if (field.optional || field.default.is_some())
                    && lookup(&source.from_task)
                        .is_some_and(|p| super::route::is_not_selected(&p)) =>
            {
                None
            }
            InputBinding::FromTask { source, .. } => {
                Some(read(source, pins)?.ok_or_else(|| {
                    fail(
                        format!("source task {} has no successful output", source.from_task),
                        loc.clone(),
                    )
                })?)
            }
            InputBinding::OneOf { sources, .. } => {
                let mut found = Vec::new();
                for s in sources {
                    if let Some(v) = read(s, pins)? {
                        found.push((s, v));
                    }
                }
                match found.len() {
                    0 if field.optional || field.default.is_some() => None,
                    0 => {
                        return Err(fail(
                            "no one_of source succeeded with an output".into(),
                            loc.clone(),
                        ));
                    }
                    1 => found.pop().map(|(_, v)| v),
                    _ => {
                        let ids: Vec<&str> =
                            found.iter().map(|(s, _)| s.from_task.as_str()).collect();
                        return Err(fail(
                            format!(
                                "one_of expects exactly one succeeded source, got {}",
                                ids.join(", ")
                            ),
                            loc.clone(),
                        ));
                    }
                }
            }
        };
        let Some(raw) = raw else { continue };
        let converted = convert_value(binding.convert(), raw)
            .map_err(|e| typed_failure(Some(&task.id), e, loc.clone(), None))?;
        obj.insert(name.clone(), converted);
    }
    defs.validate_typed(&input, &Value::Object(obj))
        .map_err(|mut e| {
            let loc = match e.path.split('/').nth(1) {
                Some(field) if !field.is_empty() => format!("/bindings/{field}"),
                _ => "/input_schema".to_string(),
            };
            e.path = format!("/input{}", e.path);
            typed_failure(Some(&task.id), e, loc, None)
        })
}

fn convert_value(convert: Option<Conversion>, v: Value) -> Result<Value, TypeError> {
    let mismatch = |expected: &str, v: &Value| TypeError {
        kind: TypeErrorKind::TypeMismatch,
        path: String::new(),
        expected: expected.to_string(),
        actual: super::types::describe_value(v),
    };
    match convert {
        None | Some(Conversion::Assert) => Ok(v),
        Some(Conversion::ToString) => match &v {
            Value::String(_) => Ok(v),
            Value::Bool(b) => Ok(Value::String(b.to_string())),
            Value::Number(n) if n.is_i64() => Ok(Value::String(n.to_string())),
            _ => Err(mismatch("string, int64 or boolean for to_string", &v)),
        },
        Some(Conversion::Int64ToFloat64) => {
            let i = super::types::int64_of(&v).map_err(|kind| TypeError {
                kind,
                ..mismatch("int64", &v)
            })?;
            let f = i as f64;
            if f as i128 != i as i128 {
                return Err(TypeError {
                    kind: TypeErrorKind::OutOfRange,
                    path: String::new(),
                    expected: "an int64 that float64 represents exactly".into(),
                    actual: i.to_string(),
                });
            }
            serde_json::Number::from_f64(f)
                .map(Value::Number)
                .ok_or_else(|| mismatch("a finite float64", &v))
        }
    }
}

fn value_at<'v>(value: &'v Value, pointer: &str) -> Option<&'v Value> {
    let tokens = pointer_tokens(pointer).ok()?;
    let mut cur = value;
    for t in &tokens {
        cur = match cur {
            Value::Object(m) => m.get(t.as_str())?,
            Value::Array(a) => a.get(t.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn resolve_execution(
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
    if mapping.input_block
        && let TaskCommand::Agent { instruction, .. } = &task.command
    {
        out.instruction = Some(super::agent::compose_instruction(
            instruction,
            Some(&value.to_wire()),
        ));
    }
    Ok(out)
}

#[cfg(test)]
#[path = "binding_tests.rs"]
mod tests;
