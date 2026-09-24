//! Scoped hook configuration installation; never guesses a remote daemon configuration.
use serde_json::{Value, json};
use tasty_plugin_sdk::{IpcMethodError, i18n::Translator};

pub(crate) fn handle_install(params: &Value, tr: &Translator) -> Result<Value, IpcMethodError> {
    let path = selected_path(params, tr)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            IpcMethodError::new(tr.t_replace(
                "codex.install.mkdir_failed",
                "{detail}",
                &e.to_string(),
            ))
        })?;
    }
    let existing = read_config(&path, tr)?;
    let merged = merge_install(existing);
    write_toml(&path, &merged, tr)?;
    let trusted = codex_hooks_all_trusted_in(&merged, &path.to_string_lossy());
    let mut resp = json!({
        "installed": true,
        "path": path.to_string_lossy(),
        "trust_status": if trusted { "trusted" } else { "needs_review" },
    });
    if !trusted {
        resp["note"] = Value::String(
            "Review newly added or changed hooks in Codex using /hooks. Tasty includes \
--dangerously-bypass-hook-trust in launch commands, but Codex may still show hook review \
on resume. This metadata is not runtime trust verification; installation does not approve \
hooks or prove that a remote daemon loaded this configuration."
                .into(),
        );
    }
    Ok(resp)
}

pub(crate) fn handle_uninstall(params: &Value, tr: &Translator) -> Result<Value, IpcMethodError> {
    let path = selected_path(params, tr)?;
    if !path.exists() {
        return Ok(json!({ "uninstalled": true, "path": path.to_string_lossy(), "noop": true }));
    }
    let existing = read_config(&path, tr)?;
    let cleaned = remove_install(existing);
    write_toml(&path, &cleaned, tr)?;
    Ok(json!({ "uninstalled": true, "path": path.to_string_lossy() }))
}

use std::path::{Path, PathBuf};

pub(crate) const HOOK_MARKER: &str = "tasty codex hook";

/// 설정 테이블의 CamelCase, Tasty 명령의 kebab-case, 신뢰 기록의 snake_case 이름.
/// 승인 요청을 도구 이름으로 제한하지 않도록 matcher를 지정하지 않는다.
pub(crate) const HOOK_EVENTS: &[(&str, &str, &str)] = &[
    ("Stop", "stop", "stop"),
    ("UserPromptSubmit", "prompt-submit", "user_prompt_submit"),
    ("SessionStart", "session-start", "session_start"),
    ("SessionEnd", "session-end", "session_end"),
    (
        "PermissionRequest",
        "permission-request",
        "permission_request",
    ),
    ("PostToolUse", "post-tool-use", "post_tool_use"),
    ("Interrupt", "interrupt", "interrupt"),
];

/// CODEX_HOME, HOME, USERPROFILE 순으로 설정 경로를 선택한다.
pub(crate) fn config_toml_path_opt() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("CODEX_HOME").filter(|s| !s.is_empty()) {
        return Some(PathBuf::from(home).join("config.toml"));
    }
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
    Some(PathBuf::from(home).join(".codex").join("config.toml"))
}

pub(crate) fn codex_config_toml_path(tr: &Translator) -> Result<PathBuf, IpcMethodError> {
    config_toml_path_opt().ok_or_else(|| IpcMethodError::new(tr.t("codex.install.no_home")))
}

fn read_config(path: &Path, tr: &Translator) -> Result<toml::Value, IpcMethodError> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).map_err(|error| {
            IpcMethodError::new(tr.t_replace(
                "codex.install.read_failed",
                "{detail}",
                &error.to_string(),
            ))
        }),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(toml::Value::Table(toml::map::Map::new()))
        }
        Err(error) => Err(IpcMethodError::new(tr.t_replace(
            "codex.install.read_failed",
            "{detail}",
            &error.to_string(),
        ))),
    }
}
fn selected_path(params: &Value, tr: &Translator) -> Result<PathBuf, IpcMethodError> {
    let file = params.get("config_file").filter(|v| !v.is_null());
    let home = params.get("codex_home").filter(|v| !v.is_null());
    if file.is_some() && home.is_some() {
        return Err(IpcMethodError::invalid_params(
            tr.t("codex.install.path_conflict"),
        ));
    }
    let invalid = || IpcMethodError::invalid_params(tr.t("codex.install.home_absolute"));
    if let Some(raw) = file.or(home) {
        let path = raw
            .as_str()
            .map(Path::new)
            .filter(|p| p.is_absolute())
            .ok_or_else(invalid)?;
        return Ok(if file.is_some() {
            path.to_path_buf()
        } else {
            path.join("config.toml")
        });
    }
    let path = codex_config_toml_path(tr)?;
    if !path.is_absolute() {
        return Err(invalid());
    }
    Ok(path)
}

pub(crate) fn write_toml(
    path: &Path,
    value: &toml::Value,
    tr: &Translator,
) -> Result<(), IpcMethodError> {
    let text = toml::to_string_pretty(value).map_err(|e| {
        IpcMethodError::new(tr.t_replace("codex.install.encode_failed", "{detail}", &e.to_string()))
    })?;
    std::fs::write(path, text).map_err(|e| {
        IpcMethodError::new(tr.t_replace("codex.install.write_failed", "{detail}", &e.to_string()))
    })
}

pub(crate) fn hook_command(event_kebab: &str) -> String {
    // surface ID가 없으면 실행하지 않는다. CLI 내부 응답은 버리고 Codex에는 {}를 보낸다.
    // stderr는 전달 오류를 확인할 수 있도록 유지한다. Windows 훅에는 PowerShell 문법을 쓴다.
    #[cfg(windows)]
    {
        format!(
            "if ($env:TASTY_SURFACE_ID) {{ $input | tasty codex hook {event_kebab} --surface $env:TASTY_SURFACE_ID > $null; '{{}}' }}"
        )
    }
    // Tasty CLI가 실패를 기록한 뒤에도 Codex 턴을 막지 않도록 성공으로 반환한다.
    #[cfg(not(windows))]
    {
        format!(
            "if [ -n \"$TASTY_SURFACE_ID\" ]; then tasty codex hook {event_kebab} --surface $TASTY_SURFACE_ID > /dev/null || true; printf '{{}}\\n'; fi"
        )
    }
}

pub(crate) fn new_matcher_group(event_kebab: &str) -> toml::Value {
    let mut handler = toml::map::Map::new();
    handler.insert("type".into(), toml::Value::String("command".into()));
    handler.insert(
        "command".into(),
        toml::Value::String(hook_command(event_kebab)),
    );
    let mut group = toml::map::Map::new();
    group.insert(
        "hooks".into(),
        toml::Value::Array(vec![toml::Value::Table(handler)]),
    );
    toml::Value::Table(group)
}

/// 시험에서 Tasty 표식이 있는 훅을 찾는다.
#[cfg(test)]
pub(crate) fn matcher_group_has_marker(item: &toml::Value, marker: &str) -> bool {
    let Some(group) = item.as_table() else {
        return false;
    };
    let Some(hooks) = group.get("hooks").and_then(|v| v.as_array()) else {
        return false;
    };
    hooks.iter().any(|h| {
        h.as_table()
            .and_then(|t| t.get("command"))
            .and_then(|c| c.as_str())
            .map(|s| s.contains(marker))
            .unwrap_or(false)
    })
}

/// Remove only our handlers, retaining a mixed group's matcher and user metadata.
fn remove_managed_handlers(groups: &mut Vec<toml::Value>) {
    groups.retain_mut(|group| {
        let Some(handlers) = group.get_mut("hooks").and_then(toml::Value::as_array_mut) else {
            return true;
        };
        let before = handlers.len();
        handlers.retain(|handler| {
            !handler
                .get("command")
                .and_then(toml::Value::as_str)
                .is_some_and(|command| command.contains(HOOK_MARKER))
        });
        handlers.len() == before || !handlers.is_empty()
    });
}

/// Tasty 훅을 교체하고 사용자 훅과 다른 설정은 보존한다.
pub(crate) fn merge_install(mut value: toml::Value) -> toml::Value {
    let Some(table) = value.as_table_mut() else {
        return value;
    };
    let hooks_table = table
        .entry("hooks".to_string())
        .or_insert_with(|| toml::Value::Table(toml::map::Map::new()));
    let Some(hooks) = hooks_table.as_table_mut() else {
        return value;
    };
    for (event_key, kebab, _trust_snake) in HOOK_EVENTS {
        let event_array = hooks
            .entry((*event_key).to_string())
            .or_insert_with(|| toml::Value::Array(Vec::new()));
        let Some(arr) = event_array.as_array_mut() else {
            continue;
        };
        // 기존 Tasty 훅을 제거한 뒤 새 항목을 넣는다.
        remove_managed_handlers(arr);
        arr.push(new_matcher_group(kebab));
    }
    value
}

/// 설치한 이벤트의 신뢰 기록에 비어 있지 않은 sha256: 해시가 있는지 확인한다.
/// 현재 명령과 해시를 비교하지 않으므로 실제 실행 중인 Codex의 신뢰 여부는 보장하지 않는다.
pub(crate) fn codex_hooks_all_trusted_in(value: &toml::Value, source_path: &str) -> bool {
    let Some(state_table) = value
        .get("hooks")
        .and_then(|v| v.get("state"))
        .and_then(|v| v.as_table())
    else {
        return false;
    };
    for (_, _, trust_snake) in HOOK_EVENTS {
        let key = format!("{source_path}:{trust_snake}:0:0");
        let trusted = state_table
            .get(&key)
            .and_then(|v| v.as_table())
            .and_then(|t| t.get("trusted_hash"))
            .and_then(|h| h.as_str())
            .map(|s| s.starts_with("sha256:") && s.len() > "sha256:".len())
            .unwrap_or(false);
        if !trusted {
            return false;
        }
    }
    true
}

pub(crate) fn remove_install(mut value: toml::Value) -> toml::Value {
    let Some(table) = value.as_table_mut() else {
        return value;
    };
    let Some(hooks_table) = table.get_mut("hooks").and_then(|v| v.as_table_mut()) else {
        return value;
    };
    // Tasty 핸들러만 제거한다. 사용자 핸들러와 그룹 메타데이터는 남긴다.
    let event_keys: Vec<String> = hooks_table.keys().cloned().collect();
    for key in event_keys {
        if let Some(arr) = hooks_table.get_mut(&key).and_then(|v| v.as_array_mut()) {
            remove_managed_handlers(arr);
        }
    }
    // 빈 array 가 된 event 키 정리.
    hooks_table.retain(|_, v| !v.as_array().map(|a| a.is_empty()).unwrap_or(false));
    // [hooks] 가 텅 비면 제거.
    if hooks_table.is_empty() {
        table.remove("hooks");
    }
    value
}

#[cfg(test)]
mod path_tests {
    use super::*;
    #[test]
    fn malformed_explicit_context_never_falls_back_to_default_config() {
        let tr = Translator::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("lang"), "en");
        for params in [
            json!({"codex_home":42}),
            json!({"config_file":false}),
            json!({"codex_home":"relative"}),
        ] {
            assert!(selected_path(&params, &tr).is_err());
        }
        // 이유: 경로 문자열만 비교하며 이 위치에 파일이나 디렉터리를 만들지 않는다.
        let dir = std::env::temp_dir();
        assert_eq!(
            selected_path(&json!({"codex_home":dir,"config_file":null}), &tr).unwrap(),
            dir.join("config.toml")
        );
    }
}
