//! `surface_id` → 세션 id → transcript 파일 경로 해석.
//!
//! 체인은 host IPC 만으로 닫힌다 — 이 plugin 은 claude plugin 에 코드로 의존하지 않는다:
//!
//! 1. claude plugin 의 `SessionStart` 훅이 `claude-session-id` 를 **surface meta 로**
//!    기록해 둔다.
//! 2. 여기서 `surface.meta.get` 으로 그 값을 읽는다 (`surface.read` 권한).
//! 3. 그 값이 그대로 transcript 파일명이다 — `<root>/<project-slug>/<session-id>.jsonl`.
//!
//! **project-slug 는 계산하지 않고 탐색한다.** slug 는 cwd 절대경로의 구분자를 `-` 로
//! 바꾼 형태로 보이지만, `.` 를 포함한 경로의 처리 규칙을 관찰 샘플만으로 확정할 수
//! 없다. 규칙은 우리가 소유하지 않는 외부(Claude Code) 스펙이라 언제든 바뀔 수 있고,
//! 틀린 slug 는 "조용히 아무 것도 안 나옴" 으로 나타나 진단이 어렵다. 반면 파일명은
//! 세션 id 그대로라 **root 한 겹 아래를 훑어 `<session-id>.jsonl` 을 찾는** 방식이면
//! 규칙 추론 자체가 필요 없다.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, PluginError};

/// claude plugin 이 `session-start` 훅에서 기록하는 surface meta 키.
pub const CLAUDE_SESSION_META_KEY: &str = "claude-session-id";

/// 호스트 IPC 호출 추상화 — 단위 테스트가 스텁을 끼울 수 있게 trait 뒤에 둔다.
pub trait HostCall {
    fn call(&self, method: &str, params: Value) -> Result<Value, PluginError>;
}

impl HostCall for HostHandle {
    fn call(&self, method: &str, params: Value) -> Result<Value, PluginError> {
        HostHandle::call(self, method, params)
    }
}

/// 경로 해석이 실패하는 방식. 전부 **명시적 에러**다 — 조용한 무동작을 만들지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// 대상 surface 에 세션 id meta 가 없다.
    NoSessionMeta { surface_id: u32 },
    /// transcript 루트 디렉토리를 특정할 수 없다(홈 디렉토리 미확인 등).
    TranscriptRootMissing,
    /// 루트는 있으나 그 세션의 파일이 아직 없다.
    TranscriptNotFound { session_id: String },
    /// host IPC 호출 자체가 실패했다.
    HostCall { message: String },
}

/// 대상 surface 의 에이전트 세션 id 를 읽는다.
///
/// meta 가 없으면 [`ResolveError::NoSessionMeta`] — 세션 id 를 모르면 어떤 파일을 볼지
/// 결정할 수 없으므로 watch 등록을 거부해야 한다.
pub fn session_id_for_surface<H: HostCall>(
    host: &H,
    surface_id: u32,
) -> Result<String, ResolveError> {
    let resp = host
        .call(
            "surface.meta.get",
            json!({ "surface_id": surface_id, "key": CLAUDE_SESSION_META_KEY }),
        )
        .map_err(|e| ResolveError::HostCall {
            message: e.to_string(),
        })?;
    resp.get("value")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or(ResolveError::NoSessionMeta { surface_id })
}

/// 대상 surface 가 아직 존재하는지. 판정할 수 없으면 `true`(살아있다고 보수적으로 본다)
/// — 일시적인 IPC 실패로 watch 를 걷어내지 않기 위해서다.
/// 그 surface 가 살아 있는가.
///
/// **호스트는 없는 대상에 `{exists:false}` 로 답하지 않는다 — 요청 자체를 거절한다.**
/// 실측(2026-09-05, 격리 헤드리스 인스턴스):
///
/// ```text
/// surface.locate surface_id=1        → {"exists": true, "pane_id": 1, "surface_id": 1}
/// surface.locate surface_id=424242   → error -32602 "no live surface 424242 (named by
///                                       'surface.locate'); list the resource to get a live id …"
/// ```
///
/// 그래서 `Err` 를 통째로 "모름 → 살아 있다고 보자" 로 접으면 **이 함수는 false 를
/// 돌려줄 길이 없다.** 예전 구현이 그랬고, 그 결과 [`crate::pump::verify_one`] 의
/// "사라진 대상의 턴을 닫는다" 가 한 번도 발화하지 못했다 — 그 doc 이 약속하는 정리가
/// 코드에 없었던 것과 같다.
///
/// **두 종류의 `Err` 를 가른다.** 대상을 지목했는데 그런 대상이 없다는 거절은 **답**이다
/// (→ `false`). 그 외(연결 끊김·timeout·호스트 내부 오류)는 여전히 **모름**이라
/// `true` 로 떨어진다 — 살아 있는 대상을 없다고 말하지 않는 쪽으로 틀린다.
///
/// 판정을 문자열로 하는 이유: 규약의 정본은 [`tasty_utils::target`] 이고 판정도 거기
/// [`tasty_utils::target::says_no_live_target`] 에 있다 — 만드는 쪽과 붙여 두어야 문구가
/// 바뀔 때 왕복이 먼저 깨진다. 본체가 문구를 바꾸면 이 판정은 조용히 "모름" 쪽으로
/// 떨어진다(살아 있다고 답한다) — 좁게 틀리는 방향이라 안전하다.
///
/// [`tasty_plugin_sdk::PluginError::HostCall`] 은 이제 호스트가 준 코드도 함께 싣는다
/// (ADR-0171). 다만 그 코드로는 **이 판정을 못 한다** — `-32602` 는 "인자가 틀렸다" 이지
/// "그 대상이 없다" 가 아니고, 여기서 갈라야 하는 것은 "없다는 답" 과 "모름" 이다.
/// 코드가 그 구분을 실으려면 대상 부재 전용 코드가 있어야 하고, 그건 별개 결정이다.
pub fn surface_exists<H: HostCall>(host: &H, surface_id: u32) -> bool {
    match host.call("surface.locate", json!({ "surface_id": surface_id })) {
        Ok(r) => r.get("exists").and_then(Value::as_bool).unwrap_or(true),
        Err(e) => !rejects_as_no_live_surface(&e.to_string(), surface_id),
    }
}

/// 호스트가 "그런 surface 는 없다" 고 거절했는가. [`surface_exists`] 참조.
///
/// 순수 함수로 뽑아 두어 합성 문자열로 찌를 수 있다 — 호스트를 세우지 않고도 이
/// 판정의 두 방향을 고정할 수 있다.
fn rejects_as_no_live_surface(message: &str, surface_id: u32) -> bool {
    tasty_utils::target::says_no_live_target(message, "surface", u64::from(surface_id))
}

/// transcript 루트 디렉토리. `CLAUDE_CONFIG_DIR` 이 설정돼 있으면 그 아래 `projects`,
/// 아니면 홈의 `.claude/projects`.
pub fn transcript_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        return Some(PathBuf::from(dir).join("projects"));
    }
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    Some(home.join(".claude").join("projects"))
}

/// 루트 바로 아래 프로젝트 디렉토리들에서 `<session_id>.jsonl` 을 찾는다.
///
/// 여러 프로젝트 디렉토리에 같은 세션 id 가 있을 수는 없다(세션 id 는 전역 유일). 찾는
/// 즉시 반환하므로 디렉토리 수만큼의 `exists` 검사로 끝난다 — 전체 트리 순회가 아니다.
pub fn find_transcript(root: &Path, session_id: &str) -> Option<PathBuf> {
    let file_name = format!("{session_id}.jsonl");
    // 루트 바로 아래에 놓인 경우도 함께 본다(레이아웃이 바뀌어도 깨지지 않게).
    let direct = root.join(&file_name);
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(root).ok()?;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let candidate = entry.path().join(&file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 루트 탐색까지 묶은 편의 함수.
pub fn transcript_path(session_id: &str) -> Result<PathBuf, ResolveError> {
    let root = transcript_root().ok_or(ResolveError::TranscriptRootMissing)?;
    find_transcript(&root, session_id).ok_or_else(|| ResolveError::TranscriptNotFound {
        session_id: session_id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// 호출 기록을 남기는 host 스텁.
    struct StubHost {
        meta_value: Option<Value>,
        locate_exists: Option<bool>,
        fail: bool,
        calls: RefCell<Vec<String>>,
    }

    impl StubHost {
        fn with_meta(value: Option<&str>) -> Self {
            Self {
                meta_value: Some(json!({ "value": value })),
                locate_exists: None,
                fail: false,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl HostCall for StubHost {
        fn call(&self, method: &str, _params: Value) -> Result<Value, PluginError> {
            self.calls.borrow_mut().push(method.to_string());
            if self.fail {
                return Err(PluginError::HostClosed);
            }
            match method {
                "surface.meta.get" => Ok(self.meta_value.clone().unwrap_or(json!({}))),
                "surface.locate" => Ok(json!({ "exists": self.locate_exists.unwrap_or(true) })),
                other => panic!("unexpected host call {other}"),
            }
        }
    }

    #[test]
    fn reads_the_session_id_from_surface_meta() {
        let host = StubHost::with_meta(Some("5ff72b70-4a0a-4530-b3ca-ab4159b3ca24"));
        let id = session_id_for_surface(&host, 657).expect("session id");
        assert_eq!(id, "5ff72b70-4a0a-4530-b3ca-ab4159b3ca24");
        assert_eq!(host.calls.borrow().as_slice(), ["surface.meta.get"]);
    }

    #[test]
    fn missing_meta_is_an_explicit_error() {
        let host = StubHost::with_meta(None);
        assert_eq!(
            session_id_for_surface(&host, 42),
            Err(ResolveError::NoSessionMeta { surface_id: 42 })
        );
    }

    #[test]
    fn empty_meta_is_treated_as_missing() {
        let host = StubHost::with_meta(Some(""));
        assert_eq!(
            session_id_for_surface(&host, 42),
            Err(ResolveError::NoSessionMeta { surface_id: 42 })
        );
    }

    #[test]
    fn host_failure_is_reported_as_such_not_as_missing_meta() {
        let mut host = StubHost::with_meta(Some("s"));
        host.fail = true;
        match session_id_for_surface(&host, 1) {
            Err(ResolveError::HostCall { .. }) => {}
            other => panic!("expected HostCall error, got {other:?}"),
        }
    }

    #[test]
    fn liveness_defaults_to_alive_when_the_host_cannot_answer() {
        let mut host = StubHost::with_meta(None);
        host.fail = true;
        assert!(surface_exists(&host, 1));
    }

    #[test]
    fn liveness_reports_a_closed_surface() {
        let mut host = StubHost::with_meta(None);
        host.locate_exists = Some(false);
        assert!(!surface_exists(&host, 1));
    }

    #[test]
    fn finds_the_transcript_by_scanning_project_dirs_without_computing_the_slug() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        // slug 규칙을 흉내 낼 수 없는 이름들 — 탐색 방식이라 상관없다.
        std::fs::create_dir_all(root.join("-home-user-a")).expect("mkdir");
        std::fs::create_dir_all(root.join("-home-user-b-worktree-wt-1")).expect("mkdir");
        let wanted = root.join("-home-user-b-worktree-wt-1").join("sess-1.jsonl");
        std::fs::write(&wanted, b"{}\n").expect("write");

        assert_eq!(find_transcript(root, "sess-1"), Some(wanted));
    }

    #[test]
    fn finds_a_transcript_placed_directly_under_the_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wanted = dir.path().join("sess-2.jsonl");
        std::fs::write(&wanted, b"{}\n").expect("write");
        assert_eq!(find_transcript(dir.path(), "sess-2"), Some(wanted));
    }

    #[test]
    fn returns_none_when_the_session_file_does_not_exist_yet() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("-proj")).expect("mkdir");
        assert_eq!(find_transcript(dir.path(), "not-written-yet"), None);
    }

    #[test]
    fn a_directory_named_like_the_transcript_is_not_accepted() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir_all(dir.path().join("-proj").join("sess-3.jsonl")).expect("mkdir");
        assert_eq!(find_transcript(dir.path(), "sess-3"), None);
    }
    // ─── 호스트의 "그런 대상 없다" 거절을 답으로 읽는가 ────────────────────

    /// 실측한 호스트 문구를 그대로 쓴다(2026-09-05, `surface.locate surface_id=424242`).
    const HOST_REJECTION: &str = "host call 'surface.locate' failed: no live surface 424242 \
         (named by 'surface.locate'); list the resource to get a live id — a named target is \
         never resolved by focus";

    /// 지목한 대상이 없다는 거절은 **답**이다 — "모름" 이 아니다.
    #[test]
    fn a_no_live_surface_rejection_is_read_as_absence() {
        assert!(
            rejects_as_no_live_surface(HOST_REJECTION, 424_242),
            "이 문구를 못 읽으면 `surface_exists` 는 false 를 돌려줄 길이 없다"
        );
    }

    /// **양방향** — 그 외의 실패는 여전히 "모름" 이라 살아 있다고 본다.
    ///
    /// 이쪽이 무너지면 연결이 한 번 끊긴 것만으로 살아 있는 대상을 없다고 답하게 된다.
    #[test]
    fn other_host_failures_are_not_read_as_absence() {
        for msg in [
            "host call 'surface.locate' timed out after 5s",
            "io: connection reset by peer",
            "host call 'surface.locate' failed: internal error",
            // 다른 surface 를 지목한 거절은 이 surface 의 답이 아니다.
            "host call 'surface.locate' failed: no live surface 999 (named by 'surface.locate')",
        ] {
            assert!(
                !rejects_as_no_live_surface(msg, 424_242),
                "모름을 부재로 읽으면 안 된다: {msg}"
            );
        }
    }
}
