//! 백그라운드 작업을 기다리는 자식이 기다리는 작업의 출력 파일을 찾아 활동을 관측한다.
//!
//! Claude Code 는 백그라운드 작업의 출력을 `<임시 폴더>/claude-<uid>/<cwd slug>/<session_id>/tasks/<id>.output`
//! 에 쓴다. 임시 폴더는 `CLAUDE_CODE_TMPDIR` 이 있으면 그 값, 없으면 OS 임시 폴더다. slug 는 cwd 의
//! 영숫자가 아닌 문자를 `-` 로 바꾼 값이다. 서브에이전트의 `.output` 은
//! `~/.claude/projects/<slug>/<session_id>/subagents/agent-<id>.jsonl` 을 가리키는 심볼릭 링크다.
//! 공식 문서에 없는 내부 경로이며 Claude Code 2.1.291 의 코드와 실제 폴더로 확인했다. 버전에 따라 바뀔 수 있어,
//! 찾지 못하면 호출자가 기존 출력 정지 기준으로 돌아간다.
//!
//! 이 플러그인은 자식 Claude 의 환경 변수를 읽지 못한다. 그래서 플러그인 프로세스의 `CLAUDE_CODE_TMPDIR`·임시 폴더
//! 아래의 `claude-*` 폴더에서 `<session_id>/tasks` 를 찾는다. session id 는 UUID 라 다른 세션과 겹치지 않는다.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::Value;

/// 정지 알림 직전에 기록하는 조용한 작업 정보. notify-error 가 대기 시작 시각이 같을 때만 읽는다.
pub(crate) const BACKGROUND_QUIET_META_KEY: &str = "claude-background-quiet";

/// slug 가 이 길이를 넘으면 Claude Code 가 해시를 붙인다. 해시 규칙은 따라 하지 않고 폴더 탐색으로 찾는다.
const SLUG_MAX: usize = 200;

/// 기다리는 작업 하나.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct WatchedTask {
    pub id: String,
    /// 알림에 적을 이름. Stop 의 `description`·`command`, 없으면 id 다.
    pub label: String,
}

/// 대기 한 번에서 관측할 작업과 그 출력 파일을 찾을 단서.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct TaskWatch {
    pub session_id: String,
    pub cwd: String,
    pub tasks: Vec<WatchedTask>,
}

impl TaskWatch {
    /// 대기 Stop 의 payload 에서 끝나지 않은 작업을 모은다. 세션·cwd·작업이 없으면 `None` 이다.
    pub fn from_stop(params: &Value) -> Option<Self> {
        let parsed;
        let list = match params.get("background_tasks")? {
            Value::String(raw) => {
                parsed = serde_json::from_str::<Value>(raw).ok()?;
                &parsed
            }
            other => other,
        };
        let tasks = list
            .as_array()?
            .iter()
            .filter(|t| !crate::hook::is_finished_task(t))
            .filter_map(|t| {
                let id = t.get("id").and_then(Value::as_str)?.to_string();
                let label = ["description", "command"]
                    .iter()
                    .find_map(|k| t.get(*k).and_then(Value::as_str).filter(|s| !s.is_empty()))
                    .unwrap_or(&id)
                    .to_string();
                Some(WatchedTask { id, label })
            })
            .collect();
        Self::new(params, tasks)
    }

    /// 플러그인이 기록한 작업 id 로 만든다(StopFailure 경로). 이름은 id 다.
    pub fn from_ids(params: &Value, ids: Vec<String>) -> Option<Self> {
        let tasks = ids
            .into_iter()
            .map(|id| WatchedTask {
                label: id.clone(),
                id,
            })
            .collect();
        Self::new(params, tasks)
    }

    fn new(params: &Value, tasks: Vec<WatchedTask>) -> Option<Self> {
        let text = |k: &str| {
            params
                .get(k)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        if tasks.is_empty() {
            return None;
        }
        Some(Self {
            session_id: text("session")?,
            cwd: text("cwd")?,
            tasks,
        })
    }
}

/// Claude Code 의 프로젝트 폴더 이름 규칙. JavaScript 의 `/[^a-zA-Z0-9]/g` 는 UTF-16 단위로 바꾸므로
/// BMP 밖 문자는 `-` 두 개가 된다.
fn slug(cwd: &str) -> String {
    cwd.chars()
        .flat_map(|c| {
            let n = if c.is_ascii_alphanumeric() {
                0
            } else {
                c.len_utf16()
            };
            std::iter::repeat_n('-', n).chain((n == 0).then_some(c))
        })
        .collect()
}

/// `claude-*` 폴더를 찾을 기준 폴더. 플러그인 프로세스의 값이며 자식의 값과 다를 수 있다.
fn temp_roots() -> Vec<PathBuf> {
    let base = std::env::var_os("CLAUDE_CODE_TMPDIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let mut roots = vec![base.clone()];
    if let Ok(real) = base.canonicalize()
        && real != base
    {
        roots.push(real);
    }
    roots
}

/// 이 세션의 `tasks` 폴더. 없으면 `None` 이다.
pub(crate) fn tasks_dir(watch: &TaskWatch) -> Option<PathBuf> {
    tasks_dir_in(&temp_roots(), watch)
}

fn tasks_dir_in(roots: &[PathBuf], watch: &TaskWatch) -> Option<PathBuf> {
    let slug = slug(&watch.cwd);
    for root in roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        let users = entries.flatten().filter(|e| {
            e.file_name().to_string_lossy().starts_with("claude-")
                && e.file_type().is_ok_and(|t| t.is_dir())
        });
        for user in users {
            let user = user.path();
            if slug.len() <= SLUG_MAX {
                let direct = user.join(&slug).join(&watch.session_id).join("tasks");
                if direct.is_dir() {
                    return Some(direct);
                }
            }
            // cwd 가 바뀐 세션이나 긴 slug 는 프로젝트 폴더를 훑어 session id 로 찾는다.
            let Ok(projects) = std::fs::read_dir(&user) else {
                continue;
            };
            if let Some(found) = projects
                .flatten()
                .map(|p| p.path().join(&watch.session_id).join("tasks"))
                .find(|p| p.is_dir())
            {
                return Some(found);
            }
        }
    }
    None
}

/// 출력 파일 하나의 관측값. 심볼릭 링크는 대상 파일을 본다.
#[derive(Clone, Copy, Debug, PartialEq)]
struct FileMark {
    len: u64,
    modified: Option<SystemTime>,
}

/// 서브에이전트의 링크는 대상 transcript 가 생기기 전에 만들어질 수 있다. 그때는 링크 자체를 크기 0 으로 본다.
fn mark(path: &Path) -> Option<FileMark> {
    let Ok(m) = std::fs::metadata(path) else {
        let link = std::fs::symlink_metadata(path).ok()?;
        return Some(FileMark {
            len: 0,
            modified: link.modified().ok(),
        });
    };
    Some(FileMark {
        len: m.len(),
        modified: m.modified().ok(),
    })
}

/// 대기 한 번의 작업 파일 관측 상태.
#[derive(Debug)]
pub(crate) struct FileActivity {
    files: Vec<(WatchedTask, PathBuf, Option<FileMark>)>,
    /// 마지막으로 크기나 수정 시각이 바뀐 것을 본 시각.
    last_activity: SystemTime,
}

impl FileActivity {
    /// 출력 파일을 찾는다. 기다리는 작업의 파일이 하나도 없으면 `None` 이다.
    pub fn locate(watch: &TaskWatch, dir: &Path, now: SystemTime) -> Option<Self> {
        let files: Vec<_> = watch
            .tasks
            .iter()
            .map(|t| {
                let path = dir.join(format!("{}.output", t.id));
                let m = mark(&path);
                (t.clone(), path, m)
            })
            .filter(|(_, _, m)| m.is_some())
            .collect();
        if files.is_empty() {
            return None;
        }
        // 처음 관측은 파일의 마지막 수정 시각부터 조용한 것으로 본다.
        let last_activity = files
            .iter()
            .filter_map(|(_, _, m)| m.and_then(|m| m.modified))
            .max()
            .unwrap_or(now);
        Some(Self {
            files,
            last_activity,
        })
    }

    /// 파일을 다시 보고, 모든 대상이 활동 없이 지난 시간을 돌려준다. 크기나 수정 시각이 바뀌면 활동이다.
    pub fn poll(&mut self, now: SystemTime) -> Duration {
        for (_, path, last) in &mut self.files {
            let current = mark(path);
            if current != *last {
                let at = current
                    .and_then(|m| m.modified)
                    .filter(|t| *t <= now)
                    .map_or(now, |t| t.max(self.last_activity));
                // 크기만 바뀌고 수정 시각 해상도가 거칠면 지금을 활동 시각으로 본다.
                self.last_activity = if current.map(|m| m.len) != last.map(|m| m.len) {
                    now
                } else {
                    at
                };
                *last = current;
            }
        }
        now.duration_since(self.last_activity).unwrap_or_default()
    }

    /// 알림에 적을 작업 이름.
    pub fn labels(&self) -> Vec<String> {
        self.files.iter().map(|(t, _, _)| t.label.clone()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn watch(cwd: &str) -> TaskWatch {
        TaskWatch {
            session_id: "86612f01-61a6-4be8-b272-d960acd62aa5".into(),
            cwd: cwd.into(),
            tasks: vec![WatchedTask {
                id: "btwzfqgkg".into(),
                label: "sleep 300".into(),
            }],
        }
    }

    /// Claude Code 2.1.291 이 실제로 만든 폴더 이름과 같은 규칙이다(점으로 시작하는 폴더는 `--`).
    #[test]
    fn the_slug_replaces_every_non_alphanumeric_character() {
        assert_eq!(
            slug("/home/user/repo/.hidden-dir/temp/probe/cwd"),
            "-home-user-repo--hidden-dir-temp-probe-cwd"
        );
        assert_eq!(slug("/a/한/\u{10437}"), "-a-----");
    }

    #[test]
    fn a_stop_payload_gives_the_unfinished_tasks_with_their_names() {
        let params = json!({
            "session": "s-1", "cwd": "/w",
            "background_tasks": [
                {"id":"btwzfqgkg","type":"shell","status":"running","description":"tick loop","command":"for i in 1 2 3; do echo; done"},
                {"id":"a22d7fc9077e1353c","type":"subagent","status":"running"},
                {"id":"done1","type":"shell","status":"completed","description":"x"}
            ]
        });
        let w = TaskWatch::from_stop(&params).expect("작업 있음");
        assert_eq!(w.session_id, "s-1");
        let got: Vec<_> = w
            .tasks
            .iter()
            .map(|t| (t.id.as_str(), t.label.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("btwzfqgkg", "tick loop"),
                ("a22d7fc9077e1353c", "a22d7fc9077e1353c")
            ]
        );
        assert_eq!(
            TaskWatch::from_stop(&json!({"session":"s","background_tasks":[]})),
            None
        );
        assert_eq!(
            TaskWatch::from_stop(&json!({"cwd":"/w","background_tasks":[{"id":"b"}]})),
            None
        );
    }

    #[test]
    fn the_tasks_folder_is_found_by_slug_or_by_session_id() {
        let root = tempfile::tempdir().unwrap();
        let w = watch("/home/u/proj.x");
        assert_eq!(tasks_dir_in(&[root.path().into()], &w), None);
        let direct = root
            .path()
            .join("claude-1000/-home-u-proj-x")
            .join(&w.session_id)
            .join("tasks");
        std::fs::create_dir_all(&direct).unwrap();
        assert_eq!(
            tasks_dir_in(&[root.path().into()], &w),
            Some(direct.clone())
        );
        // 세션 중 cwd 를 옮겨도 session id 로 찾는다.
        let moved = watch("/elsewhere");
        assert_eq!(tasks_dir_in(&[root.path().into()], &moved), Some(direct));
    }

    #[test]
    fn growing_files_are_active_and_unchanged_files_grow_quiet() {
        let dir = tempfile::tempdir().unwrap();
        let w = watch("/w");
        let path = dir.path().join("btwzfqgkg.output");
        let now = SystemTime::now();
        assert!(FileActivity::locate(&w, dir.path(), now).is_none());
        std::fs::write(&path, "tick 1\n").unwrap();
        let mut a = FileActivity::locate(&w, dir.path(), now).expect("파일 있음");
        assert_eq!(a.labels(), vec!["sleep 300"]);
        let later = now + Duration::from_secs(130);
        assert!(a.poll(later) >= Duration::from_secs(129));
        std::fs::write(&path, "tick 1\ntick 2\n").unwrap();
        assert_eq!(a.poll(later), Duration::ZERO);
        assert_eq!(
            a.poll(later + Duration::from_secs(30)),
            Duration::from_secs(30)
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_subagent_link_is_followed_to_its_transcript() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = dir.path().join("agent-a1.jsonl");
        std::fs::write(&transcript, "{}\n").unwrap();
        std::os::unix::fs::symlink(&transcript, dir.path().join("a1.output")).unwrap();
        let mut w = watch("/w");
        w.tasks[0].id = "a1".into();
        let now = SystemTime::now() + Duration::from_secs(200);
        let pending = dir.path().join("pending.output");
        std::os::unix::fs::symlink(dir.path().join("agent-pending.jsonl"), &pending).unwrap();
        let mut early = w.clone();
        early.tasks[0].id = "pending".into();
        let mut e = FileActivity::locate(&early, dir.path(), now).expect("끊긴 링크도 찾음");
        std::fs::write(dir.path().join("agent-pending.jsonl"), "{}\n").unwrap();
        assert_eq!(
            e.poll(now),
            Duration::ZERO,
            "대상 transcript 가 생기면 활동이다"
        );
        let mut a = FileActivity::locate(&w, dir.path(), now).expect("링크 대상 있음");
        assert!(a.poll(now) >= Duration::from_secs(150));
        std::fs::write(&transcript, "{}\n{}\n").unwrap();
        assert_eq!(a.poll(now), Duration::ZERO);
    }
}
