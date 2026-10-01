//! 생성 검증과 실행 시 치환이 공유하는 ${task.<id>.output<JSON Pointer>} 파서.
//! 빈 포인터는 출력 전체다. ${task.로 시작한 잘못된 표식은 일반 문자열로 넘기지 않고 오류로 반환한다.
//! ID는 첫 점까지 읽으며 저장소의 ID 발급 형식까지 검증하지는 않는다.
//! 포인터는 빈 값 또는 / 접두사만 확인하고 실제 값 조회는 serde_json::Value::pointer가 처리한다.

use std::collections::BTreeSet;
use std::ops::Range;

use tasty_agent::{TaskCommand, TaskId};

const OPEN: &str = "${task.";
const OUTPUT_MARKER: &str = "output";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TaskOutputRef {
    pub(crate) task_id: TaskId,
    /// 빈 문자열이면 결과 전체를 가리킨다.
    pub(crate) pointer: String,
}

/// 생성 시 invalid_params, 실행 시 PermanentFail로 사용자에게 전달할 오류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ParseError(pub(crate) String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// 원문 바이트 범위도 반환해 문자열 전체를 차지한 표식의 타입 보존 치환에 사용한다.
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
        let end = body_start + close_rel + 1;
        let inner = &s[body_start..body_start + close_rel];
        out.push((start..end, parse_inner(inner)?));
        from = end;
    }
    Ok(out)
}

fn parse_inner(inner: &str) -> Result<TaskOutputRef, ParseError> {
    let malformed = || {
        ParseError(format!(
            "malformed task output placeholder '${{task.{inner}}}' \
             (expected '${{task.<task_id>.output<json_pointer>}}', \
             e.g. '${{task.t-1716-000007.output/child_surface_id}}')"
        ))
    };
    let (task_id, rest) = inner.split_once('.').ok_or_else(malformed)?;
    if task_id.is_empty() {
        return Err(malformed());
    }
    let pointer = rest.strip_prefix(OUTPUT_MARKER).ok_or_else(malformed)?;
    if !pointer.is_empty() && !pointer.starts_with('/') {
        return Err(malformed());
    }
    Ok(TaskOutputRef {
        task_id: task_id.to_string(),
        pointer: pointer.to_string(),
    })
}

/// 생성 검증에서 참조 작업을 모은다. 여기의 순회와 runner_host의 별도 치환 순회를 같은 범위로 유지해야 한다.
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

/// Run 인자·UTF-8 cwd와 Custom JSON의 문자열 값을 방문한다. Reduce·WaitBarrier는 제외한다.
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
