//! `${task.<id>.output<JSON Pointer>}` placeholder 문법의 **단일 소유자**.
//!
//! 이 문법은 두 곳에서 해석된다 — 생성 시점 검증(`agent.task_create` handler)과
//! dispatch 시점 치환(`runner_host`). 문법을 양쪽에 복제하면 "생성은 통과했는데
//! 치환이 못 알아본다"(또는 그 반대) 가 조용히 생기므로, 파싱은 여기 한 곳에만 둔다.
//!
//! ## 문법
//!
//! ```text
//! ${task.<task_id>.output<pointer>}
//! ```
//!
//! - `<task_id>`: `t-<ms>-<seq>` 형식(`crates/tasty-agent/src/task/store.rs`
//!   `new_id`). `.` 을 포함하지 않으므로 첫 `.` 이 곧 id 의 끝이라 파싱이 모호하지
//!   않다.
//! - `<pointer>`: RFC 6901 JSON Pointer. `/` 로 시작하므로 구분자 없이 이어 붙는다
//!   (`${task.t-1-000001.output/child_surface_id}`). **빈 문자열도 유효**하며
//!   (`${task.t-1-000001.output}`) 그때는 출력 전체를 가리킨다 — RFC 6901 의 빈
//!   포인터 의미와 같고 `serde_json::Value::pointer("")` 가 그대로 구현한다.
//!   추출 API 는 reducer 의 `--extract-path`(`crates/tasty-agent/src/reducer.rs`)
//!   와 **같은 `Value::pointer`** 다 — 경로 문법을 두 개 외우게 하지 않는다.
//!
//! ## 오문법은 무시하지 않고 거부한다
//!
//! `${task.` 로 시작했는데 문법에 안 맞으면 "그냥 리터럴 문자열" 로 넘기지 않고
//! [`ParseError`] 를 낸다. 이 접두사로 시작하는 문자열이 우연히 등장할 일은 거의
//! 없고, 대부분 이 문법의 오타이기 때문이다 — 조용히 리터럴로 흘리면 프롬프트에
//! `${task.t-x.ouput/id}` 가 그대로 박혀 나가고, 그 사실은 한참 뒤에야 드러난다.

use std::collections::BTreeSet;
use std::ops::Range;

use tasty_agent::{TaskCommand, TaskId};

/// placeholder 여는 토큰. 뒤에 `<id>.output<pointer>}` 가 이어진다.
const OPEN: &str = "${task.";
/// task id 와 pointer 사이의 고정 마커.
const OUTPUT_MARKER: &str = "output";

/// 파싱된 참조 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskOutputRef {
    /// 참조 대상 task id.
    pub(crate) task_id: TaskId,
    /// RFC 6901 JSON Pointer. 빈 문자열이면 출력 전체.
    pub(crate) pointer: String,
}

/// 문법 위반. 메시지는 그대로 사용자에게 노출된다(생성 시 `invalid_params`,
/// dispatch 시 `PermanentFail`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseError(pub(crate) String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 문자열 안의 placeholder 를 왼쪽부터 전부 찾는다.
///
/// 반환은 `(원문 바이트 범위, 참조)` 목록 — 범위를 함께 주는 이유는 호출자가
/// "문자열 전체가 정확히 placeholder 하나인가" 를 판정해 **타입 보존 치환**을
/// 할 수 있어야 하기 때문이다(그 판정이 이 기능의 핵심이다).
pub(crate) fn parse_refs(s: &str) -> Result<Vec<(Range<usize>, TaskOutputRef)>, ParseError> {
    let mut out = Vec::new();
    let mut from = 0usize;
    while let Some(rel) = s[from..].find(OPEN) {
        let start = from + rel;
        let body_start = start + OPEN.len();
        let Some(close_rel) = s[body_start..].find('}') else {
            return Err(ParseError(format!(
                "unterminated task output placeholder (missing '}}'): {}",
                &s[start..]
            )));
        };
        let end = body_start + close_rel + 1; // '}' 포함
        let inner = &s[body_start..body_start + close_rel];
        out.push((start..end, parse_inner(inner)?));
        from = end;
    }
    Ok(out)
}

/// `${task.` 와 `}` 사이(`<id>.output<pointer>`)를 뜯는다.
fn parse_inner(inner: &str) -> Result<TaskOutputRef, ParseError> {
    let malformed = || {
        ParseError(format!(
            "malformed task output placeholder '${{task.{inner}}}' \
             (expected '${{task.<task_id>.output<json_pointer>}}', \
             e.g. '${{task.t-1716-000007.output/child_surface_id}}')"
        ))
    };
    // task id 는 `.` 을 포함하지 않으므로 첫 `.` 이 경계다.
    let (task_id, rest) = inner.split_once('.').ok_or_else(malformed)?;
    if task_id.is_empty() {
        return Err(malformed());
    }
    let pointer = rest.strip_prefix(OUTPUT_MARKER).ok_or_else(malformed)?;
    // 빈 포인터(출력 전체)이거나 `/` 로 시작해야 한다 — RFC 6901.
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(malformed());
    }
    Ok(TaskOutputRef {
        task_id: task_id.to_string(),
        pointer: pointer.to_string(),
    })
}

/// command 가 참조하는 task id 집합.
///
/// 순회 범위는 치환이 실제로 적용되는 자리와 **같아야 한다** — 검증이 더 넓으면
/// 쓰지도 않을 의존을 강요하고, 더 좁으면 검증을 빠져나간 참조가 dispatch 에서
/// 터진다. 두 순회는 [`for_each_template`] 하나를 공유해 갈라지지 않는다.
pub(crate) fn referenced_tasks(command: &TaskCommand) -> Result<BTreeSet<TaskId>, ParseError> {
    let mut ids = BTreeSet::new();
    let mut err = None;
    for_each_template(command, &mut |s| {
        if err.is_some() {
            return;
        }
        match parse_refs(s) {
            Ok(refs) => ids.extend(refs.into_iter().map(|(_, r)| r.task_id)),
            Err(e) => err = Some(e),
        }
    });
    match err {
        Some(e) => Err(e),
        None => Ok(ids),
    }
}

/// 치환 대상이 되는 모든 문자열 자리를 방문한다(읽기 전용).
///
/// - `Run.command` 의 각 인자 / `Run.cwd`
/// - `Custom.params` JSON 트리의 모든 문자열 값
/// - `Reduce`/`WaitBarrier`: 치환 대상 없음
pub(crate) fn for_each_template(command: &TaskCommand, f: &mut impl FnMut(&str)) {
    match command {
        TaskCommand::Run { command, cwd, .. } => {
            for arg in command {
                f(arg);
            }
            if let Some(p) = cwd
                && let Some(s) = p.to_str()
            {
                f(s);
            }
        }
        TaskCommand::Custom { params, .. } => visit_json(params, f),
        TaskCommand::Reduce { .. } | TaskCommand::WaitBarrier { .. } => {}
    }
}

fn visit_json(value: &serde_json::Value, f: &mut impl FnMut(&str)) {
    match value {
        serde_json::Value::String(s) => f(s),
        serde_json::Value::Array(arr) => {
            for v in arr {
                visit_json(v, f);
            }
        }
        serde_json::Value::Object(map) => {
            for v in map.values() {
                visit_json(v, f);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(id: &str, ptr: &str) -> TaskOutputRef {
        TaskOutputRef {
            task_id: id.to_string(),
            pointer: ptr.to_string(),
        }
    }

    #[test]
    fn parses_pointer_form() {
        let src = "${task.t-1716-000007.output/child_surface_id}";
        let refs = parse_refs(src).unwrap();
        assert_eq!(refs.len(), 1);
        // 범위가 문자열 전체여야 타입 보존 치환(통째 교체) 조건이 성립한다.
        assert_eq!(refs[0].0, 0..src.len());
        assert_eq!(refs[0].1, r("t-1716-000007", "/child_surface_id"));
    }

    #[test]
    fn empty_pointer_means_whole_output() {
        let refs = parse_refs("${task.t-1-000001.output}").unwrap();
        assert_eq!(refs[0].1, r("t-1-000001", ""));
    }

    #[test]
    fn parses_nested_pointer_and_multiple_refs() {
        let refs = parse_refs("a ${task.t-a.output/x/0/y} b ${task.t-b.output} c").unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].1, r("t-a", "/x/0/y"));
        assert_eq!(refs[1].1, r("t-b", ""));
        // 범위가 정확해야 부분 보간이 원문을 안 망친다.
        assert_eq!(
            &"a ${task.t-a.output/x/0/y} b ${task.t-b.output} c"[refs[0].0.clone()],
            "${task.t-a.output/x/0/y}"
        );
    }

    #[test]
    fn text_without_placeholder_yields_nothing() {
        assert!(
            parse_refs("plain text with ${lease.resource}")
                .unwrap()
                .is_empty()
        );
        assert!(parse_refs("").unwrap().is_empty());
    }

    #[test]
    fn typo_in_marker_is_rejected_not_silently_passed_through() {
        // `ouput` 오타 — 조용히 리터럴로 흘리면 프롬프트에 그대로 박혀 나간다.
        let e = parse_refs("${task.t-a.ouput/id}").unwrap_err();
        assert!(e.0.contains("malformed"), "{}", e.0);
    }

    #[test]
    fn missing_marker_and_pointer_without_slash_are_rejected() {
        assert!(parse_refs("${task.t-a}").is_err());
        assert!(parse_refs("${task.t-a.outputchild}").is_err());
        assert!(parse_refs("${task..output/x}").is_err());
    }

    #[test]
    fn unterminated_placeholder_is_rejected() {
        let e = parse_refs("${task.t-a.output/id").unwrap_err();
        assert!(e.0.contains("unterminated"), "{}", e.0);
    }

    #[test]
    fn referenced_tasks_covers_run_and_custom_shapes() {
        let run = TaskCommand::Run {
            command: vec!["echo".into(), "${task.t-a.output/msg}".into()],
            workspace_id: 1,
            cwd: Some("/tmp/${task.t-b.output/dir}".into()),
        };
        assert_eq!(
            referenced_tasks(&run).unwrap(),
            ["t-a".to_string(), "t-b".to_string()].into_iter().collect()
        );

        let custom = TaskCommand::Custom {
            ipc_method: "claude.tell".into(),
            params: serde_json::json!({
                "surface_id": "${task.t-c.output/child_surface_id}",
                "nested": [{"deep": "${task.t-d.output}"}],
                "untouched": 5,
            }),
            poll: None,
        };
        assert_eq!(
            referenced_tasks(&custom).unwrap(),
            ["t-c".to_string(), "t-d".to_string()].into_iter().collect()
        );
    }

    #[test]
    fn referenced_tasks_is_empty_for_non_templating_commands() {
        let reduce = TaskCommand::Reduce {
            inputs: vec!["t-a".into()],
            strategy: tasty_agent::ReducerStrategy::ConcatText,
        };
        assert!(referenced_tasks(&reduce).unwrap().is_empty());
    }
}
