//! Capture managed relay context in the CLI caller, never in the shared plugin.
use anyhow::{Result, bail};
use serde_json::{Map, Value};
use std::path::Path;

pub(super) fn fill(
    params: &mut Map<String, Value>,
    thread: Option<&str>,
    has_agent_context: bool,
    executable: &Path,
    port_file: &Path,
) -> Result<()> {
    if params.get("action").and_then(Value::as_str) != Some("prepare") {
        return Ok(());
    }
    if has_agent_context {
        bail!("managed_relay_requires_local_caller");
    }
    if !params.contains_key("parent_thread") {
        params.insert(
            "parent_thread".into(),
            Value::String(thread.unwrap_or("").into()),
        );
    }
    let parent = params["parent_thread"].as_str().unwrap_or("");
    if !uuid(parent) {
        bail!("managed_parent_thread_required");
    }
    if !params.contains_key("cli_path") {
        params.insert("cli_path".into(), Value::String(absolute(executable)?));
    }
    params.insert("port_file".into(), Value::String(absolute(port_file)?));
    params.insert("expected_auth_mode".into(), Value::String("local".into()));
    Ok(())
}

fn absolute(path: &Path) -> Result<String> {
    let path = std::path::absolute(path)?;
    if !path.is_file() {
        bail!("managed_relay_path_not_file: {}", path.display());
    }
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("managed_relay_path_not_utf8"))
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn caller_context_is_explicit_and_does_not_require_environment_mutation() {
        let exe = std::env::current_exe().unwrap();
        let mut params = json!({"action":"prepare"}).as_object().unwrap().clone();
        fill(
            &mut params,
            Some("11111111-1111-1111-1111-111111111111"),
            false,
            &exe,
            &exe,
        )
        .unwrap();
        assert_eq!(
            params["parent_thread"],
            "11111111-1111-1111-1111-111111111111"
        );
        assert_eq!(params["expected_auth_mode"], "local");
        fill(
            &mut params,
            Some("22222222-2222-2222-2222-222222222222"),
            false,
            &exe,
            &exe,
        )
        .unwrap();
        assert_eq!(
            params["parent_thread"],
            "11111111-1111-1111-1111-111111111111"
        );
    }
    #[test]
    fn missing_parent_or_tokenized_caller_is_not_silently_reinterpreted() {
        let exe = std::env::current_exe().unwrap();
        for parent in [None, Some(""), Some("surface-1297")] {
            let mut params = json!({"action":"prepare"}).as_object().unwrap().clone();
            assert!(fill(&mut params, parent, false, &exe, &exe).is_err());
        }
        let mut params =
            json!({"action":"prepare","parent_thread":"11111111-1111-1111-1111-111111111111"})
                .as_object()
                .unwrap()
                .clone();
        assert!(fill(&mut params, None, true, &exe, &exe).is_err());
    }
}
