//! Claude 세션 프로필 레지스트리 — 이름으로 등록해 둔 `settings.json` 조각을
//! 세션(자기 자신 또는 자식)에 부착한다.
//!
//! `src/hook_handler/registry.rs` 의 형태(patch semantics · priority ↑ → owner
//! tie-break → id 정렬 · `<owner>/<short>` id)를 **미러링**한다 — 타입은
//! 공유하지 않는다(이 레포의 확립된 방식, TODO 문서 "레지스트리 선례" 절 참고).
//!
//! 실질 2 출처: host(내장 훅 6종을 조회 전용 항목으로 나열 — `install::MANAGED_HOOKS`
//! 가 유일한 정의처, 여기서 재정의하지 않는다) + user(사용자가 등록한 프로필의
//! 실체 JSON). plugin manifest 출처는 소비자가 이 plugin 하나뿐이라 비어 있다.
//! host/user 는 id 네임스페이스가 겹치지 않으므로(`host/*` vs `user/*`)
//! patch-fold 는 실제로는 단일 contribution 병합으로만 동작하지만, 형태는
//! 3출처 병합 코드와 동일하게 유지해 나중에 세 번째 출처가 필요해져도 재설계가
//! 없도록 한다.
//!
//! 이 plugin 은 단일 스레드 IPC 디스패치(`ClaudePlugin::handle_ipc_method`)만
//! 레지스트리를 건드리므로 호스트 레지스트리들과 달리 `RwLock`/`OnceLock`
//! 프로세스 전역 싱글턴이 불필요하다 — `ClaudePlugin` 의 plain 필드로 충분하다
//! (`state.rs` 의 `ClaudeState` 와 동일 근거: 별도 스레드가 건드리지 않는다).
//!
//! ## 저장 위치 (결정 3)
//!
//! `TASTY_PLUGIN_DATA_DIR` 하위, 사용자 원본과 tasty 생성 산출물을 분리:
//! - `profiles/registered/<short-name>.json` — 등록 시 호출자가 준 파일의 복사본
//!   (원본이 나중에 옮겨지거나 지워져도 레지스트리는 안전). 실제 attachable 항목의
//!   유일한 소스.
//! - `profiles/generated/<sorted-names>.json` — 프로필 조합을 부착할 때마다
//!   재생성되는 머지 산출물. 캐시가 아니라 항상 최신 등록 내용을 반영하도록
//!   매 attach 시점에 다시 쓴다.
//!
//! `data_dir` 이 `None`(호스트가 주입하지 않은 비정상 기동)이면 등록/부착 모두
//! 명시적 에러로 거부한다 — 조용히 다른 경로에 쓰지 않는다(`~/.claude/` 나
//! 새 경로를 발명하지 않는다는 결정 3 그대로).

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use tasty_plugin_sdk::{HostHandle, IpcMethodError};
use tracing::warn;

use crate::install::MANAGED_HOOKS;
use crate::profile_merge::{MergeError, merge_contents};

/// 프로필 short-name 규칙 — `hook_handler` short-name 규칙과 동일한 형태를
/// 미러링(소문자/숫자/하이픈, 최대 32자). 파일명으로도 그대로 쓰이므로 경로
/// traversal 문자(`/`, `..`)를 원천적으로 배제한다.
fn is_valid_short_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileError {
    /// 호스트가 `TASTY_PLUGIN_DATA_DIR` 를 주입하지 않은 비정상 기동.
    NoDataDir,
    InvalidShortName(String),
    /// 이름 목록이 비어 있음(`--profile ""` 등).
    EmptyNames,
    UnknownProfile(String),
    /// 조회 전용 항목(내장 훅)은 이름으로 부착할 수 없다.
    NotAttachable(String),
    SourceNotReadable {
        path: String,
        message: String,
    },
    SourceNotJsonObject {
        path: String,
    },
    Merge(MergeError),
    Io {
        path: String,
        message: String,
    },
}

impl std::fmt::Display for ProfileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoDataDir => write!(
                f,
                "plugin data directory not available (host did not inject TASTY_PLUGIN_DATA_DIR) — cannot register or attach profiles"
            ),
            Self::InvalidShortName(s) => write!(
                f,
                "'{s}' is not a valid profile name (lowercase letters, digits, '-' only, max 32 chars)"
            ),
            Self::EmptyNames => write!(f, "no profile name given"),
            Self::UnknownProfile(id) => write!(f, "no registered profile named '{id}'"),
            Self::NotAttachable(id) => write!(
                f,
                "'{id}' is a built-in listing entry and cannot be attached by name"
            ),
            Self::SourceNotReadable { path, message } => {
                write!(f, "cannot read profile source '{path}': {message}")
            }
            Self::SourceNotJsonObject { path } => {
                write!(f, "profile source '{path}' is not a JSON object")
            }
            Self::Merge(e) => write!(f, "{e}"),
            Self::Io { path, message } => write!(f, "'{path}': {message}"),
        }
    }
}

/// `claude profile list` 가 반환하는 항목 요약. IPC 응답으로는 `handlers.rs`
/// 가 `serde_json::json!` 로 직접 변환한다(이 crate 는 `serde` derive 를
/// 직접 의존하지 않는다 — `serde_json` 만으로 충분).
#[derive(Debug, Clone)]
pub struct ProfileSummary {
    /// `<owner>/<short>` 형식.
    pub id: String,
    pub owner: &'static str,
    /// 이름으로 attach 가능한지. 내장 훅 listing 항목은 `false`.
    pub attachable: bool,
    pub description: Option<String>,
}

/// host 가 코드로 내장한, 이름으로 attach 가능한 기본 프로필 — `MANAGED_HOOKS`
/// listing 항목(조회 전용, 사용자가 attach 할 수 없음)과 달리 실제 attach 대상이
/// 될 수 있는 `settings.json` 조각을 갖는다. 사용자가 같은 short-name 으로
/// 등록하면(`registered/<name>.json`) 그쪽이 우선한다 — `resolve_names`/
/// `show_registered` 모두 registered 파일을 먼저 찾고, 없을 때만 여기로
/// fallback 한다.
///
/// `continue-checklist` — `Stop` 훅으로 `tasty claude checklist-hook` 을 등록한다.
/// 명령 문자열은 `install::tasty_hook_command` 와 동일 형태(`$TASTY_SURFACE_ID`
/// 가드 + bare `tasty` 바이너리명)를 그대로 따른다. 체크리스트 본문은 이 JSON
/// 에 담기지 않는다 — hook 발화 시점에 `Translator` 로 해석된 문자열이 별도로
/// `reason` 필드에 실린다(`checklist.rs`).
fn host_default_profile(short_name: &str) -> Option<Value> {
    match short_name {
        "continue-checklist" => Some(json!({
            "hooks": {
                "Stop": [{
                    "matcher": "",
                    "hooks": [{
                        "type": "command",
                        "command": "[ -n \"$TASTY_SURFACE_ID\" ] && tasty claude checklist-hook || true"
                    }]
                }]
            }
        })),
        _ => None,
    }
}

/// `host_default_profile` 이 아는 이름 전체 — `list()` 이 attachable host 항목을
/// 나열할 때 순회한다.
const HOST_DEFAULT_PROFILE_NAMES: &[&str] = &["continue-checklist"];

fn registered_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("profiles").join("registered")
}

fn generated_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("profiles").join("generated")
}

fn registered_file(data_dir: &Path, short_name: &str) -> PathBuf {
    registered_dir(data_dir).join(format!("{short_name}.json"))
}

fn require_data_dir(data_dir: Option<&Path>) -> Result<&Path, ProfileError> {
    data_dir.ok_or(ProfileError::NoDataDir)
}

fn parse_names(names_csv: &str) -> Result<Vec<String>, ProfileError> {
    let names: Vec<String> = names_csv
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    if names.is_empty() {
        return Err(ProfileError::EmptyNames);
    }
    for n in &names {
        if !is_valid_short_name(n) {
            return Err(ProfileError::InvalidShortName(n.clone()));
        }
    }
    Ok(names)
}

// ── 등록/조회/해제 (user 출처) ────────────────────────────────────────────

/// `short_name` 으로 프로필을 등록한다. `source_path` 의 내용을 읽어 JSON
/// object 인지 검증한 뒤 `registered/<short_name>.json` 에 복사본을 쓴다 —
/// 원본이 나중에 옮겨지거나 지워져도 레지스트리는 영향받지 않는다.
/// 이미 등록된 이름이면 내용을 덮어쓴다(재등록으로 갱신하는 것이 정상 사용).
pub fn register(
    data_dir: Option<&Path>,
    short_name: &str,
    source_path: &Path,
) -> Result<(), ProfileError> {
    if !is_valid_short_name(short_name) {
        return Err(ProfileError::InvalidShortName(short_name.to_string()));
    }
    let data_dir = require_data_dir(data_dir)?;
    let text =
        std::fs::read_to_string(source_path).map_err(|e| ProfileError::SourceNotReadable {
            path: source_path.display().to_string(),
            message: e.to_string(),
        })?;
    let value: Value =
        serde_json::from_str(&text).map_err(|e| ProfileError::SourceNotReadable {
            path: source_path.display().to_string(),
            message: e.to_string(),
        })?;
    if !value.is_object() {
        return Err(ProfileError::SourceNotJsonObject {
            path: source_path.display().to_string(),
        });
    }
    let dir = registered_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| ProfileError::Io {
        path: dir.display().to_string(),
        message: e.to_string(),
    })?;
    let dest = registered_file(data_dir, short_name);
    std::fs::write(&dest, text).map_err(|e| ProfileError::Io {
        path: dest.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(())
}

/// `short_name` 등록을 해제한다. 없으면 `UnknownProfile`.
pub fn unregister(data_dir: Option<&Path>, short_name: &str) -> Result<(), ProfileError> {
    let data_dir = require_data_dir(data_dir)?;
    let path = registered_file(data_dir, short_name);
    if !path.is_file() {
        return Err(ProfileError::UnknownProfile(format!("user/{short_name}")));
    }
    std::fs::remove_file(&path).map_err(|e| ProfileError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(())
}

/// 등록된 프로필 원본 JSON 을 반환한다(`claude profile show`). 반환하는 owner
/// prefix(`"user"`/`"host"`)는 실제로 어느 출처에서 읽었는지를 그대로 반영한다 —
/// 사용자 등록이 없어 host 기본값으로 fallback 됐는데도 `"user/..."` 라고 답하면
/// 호출자가 실체와 다른 id 로 착각한다.
pub fn show_registered(
    data_dir: Option<&Path>,
    short_name: &str,
) -> Result<(&'static str, Value), ProfileError> {
    let data_dir = require_data_dir(data_dir)?;
    let path = registered_file(data_dir, short_name);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        // 사용자 등록이 없으면 host 내장 기본 프로필로 fallback(사용자 등록 우선).
        Err(_) => {
            if let Some(value) = host_default_profile(short_name) {
                return Ok(("host", value));
            }
            return Err(ProfileError::UnknownProfile(format!("user/{short_name}")));
        }
    };
    let value = serde_json::from_str(&text).map_err(|e| ProfileError::Io {
        path: path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(("user", value))
}

/// 등록된 사용자 프로필 전체 + 내장 훅 listing 항목을 함께 나열한다
/// (`priority` 개념이 필요 없는 단순 목록이라 owner tie-break 없이 owner→id
/// 순으로만 정렬 — host 를 먼저 보여줘 "항상 켜져 있는 것"이 먼저 눈에 띄게 한다).
pub fn list(data_dir: Option<&Path>) -> Vec<ProfileSummary> {
    let mut out: Vec<ProfileSummary> = MANAGED_HOOKS
        .iter()
        .map(|(claude_event, token, matcher)| ProfileSummary {
            id: format!("host/{token}"),
            owner: "host",
            attachable: false,
            description: Some(if matcher.is_empty() {
                format!("built-in — always installed (Claude Code event: {claude_event})")
            } else {
                format!(
                    "built-in — always installed (Claude Code event: {claude_event}, matcher: {matcher})"
                )
            }),
        })
        .collect();

    for name in HOST_DEFAULT_PROFILE_NAMES {
        out.push(ProfileSummary {
            id: format!("host/{name}"),
            owner: "host",
            attachable: true,
            description: Some("built-in — attach by name like a registered profile".to_string()),
        });
    }

    if let Some(data_dir) = data_dir {
        let dir = registered_dir(data_dir);
        let mut names: Vec<String> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".json").map(str::to_string)
            })
            .collect();
        names.sort();
        for short in names {
            out.push(ProfileSummary {
                id: format!("user/{short}"),
                owner: "user",
                attachable: true,
                description: None,
            });
        }
    }
    out
}

// ── 부착용 해석(조합 머지 포함) ────────────────────────────────────────────

/// `names_csv`(쉼표 구분 이름 목록)를 등록된 프로필 파일들로 해석하고, 필요하면
/// 머지해 `generated/` 에 실체화한 뒤 그 경로를 반환한다. 이름이 하나뿐이어도
/// 항상 이 경로를 통해 `generated/` 에 다시 쓴다 — attach 경로가 단수/복수로
/// 갈라지지 않게 하기 위함(등록 내용이 바뀐 뒤 재부착 시에도 최신 내용 보장).
pub fn resolve_names(data_dir: Option<&Path>, names_csv: &str) -> Result<PathBuf, ProfileError> {
    let data_dir = require_data_dir(data_dir)?;
    let names = parse_names(names_csv)?;

    let mut contents = Vec::with_capacity(names.len());
    for name in &names {
        let path = registered_file(data_dir, name);
        if !path.is_file() {
            // 사용자 등록이 없으면 host 내장 attachable 기본 프로필로 fallback
            // (예: `continue-checklist`) — 사용자 등록이 항상 우선한다.
            if let Some(value) = host_default_profile(name) {
                contents.push((format!("host/{name}"), value));
                continue;
            }
            // host/* listing 전용 항목(내장 훅)은 attachable 하지 않다 — 사용자가
            // 이름을 착각해 `--profile stop` 처럼 넘기면 "attach 불가" 를 명확히 알린다.
            if MANAGED_HOOKS.iter().any(|(_, token, _)| token == name) {
                return Err(ProfileError::NotAttachable(format!("host/{name}")));
            }
            return Err(ProfileError::UnknownProfile(format!("user/{name}")));
        }
        let text = std::fs::read_to_string(&path).map_err(|e| ProfileError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let value: Value = serde_json::from_str(&text).map_err(|e| ProfileError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        contents.push((format!("user/{name}"), value));
    }

    let (merged, warnings) = merge_contents(&contents).map_err(ProfileError::Merge)?;
    for w in &warnings {
        warn!("claude profile resolve({names_csv}): {w}");
    }

    let mut sorted_names = names.clone();
    sorted_names.sort();
    let key = sorted_names.join("+");
    let dir = generated_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| ProfileError::Io {
        path: dir.display().to_string(),
        message: e.to_string(),
    })?;
    let out_path = dir.join(format!("{key}.json"));
    let text = serde_json::to_string_pretty(&merged).unwrap_or_else(|_| "{}".to_string());
    std::fs::write(&out_path, text).map_err(|e| ProfileError::Io {
        path: out_path.display().to_string(),
        message: e.to_string(),
    })?;
    Ok(out_path)
}

// ── IPC handler (main.rs 배선 대상) ────────────────────────────────────────

fn require_name(params: &Value) -> Result<&str, IpcMethodError> {
    params
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing required 'name' parameter"))
}

fn to_ipc_err(e: ProfileError) -> IpcMethodError {
    IpcMethodError::new(format!("profile: {e}"))
}

fn summary_to_json(s: &ProfileSummary) -> Value {
    json!({
        "id": s.id,
        "owner": s.owner,
        "attachable": s.attachable,
        "description": s.description,
    })
}

/// `claude.profile_register` — `--name <short> --file <path>` 로 등록. `file` 은
/// CLI `path_kind = "file"` 정규화를 이미 거친 절대경로.
pub(crate) fn handle_register(
    data_dir: Option<&Path>,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let name = require_name(params)?;
    let file = params
        .get("file")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| IpcMethodError::invalid_params("Missing required 'file' parameter"))?;
    register(data_dir, name, Path::new(file)).map_err(to_ipc_err)?;
    Ok(json!({ "id": format!("user/{name}") }))
}

/// `claude.profile_unregister` — `--name <short>`.
pub(crate) fn handle_unregister(
    data_dir: Option<&Path>,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let name = require_name(params)?;
    unregister(data_dir, name).map_err(to_ipc_err)?;
    Ok(json!({ "id": format!("user/{name}") }))
}

/// `claude.profile_list` — 등록된 프로필 + 내장 훅 listing 항목 전체.
pub(crate) fn handle_list(
    data_dir: Option<&Path>,
    _params: &Value,
) -> Result<Value, IpcMethodError> {
    let entries: Vec<Value> = list(data_dir).iter().map(summary_to_json).collect();
    Ok(json!({ "profiles": entries }))
}

/// `claude.profile_show` — `--name <short>` 로 등록된 원본 JSON 내용을 그대로 반환.
pub(crate) fn handle_show(
    data_dir: Option<&Path>,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let name = require_name(params)?;
    let (owner, content) = show_registered(data_dir, name).map_err(to_ipc_err)?;
    Ok(json!({ "id": format!("{owner}/{name}"), "content": content }))
}

/// `claude.profile_current` — 이 세션(surface)에 지금 부착된 프로필 + 항상
/// 살아있는 내장 훅을 함께 보여준다("지금 이 세션에 무슨 프로필이 걸려 있나").
pub(crate) fn handle_current(
    data_dir: Option<&Path>,
    host: &HostHandle,
    params: &Value,
) -> Result<Value, IpcMethodError> {
    let surface_id = crate::handlers::require_surface_id(params)?;
    let attached = crate::reboot::attached_profile_summary(host, surface_id);
    let builtins: Vec<Value> = list(data_dir)
        .iter()
        .filter(|s| !s.attachable)
        .map(summary_to_json)
        .collect();
    Ok(json!({
        "surface_id": surface_id,
        "attached_names": attached.names,
        "attached_path": attached.path,
        "builtin": builtins,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_profile(dir: &Path, name: &str, content: &str) {
        std::fs::create_dir_all(registered_dir(dir)).unwrap();
        std::fs::write(registered_file(dir, name), content).unwrap();
    }

    #[test]
    fn register_copies_source_and_show_returns_it() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.json");
        std::fs::write(&src, r#"{"env":{"A":"1"}}"#).unwrap();
        register(Some(tmp.path()), "myprofile", &src).unwrap();
        let (owner, shown) = show_registered(Some(tmp.path()), "myprofile").unwrap();
        assert_eq!(owner, "user");
        assert_eq!(shown["env"]["A"], "1");
    }

    #[test]
    fn register_rejects_non_object_json() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.json");
        std::fs::write(&src, r#"["not","object"]"#).unwrap();
        let err = register(Some(tmp.path()), "bad", &src).unwrap_err();
        assert!(matches!(err, ProfileError::SourceNotJsonObject { .. }));
    }

    #[test]
    fn register_rejects_invalid_short_name() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.json");
        std::fs::write(&src, r#"{}"#).unwrap();
        let err = register(Some(tmp.path()), "Bad Name!", &src).unwrap_err();
        assert!(matches!(err, ProfileError::InvalidShortName(_)));
    }

    #[test]
    fn register_without_data_dir_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src.json");
        std::fs::write(&src, r#"{}"#).unwrap();
        let err = register(None, "x", &src).unwrap_err();
        assert_eq!(err, ProfileError::NoDataDir);
    }

    #[test]
    fn unregister_removes_and_second_call_errors() {
        let tmp = tempfile::tempdir().unwrap();
        write_profile(tmp.path(), "gone", "{}");
        unregister(Some(tmp.path()), "gone").unwrap();
        let err = unregister(Some(tmp.path()), "gone").unwrap_err();
        assert!(matches!(err, ProfileError::UnknownProfile(_)));
    }

    #[test]
    fn list_includes_builtin_hooks_and_user_profiles() {
        let tmp = tempfile::tempdir().unwrap();
        write_profile(tmp.path(), "myprofile", "{}");
        let entries = list(Some(tmp.path()));
        assert!(entries.iter().any(|e| e.id == "host/stop" && !e.attachable));
        assert!(
            entries
                .iter()
                .any(|e| e.id == "user/myprofile" && e.attachable)
        );
    }

    #[test]
    fn list_without_data_dir_still_shows_builtins() {
        let entries = list(None);
        assert!(!entries.is_empty());
        assert!(entries.iter().all(|e| e.owner == "host"));
    }

    #[test]
    fn resolve_single_name_materializes_generated_file() {
        let tmp = tempfile::tempdir().unwrap();
        write_profile(tmp.path(), "solo", r#"{"env":{"A":"1"}}"#);
        let path = resolve_names(Some(tmp.path()), "solo").unwrap();
        assert!(path.starts_with(generated_dir(tmp.path())));
        let content: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(content["env"]["A"], "1");
    }

    #[test]
    fn resolve_two_names_merges_both() {
        let tmp = tempfile::tempdir().unwrap();
        write_profile(tmp.path(), "one", r#"{"env":{"A":"1"}}"#);
        write_profile(tmp.path(), "two", r#"{"env":{"B":"2"}}"#);
        let path = resolve_names(Some(tmp.path()), "one,two").unwrap();
        let content: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(content["env"]["A"], "1");
        assert_eq!(content["env"]["B"], "2");
    }

    #[test]
    fn resolve_is_order_independent_for_generated_filename() {
        let tmp = tempfile::tempdir().unwrap();
        write_profile(tmp.path(), "one", "{}");
        write_profile(tmp.path(), "two", "{}");
        let a = resolve_names(Some(tmp.path()), "one,two").unwrap();
        let b = resolve_names(Some(tmp.path()), "two,one").unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn resolve_unknown_name_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_names(Some(tmp.path()), "nope").unwrap_err();
        assert!(matches!(err, ProfileError::UnknownProfile(_)));
    }

    #[test]
    fn resolve_builtin_hook_name_is_not_attachable() {
        let tmp = tempfile::tempdir().unwrap();
        let err = resolve_names(Some(tmp.path()), "stop").unwrap_err();
        assert!(matches!(err, ProfileError::NotAttachable(_)));
    }

    #[test]
    fn resolve_reflects_latest_registered_content_on_reattach() {
        let tmp = tempfile::tempdir().unwrap();
        write_profile(tmp.path(), "changing", r#"{"env":{"A":"1"}}"#);
        let first = resolve_names(Some(tmp.path()), "changing").unwrap();
        let first_content: Value =
            serde_json::from_str(&std::fs::read_to_string(&first).unwrap()).unwrap();
        assert_eq!(first_content["env"]["A"], "1");

        write_profile(tmp.path(), "changing", r#"{"env":{"A":"2"}}"#);
        let second = resolve_names(Some(tmp.path()), "changing").unwrap();
        let second_content: Value =
            serde_json::from_str(&std::fs::read_to_string(&second).unwrap()).unwrap();
        assert_eq!(second_content["env"]["A"], "2");
    }

    #[test]
    fn resolve_host_default_profile_by_name() {
        let tmp = tempfile::tempdir().unwrap();
        let path = resolve_names(Some(tmp.path()), "continue-checklist").unwrap();
        let content: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert!(content["hooks"]["Stop"].is_array());
    }

    #[test]
    fn resolve_prefers_user_registration_over_host_default() {
        let tmp = tempfile::tempdir().unwrap();
        write_profile(
            tmp.path(),
            "continue-checklist",
            r#"{"env":{"OVERRIDDEN":"1"}}"#,
        );
        let path = resolve_names(Some(tmp.path()), "continue-checklist").unwrap();
        let content: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(content["env"]["OVERRIDDEN"], "1");
        assert!(content["hooks"].is_null());
    }

    #[test]
    fn show_registered_falls_back_to_host_default() {
        let tmp = tempfile::tempdir().unwrap();
        let (owner, content) = show_registered(Some(tmp.path()), "continue-checklist").unwrap();
        assert_eq!(owner, "host");
        assert!(content["hooks"]["Stop"].is_array());
    }

    #[test]
    fn list_includes_attachable_host_default_profile() {
        let entries = list(None);
        assert!(
            entries
                .iter()
                .any(|e| e.id == "host/continue-checklist" && e.attachable)
        );
    }
}
