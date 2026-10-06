//! v2 task 의 전이 조건과 경로 선택.
//!
//! 전이는 성공한 task 의 확정된 출력으로 후속 task 를 고르는 선언이다. 조건은 출력에 대한
//! 제한된 순수 식(비교·enum 소속·논리 조합)이며 셸·네트워크·시각에 의존하지 않는다. 실행
//! 실패는 실패 정책(`on_failure`)이 다루고 조건을 평가하지 않는다.
//!
//! 고른 경로는 결과와 같은 레코드 쓰기로 저장한다([`RouteDecision`]). 고르지 않은 대상은
//! 실패가 아니라 [`SkipReason::BranchNotSelected`] 로 끝나며, 그 하류도 같은 이유로 끝난다.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::binding::{output_schema_of, pointer_tokens, schema_at};
use super::contract::{self, FailureStage, TaskFailure};
use super::types::{TypeDefs, TypeKind, TypeSchema, TypedValue};
use super::{OnFailure, ReducerStrategy, Task, TaskCommand, TaskId};

/// 성공한 출력으로 후속 경로를 고르는 선언.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transitions {
    #[serde(default)]
    pub mode: TransitionMode,
    pub cases: Vec<TransitionCase>,
    /// 맞는 case 가 없을 때 고를 대상. `no_match` 와 둘 중 하나만 둔다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub otherwise: Option<Vec<TaskId>>,
    /// 맞는 case 가 없을 때 아무 경로도 고르지 않고 끝낸다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub no_match: Option<NoMatch>,
}

/// case 를 고르는 방식.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionMode {
    /// 참인 case 가 정확히 하나여야 한다. 둘 이상이면 조건 오류다.
    #[default]
    Exclusive,
    /// 참인 case 를 모두 고른다.
    AllMatches,
}

/// 조건과 그 조건이 참일 때 고를 대상.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TransitionCase {
    pub when: Condition,
    pub to: Vec<TaskId>,
}

/// 맞는 case 가 없을 때의 처리.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoMatch {
    Finish,
}

/// 출력에 대한 조건식.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Condition {
    All(Vec<Condition>),
    Any(Vec<Condition>),
    Not(Box<Condition>),
    Compare(Comparison),
    In(Membership),
}

/// 출력의 한 위치와 상수의 비교.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    /// 출력 안의 RFC 6901 JSON Pointer. 빈 문자열은 출력 전체다.
    #[serde(default)]
    pub path: String,
    pub op: CompareOp,
    pub value: Value,
    /// reduce `all` 전용. 이 입력 task 의 레코드 출력을 그 task 의 출력 타입으로 읽는다.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<TaskId>,
}

/// 출력의 한 위치가 상수 목록 중 하나인가.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Membership {
    #[serde(default)]
    pub path: String,
    pub values: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<TaskId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

/// 한 회차가 고른 경로. 결과와 같은 레코드 쓰기로 저장한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteDecision {
    /// 경로를 고른 회차.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    /// 참이 된 case 의 순번(0부터).
    pub matched: Vec<usize>,
    /// 맞는 case 가 없어 `otherwise` 를 골랐다.
    #[serde(default)]
    pub otherwise: bool,
    /// 고른 대상 task. 선언 순서이며 중복이 없다.
    pub selected: Vec<TaskId>,
}

/// v2 task 가 실행 없이 Skipped 로 끝난 이유.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum SkipReason {
    /// 들어오는 경로가 모두 선택되지 않았다. 실패가 아니다.
    BranchNotSelected,
    /// 필요한 선행이 성공 결과를 내지 못했다. `source_state` 는 그 선행의 상태다.
    UpstreamUnavailable {
        source: TaskId,
        source_state: String,
    },
}

impl Transitions {
    /// 고를 수 있는 대상 전체. 중복을 포함할 수 있다.
    pub fn targets(&self) -> impl Iterator<Item = &TaskId> {
        self.cases
            .iter()
            .flat_map(|c| c.to.iter())
            .chain(self.otherwise.iter().flatten())
    }
}

/// task 가 전이로 고를 수 있는 대상.
pub fn transition_targets(task: &Task) -> Vec<&TaskId> {
    match task.contract.as_ref().and_then(|c| c.transitions.as_ref()) {
        Some(t) => {
            let mut out: Vec<&TaskId> = Vec::new();
            for id in t.targets() {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
            out
        }
        None => Vec::new(),
    }
}

/// 경로를 고르지 않아 끝난 task 인가.
pub fn is_not_selected(task: &Task) -> bool {
    matches!(task.skip, Some(SkipReason::BranchNotSelected))
}

// ── 제출 시 검사 ─────────────────────────────────────────────────────────────

fn route_error(message: impl Into<String>, location: String) -> TaskFailure {
    TaskFailure {
        location: Some(location),
        ..TaskFailure::new(FailureStage::Input, message)
    }
}

/// 그래프로 제출하는 task 하나의 전이를 검사한다. `graph` 는 같은 그래프의 task 전체다.
/// 대상은 같은 그래프 안에 있어야 한다. `lookup` 은 reduce 입력 task 를 찾는다.
pub(crate) fn check_transitions<'a>(
    task: &Task,
    at: &str,
    graph: &[Task],
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    let Some(t) = task.contract.as_ref().and_then(|c| c.transitions.as_ref()) else {
        return Ok(());
    };
    let at = format!("{at}/transitions");
    let fail = |m: String, loc: &str| {
        let mut f = route_error(format!("task {}: {m}", task.id), format!("{at}{loc}"));
        f.task_id = Some(task.id.clone());
        f
    };
    if t.cases.is_empty() {
        return Err(fail("transitions need at least one case".into(), "/cases"));
    }
    match (&t.otherwise, t.no_match) {
        (Some(_), Some(_)) => {
            return Err(fail(
                "set either otherwise or no_match, not both".into(),
                "",
            ));
        }
        (None, None) => {
            return Err(fail(
                "say what happens when no case holds: otherwise [targets] or no_match \"finish\""
                    .into(),
                "",
            ));
        }
        (Some(o), None) if o.is_empty() => {
            return Err(fail(
                "otherwise needs at least one target; use no_match \"finish\" to select nothing"
                    .into(),
                "/otherwise",
            ));
        }
        _ => {}
    }
    let check_target = |id: &TaskId, loc: String| -> Result<(), TaskFailure> {
        let Some(target) = graph.iter().find(|g| &g.id == id) else {
            return Err(fail(
                format!("transition target {id} is not a task of this graph"),
                &loc,
            ));
        };
        if target.id == task.id {
            return Err(fail("a task cannot select itself".into(), &loc));
        }
        if matches!(target.on_failure, OnFailure::ContinueDownstream) {
            return Err(fail(
                format!(
                    "transition target {id} uses continue_downstream, which would run it when the \
                     route is unknown; a target runs only when selected"
                ),
                &loc,
            ));
        }
        if let Some(main) = graph.iter().find(
            |g| matches!(&g.on_failure, OnFailure::Fallback { task: Some(fb), .. } if fb == id),
        ) {
            return Err(fail(
                format!(
                    "transition target {id} is the fallback of {}; a fallback runs only when its main fails",
                    main.id
                ),
                &loc,
            ));
        }
        Ok(())
    };
    for (i, case) in t.cases.iter().enumerate() {
        if case.to.is_empty() {
            return Err(fail(
                "a case needs at least one target".into(),
                &format!("/cases/{i}/to"),
            ));
        }
        for (j, id) in case.to.iter().enumerate() {
            check_target(id, format!("/cases/{i}/to/{j}"))?;
        }
        check_condition(task, &case.when, &format!("{at}/cases/{i}/when"), lookup)?;
    }
    for (j, id) in t.otherwise.iter().flatten().enumerate() {
        check_target(id, format!("/otherwise/{j}"))?;
    }
    if t.mode == TransitionMode::Exclusive {
        check_exclusive_overlap(task, t, &at, lookup)?;
    }
    Ok(())
}

/// 조건이 읽는 값의 타입. `input` 이 있으면 reduce `all` 입력의 출력 타입이다.
fn operand_schema<'a>(
    task: &Task,
    path: &str,
    input: Option<&TaskId>,
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(TypeDefs, TypeSchema), String> {
    let (defs, base) = match input {
        None => output_schema_of(task),
        Some(id) => {
            let all_inputs = match &task.command {
                TaskCommand::Reduce {
                    inputs,
                    strategy: ReducerStrategy::All,
                } => inputs,
                _ => return Err("'input' applies to reduce all outputs only".into()),
            };
            if !all_inputs.contains(id) {
                return Err(format!("'input' {id} is not an input of this reduce"));
            }
            let source = lookup(id).ok_or_else(|| format!("reduce input task not found: {id}"))?;
            output_schema_of(source)
        }
    };
    let (schema, _) = schema_at(&defs, &base, path).map_err(|e| e.to_string())?;
    let resolved = defs.resolve(&schema).map_err(|e| e.to_string())?;
    let schema = TypeSchema {
        kind: resolved.kind.clone(),
        nullable: false,
    };
    Ok((defs, schema))
}

fn check_condition<'a>(
    task: &Task,
    cond: &Condition,
    loc: &str,
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    let fail = |m: String, at: &str| {
        let mut f = route_error(format!("task {}: {m}", task.id), format!("{loc}{at}"));
        f.task_id = Some(task.id.clone());
        f
    };
    match cond {
        Condition::All(items) | Condition::Any(items) => {
            let key = if matches!(cond, Condition::All(_)) {
                "all"
            } else {
                "any"
            };
            if items.is_empty() {
                return Err(fail(format!("'{key}' needs at least one condition"), ""));
            }
            for (i, c) in items.iter().enumerate() {
                check_condition(task, c, &format!("{loc}/{key}/{i}"), lookup)?;
            }
            Ok(())
        }
        Condition::Not(inner) => check_condition(task, inner, &format!("{loc}/not"), lookup),
        Condition::Compare(c) => {
            let at = "/compare";
            let (defs, schema) = operand_schema(task, &c.path, c.input.as_ref(), lookup)
                .map_err(|m| fail(m, &format!("{at}/path")))?;
            let ordered = matches!(
                c.op,
                CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge
            );
            match &schema.kind {
                TypeKind::Int64 => {}
                TypeKind::Float64 { .. } if ordered => {}
                TypeKind::Float64 { .. } => {
                    return Err(fail(
                        "float64 values are compared with lt/le/gt/ge ranges, not eq/ne".into(),
                        &format!("{at}/op"),
                    ));
                }
                TypeKind::Boolean | TypeKind::String { .. } | TypeKind::Enum { .. } if !ordered => {
                }
                TypeKind::Boolean | TypeKind::String { .. } | TypeKind::Enum { .. } => {
                    return Err(fail(
                        format!("{} values are compared with eq/ne only", schema.describe()),
                        &format!("{at}/op"),
                    ));
                }
                _ => {
                    return Err(fail(
                        format!(
                            "a condition compares boolean, int64, float64, string or enum values; '{}' is {}",
                            c.path,
                            schema.describe()
                        ),
                        &format!("{at}/path"),
                    ));
                }
            }
            defs.validate(&schema, &c.value)
                .map(|_| ())
                .map_err(|e| fail(e.to_string(), &format!("{at}/value")))
        }
        Condition::In(m) => {
            let at = "/in";
            let (defs, schema) = operand_schema(task, &m.path, m.input.as_ref(), lookup)
                .map_err(|msg| fail(msg, &format!("{at}/path")))?;
            if !matches!(
                schema.kind,
                TypeKind::Int64 | TypeKind::String { .. } | TypeKind::Enum { .. }
            ) {
                return Err(fail(
                    format!(
                        "'in' tests int64, string or enum values; '{}' is {}",
                        m.path,
                        schema.describe()
                    ),
                    &format!("{at}/path"),
                ));
            }
            if m.values.is_empty() {
                return Err(fail(
                    "'in' needs at least one value".into(),
                    &format!("{at}/values"),
                ));
            }
            for (i, v) in m.values.iter().enumerate() {
                defs.validate(&schema, v)
                    .map_err(|e| fail(e.to_string(), &format!("{at}/values/{i}")))?;
            }
            Ok(())
        }
    }
}

/// eq 와 in 처럼 값 집합으로 나타나는 조건의 (입력, 위치, 값 집합).
fn value_set<'a>(
    task: &Task,
    cond: &Condition,
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Option<(Option<TaskId>, String, Vec<TypedValue>)> {
    let (input, path, values) = match cond {
        Condition::Compare(c) if c.op == CompareOp::Eq => {
            (c.input.clone(), c.path.clone(), vec![c.value.clone()])
        }
        Condition::In(m) => (m.input.clone(), m.path.clone(), m.values.clone()),
        _ => return None,
    };
    let (defs, schema) = operand_schema(task, &path, input.as_ref(), lookup).ok()?;
    let typed = values
        .iter()
        .map(|v| defs.typed_value(&schema, v).ok())
        .collect::<Option<Vec<_>>>()?;
    Some((input, path, typed))
}

/// exclusive 에서 두 case 가 함께 참일 수 있음을 제출 시 찾을 수 있는 경우를 거절한다. 같은
/// 조건이거나, 같은 위치의 eq/in 값 집합이 겹치는 경우다. 나머지는 실행 시 검출한다.
fn check_exclusive_overlap<'a>(
    task: &Task,
    t: &Transitions,
    at: &str,
    lookup: &impl Fn(&TaskId) -> Option<&'a Task>,
) -> Result<(), TaskFailure> {
    for (j, b) in t.cases.iter().enumerate() {
        for (i, a) in t.cases[..j].iter().enumerate() {
            let overlap = a.when == b.when
                || match (
                    value_set(task, &a.when, lookup),
                    value_set(task, &b.when, lookup),
                ) {
                    (Some((ia, pa, va)), Some((ib, pb, vb))) => {
                        ia == ib && pa == pb && va.iter().any(|v| vb.contains(v))
                    }
                    _ => false,
                };
            if overlap {
                let mut f = route_error(
                    format!(
                        "task {}: exclusive cases {i} and {j} can both hold; make them disjoint or use mode all_matches",
                        task.id
                    ),
                    format!("{at}/cases/{j}/when"),
                );
                f.task_id = Some(task.id.clone());
                return Err(f);
            }
        }
    }
    Ok(())
}

// ── 실행 뒤 경로 선택 ────────────────────────────────────────────────────────

fn eval_failure(task: &Task, message: String) -> TaskFailure {
    TaskFailure {
        task_id: Some(task.id.clone()),
        ..TaskFailure::new(FailureStage::Route, format!("task {}: {message}", task.id))
    }
}

/// 성공한 task 의 경로를 고른다. 전이가 없으면 `None` 이다. 조건을 평가할 수 없거나
/// exclusive 에서 둘 이상이 참이면 `route` 단계 실패다. `lookup` 은 reduce 입력 task 를 찾는다.
pub fn decide_route(
    task: &Task,
    lookup: &impl Fn(&TaskId) -> Option<Task>,
) -> Result<Option<RouteDecision>, TaskFailure> {
    let Some(t) = task.contract.as_ref().and_then(|c| c.transitions.as_ref()) else {
        return Ok(None);
    };
    let output = match &task.typed_result {
        Some(r) if r.has_output => &r.output,
        _ => {
            return Err(eval_failure(
                task,
                "has no output to evaluate transitions on".into(),
            ));
        }
    };
    let mut matched = Vec::new();
    for (i, case) in t.cases.iter().enumerate() {
        if eval(task, output, &case.when, lookup)? {
            matched.push(i);
        }
    }
    if t.mode == TransitionMode::Exclusive && matched.len() > 1 {
        return Err(eval_failure(
            task,
            format!("exclusive transition cases {matched:?} all hold; exactly one may hold"),
        ));
    }
    let otherwise = matched.is_empty() && t.otherwise.is_some();
    let mut selected: Vec<TaskId> = Vec::new();
    let chosen: Vec<&TaskId> = if otherwise {
        t.otherwise.iter().flatten().collect()
    } else {
        matched.iter().flat_map(|&i| t.cases[i].to.iter()).collect()
    };
    for id in chosen {
        if !selected.contains(id) {
            selected.push(id.clone());
        }
    }
    Ok(Some(RouteDecision {
        attempt_id: task.attempt.as_ref().map(|a| a.id.clone()),
        matched,
        otherwise,
        selected,
    }))
}

fn eval(
    task: &Task,
    output: &TypedValue,
    cond: &Condition,
    lookup: &impl Fn(&TaskId) -> Option<Task>,
) -> Result<bool, TaskFailure> {
    match cond {
        Condition::All(items) => {
            for c in items {
                if !eval(task, output, c, lookup)? {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        Condition::Any(items) => {
            for c in items {
                if eval(task, output, c, lookup)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }
        Condition::Not(inner) => Ok(!eval(task, output, inner, lookup)?),
        Condition::Compare(c) => {
            let (actual, literal) =
                operand(task, output, &c.path, c.input.as_ref(), lookup, &c.value)?;
            let ord = compare(&actual, &literal[0]).ok_or_else(|| {
                eval_failure(task, format!("cannot compare the value at '{}'", c.path))
            })?;
            use std::cmp::Ordering::*;
            Ok(match c.op {
                CompareOp::Eq => ord == Equal,
                CompareOp::Ne => ord != Equal,
                CompareOp::Lt => ord == Less,
                CompareOp::Le => ord != Greater,
                CompareOp::Gt => ord == Greater,
                CompareOp::Ge => ord != Less,
            })
        }
        Condition::In(m) => {
            let mut literals = Vec::new();
            let mut actual = None;
            for v in &m.values {
                let (a, l) = operand(task, output, &m.path, m.input.as_ref(), lookup, v)?;
                actual = Some(a);
                literals.extend(l);
            }
            let actual = actual.unwrap_or(TypedValue::Null);
            Ok(literals
                .iter()
                .any(|l| compare(&actual, l) == Some(std::cmp::Ordering::Equal)))
        }
    }
}

/// 조건이 읽는 값과 상수를 같은 타입의 typed 값으로 만든다.
fn operand(
    task: &Task,
    output: &TypedValue,
    path: &str,
    input: Option<&TaskId>,
    lookup: &impl Fn(&TaskId) -> Option<Task>,
    literal: &Value,
) -> Result<(TypedValue, Vec<TypedValue>), TaskFailure> {
    let fail = |m: String| eval_failure(task, m);
    let record;
    let (base, input_task) = match input {
        None => (output, None),
        Some(id) => {
            let input_task =
                lookup(id).ok_or_else(|| fail(format!("reduce input task not found: {id}")))?;
            record = contract::reduce_all_record_output(task, &input_task)
                .map_err(|f| fail(f.message))?
                .ok_or_else(|| fail(format!("reduce input {id} produced no output")))?;
            (&record, Some(input_task))
        }
    };
    let (defs, base_schema) = output_schema_of(input_task.as_ref().unwrap_or(task));
    let (schema, _) = schema_at(&defs, &base_schema, path).map_err(|e| fail(e.to_string()))?;
    let schema = TypeSchema {
        kind: defs
            .resolve(&schema)
            .map_err(|e| fail(e.to_string()))?
            .kind
            .clone(),
        nullable: false,
    };
    let tokens = pointer_tokens(path).map_err(fail)?;
    let mut cur = base;
    for token in &tokens {
        let next = match cur {
            TypedValue::Object(m) => m.get(token),
            TypedValue::List(items) => token.parse::<usize>().ok().and_then(|i| items.get(i)),
            _ => None,
        };
        cur = next.ok_or_else(|| fail(format!("the output has no value at '{path}'")))?;
    }
    if matches!(cur, TypedValue::Null) {
        return Err(fail(format!("the output is null at '{path}'")));
    }
    let literal = defs
        .typed_value(&schema, literal)
        .map_err(|e| fail(e.to_string()))?;
    Ok((cur.clone(), vec![literal]))
}

fn compare(a: &TypedValue, b: &TypedValue) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (TypedValue::Int64(x), TypedValue::Int64(y)) => Some(x.cmp(y)),
        (TypedValue::Float64(x), TypedValue::Float64(y)) => x.as_f64()?.partial_cmp(&y.as_f64()?),
        (TypedValue::String(x), TypedValue::String(y)) => Some(if x == y {
            std::cmp::Ordering::Equal
        } else {
            x.cmp(y)
        }),
        (TypedValue::Bool(x), TypedValue::Bool(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

// ── 선택 여부 전파 판정 ─────────────────────────────────────────────────────

/// 경로 판정에서 본 선행 task 하나의 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EdgeStatus {
    /// 선행이 아직 끝나지 않았다.
    Pending,
    /// 선행이 성공했다(제어 엣지면 이 task 를 골랐다).
    Available,
    /// 선행이 이 task 를 고르지 않았거나, 선행 자신이 선택되지 않았다.
    NotSelected,
    /// 선행이 성공 결과를 내지 못했다.
    Unavailable { source: TaskId, state: String },
}

/// 선행 `source` 에서 받는 순서·데이터 엣지의 상태.
pub fn path_status(source: &Task) -> EdgeStatus {
    use super::TaskState::*;
    match &source.state {
        Succeeded => EdgeStatus::Available,
        Skipped if is_not_selected(source) => EdgeStatus::NotSelected,
        s if s.is_terminal() => EdgeStatus::Unavailable {
            source: source.id.clone(),
            state: s.name().to_string(),
        },
        _ => EdgeStatus::Pending,
    }
}

/// 전이를 가진 `source` 가 `target` 으로 보내는 제어 엣지의 상태.
pub fn control_status(source: &Task, target: &TaskId) -> EdgeStatus {
    match path_status(source) {
        EdgeStatus::Available => match &source.route {
            Some(r) if r.selected.contains(target) => EdgeStatus::Available,
            Some(_) => EdgeStatus::NotSelected,
            // 경로 기록 없이 성공한 레코드는 어느 쪽인지 알 수 없다.
            None => EdgeStatus::Unavailable {
                source: source.id.clone(),
                state: "succeeded without a route".into(),
            },
        },
        other => other,
    }
}

/// `tasks` 중 선택되지 않아 끝날 수 있는 task. 전이 대상이거나, 이미 선택되지 않았거나, 들어오는
/// 경로가 모두 그런 task 에서만 온다. 필수 입력을 이런 task 에서 받는 그래프를 제출 시 찾는다.
pub(crate) fn may_be_not_selected(tasks: &[&Task]) -> HashSet<TaskId> {
    let mut out: HashSet<TaskId> = HashSet::new();
    for t in tasks {
        for target in transition_targets(t) {
            out.insert(target.clone());
        }
        if is_not_selected(t) {
            out.insert(t.id.clone());
        }
    }
    loop {
        let before = out.len();
        for t in tasks {
            if out.contains(&t.id) || !t.is_typed() {
                continue;
            }
            let sources = activation_sources(t);
            if !sources.is_empty() && sources.iter().all(|s| out.contains(*s)) {
                out.insert(t.id.clone());
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

/// 순서·데이터로 들어오는 엣지의 source(제어 엣지 제외).
pub(crate) fn activation_sources(task: &Task) -> Vec<&TaskId> {
    let mut out: Vec<&TaskId> = task.depends_on.iter().collect();
    if let TaskCommand::Reduce { inputs, .. } = &task.command {
        out.extend(inputs.iter());
    }
    out.extend(super::binding_task_ids(task));
    out
}

/// 선택되지 않을 수 있는 source 의 출력을 필수 입력으로 읽는 task 를 거절한다. 그 source 가
/// 선택되지 않아도 이 task 가 다른 경로로 실행될 수 있으면 값이 없기 때문이다. 이 task 로
/// 들어오는 경로가 모두 그 source 에서 오면(source 가 선택될 때만 실행되면) 받는다. 대안 경로의
/// 값은 `one_of` 로, 없어도 되는 값은 optional·default 로 명시한다. `graph` 는 제출한 task,
/// `all` 은 기존 task 를 포함한 전체다.
pub(crate) fn check_route_inputs(graph: &[Task], all: &[&Task]) -> Result<(), TaskFailure> {
    let maybe = may_be_not_selected(all);
    if maybe.is_empty() {
        return Ok(());
    }
    for (i, t) in graph.iter().enumerate() {
        let Some(c) = &t.contract else { continue };
        let mut sources: Vec<&TaskId> = activation_sources(t);
        sources.extend(
            all.iter()
                .filter(|s| transition_targets(s).contains(&&t.id))
                .map(|s| &s.id),
        );
        for (field, b) in &c.bindings {
            let super::InputBinding::FromTask { source, .. } = b else {
                continue;
            };
            if !maybe.contains(&source.from_task)
                || super::binding::field_may_be_absent(c, field)
                || sources.iter().all(|s| **s == source.from_task)
            {
                continue;
            }
            let mut f = route_error(
                format!(
                    "task {}: input field '{field}' requires the output of {}, which may not be \
                     selected while {} still runs through another path; bind it with one_of, or \
                     make the field optional or give it a default",
                    t.id, source.from_task, t.id
                ),
                format!(
                    "/tasks/{i}/bindings/{}",
                    field.replace('~', "~0").replace('/', "~1")
                ),
            );
            f.task_id = Some(t.id.clone());
            return Err(f);
        }
    }
    Ok(())
}
