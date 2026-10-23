use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::*;
use crate::codex_return::Returns;
use crate::error_scan::ErrorScanner;

fn tr() -> Translator {
    Translator::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("lang"), "en")
}

#[derive(Default)]
struct Host {
    calls: Mutex<Vec<(String, Value)>>,
    meta: Mutex<HashMap<(u32, String), String>>,
}

impl HostCall for Host {
    fn call(&self, method: &str, params: Value) -> Result<Value, tasty_plugin_sdk::PluginError> {
        let mut calls = self.calls.lock().unwrap();
        calls.push((method.into(), params.clone()));
        let surface = params["surface_id"].as_u64().unwrap_or(0) as u32;
        let key = params["key"].as_str().unwrap_or("").to_string();
        match method {
            "terminal.spawn" => Ok(
                json!({"child_surface_id": 100 + calls.iter().filter(|(m, _)| m == method).count(), "index": 0}),
            ),
            "surface.meta.set" => {
                self.meta
                    .lock()
                    .unwrap()
                    .insert((surface, key), params["value"].as_str().unwrap().into());
                Ok(json!({}))
            }
            "surface.meta.unset" => {
                self.meta.lock().unwrap().remove(&(surface, key));
                Ok(json!({}))
            }
            "surface.meta.get" => {
                Ok(json!({"value": self.meta.lock().unwrap().get(&(surface, key))}))
            }
            "settings.get_plugin_setting" => Ok(json!({})),
            "session.issue" => Ok(json!({"token": "mock-token"})),
            "surface.send" | "hook.set" => Ok(json!({})),
            "terminal.children" => Ok(json!({"children": []})),
            "hook.list" => Ok(json!([])),
            other => panic!("unexpected host call {other}: {params}"),
        }
    }
}

fn spawn(host: &Host, extra: Value) -> Result<Value, IpcMethodError> {
    let mut params = json!({"surface": 7, "workspace": "test"});
    params
        .as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    crate::handlers::handle_spawn(
        &Arc::new(Mutex::new(ErrorScanner::new())),
        &Returns::new(None),
        host,
        &params,
        None,
        &tr(),
    )
}

#[test]
fn invalid_values_are_rejected_before_any_host_call_or_spawn() {
    let host = Host::default();
    let invalid = [
        Value::Null,
        json!(true),
        json!(7),
        json!([]),
        json!({}),
        json!(""),
        json!(" "),
        json!("--help"),
        json!("-m"),
        json!("opus --fallback-model sonnet"),
        json!("opus\nsonnet"),
        json!("a\0b"),
    ];
    for key in ["model", "effort"] {
        for value in &invalid {
            let mut extra = json!({});
            extra[key] = value.clone();
            let err = spawn(&host, extra).unwrap_err();
            assert!(err.message.contains(key), "{err:?}");
            assert!(
                host.calls.lock().unwrap().is_empty(),
                "side effect before rejecting {key}={value}"
            );
        }
    }
    assert!(spawn(&host, json!({"effort": "unlimited"})).is_err());
    assert!(host.calls.lock().unwrap().is_empty());
}

#[test]
fn aliases_full_ids_arns_and_documented_efforts_are_not_model_allowlisted() {
    for model in [
        "opus",
        "sonnet[1m]",
        "claude-opus-4-7",
        "arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.claude-opus-v1:0",
        "vendor/custom-deployment",
    ] {
        for effort in EFFORTS {
            let options =
                SessionOptions::parse(&json!({"model": model, "effort": effort}), &tr()).unwrap();
            assert_eq!(options.model.as_deref(), Some(model));
            assert_eq!(options.effort.as_deref(), Some(*effort));
        }
    }
}

#[test]
fn session_record_survives_lost_surface_metadata_and_default_clears_reused_surface() {
    let dir = tempfile::tempdir().unwrap();
    let host = Host::default();
    let first = SessionOptions::parse(&json!({"model": "opus", "effort": "max"}), &tr()).unwrap();
    first.record(&host, 101).unwrap();
    store(Some(dir.path()), "session-a", &first);
    assert_eq!(
        load(&host, 999, Some(dir.path()), "session-a", &tr()).unwrap(),
        first
    );
    assert_eq!(
        load(&host, 999, Some(dir.path()), "session-b", &tr()).unwrap(),
        SessionOptions::default()
    );
    SessionOptions::default().record(&host, 101).unwrap();
    assert_eq!(
        load(&host, 101, Some(dir.path()), "session-a", &tr()).unwrap(),
        SessionOptions::default()
    );
    assert!(record_path(Some(dir.path()), "../escape").is_none());
    std::fs::write(record_path(Some(dir.path()), "broken").unwrap(), "{broken").unwrap();
    assert!(load(&host, 999, Some(dir.path()), "broken", &tr()).is_err());
}

/// Exercise the real spawn handler, then run its shell command against an isolated mock CLI.
/// No user CLI, settings, provider or app server is involved.
#[cfg(unix)]
#[test]
fn isolated_mock_cli_receives_independent_child_argv_profiles_and_omission() {
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .without_time()
        .with_writer(std::io::stderr)
        .finish();
    let _subscriber = tracing::subscriber::set_default(subscriber);
    let dir = tempfile::tempdir().unwrap();
    let executable = dir.path().join("claude");
    std::fs::write(
        &executable,
        "#!/usr/bin/python3\nimport json,sys\nprint(json.dumps(sys.argv[1:]))\n",
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let profile = dir.path().join("profile'$(false)`false`\".json");
    let profile_text = r#"{"model":"profile-model","effortLevel":"low","permissions":{"allow":["Read"]},"hooks":{}}"#;
    std::fs::write(&profile, profile_text).unwrap();
    let host = Host::default();
    host.meta
        .lock()
        .unwrap()
        .insert((7, META_KEY.into()), "parent-unchanged".into());
    host.meta
        .lock()
        .unwrap()
        .insert((8, META_KEY.into()), "sibling-unchanged".into());
    let model = "arn:aws:bedrock:region:123:profile/vendor'$(false);`false`";
    let a = spawn(&host, json!({"model": model, "effort": "high", "profile_file": profile, "permission_mode": "plan", "prompt": "literal $HOME ; `false` 'quoted'"})).unwrap();
    let b = spawn(&host, json!({"model": "sonnet[1m]", "effort": "medium"})).unwrap();
    let c = spawn(&host, json!({})).unwrap();
    assert_eq!(a["requested_session_options"]["model"], model);
    assert_eq!(b["requested_session_options"]["effort"], "medium");
    assert_eq!(c["requested_session_options"], json!({}));
    let commands: Vec<_> = host
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(m, _)| m == "surface.send")
        .map(|(_, p)| p["text"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(commands.len(), 3);
    let mut argv = Vec::new();
    for command in &commands {
        let output = Command::new("sh")
            .arg("-c")
            .arg(command.trim_end_matches('\r'))
            .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        argv.push(serde_json::from_slice::<Vec<String>>(&output.stdout).unwrap());
    }
    assert_eq!(
        argv[0],
        vec![
            "--model",
            model,
            "--effort",
            "high",
            "--settings",
            profile.to_str().unwrap(),
            "--permission-mode",
            "plan",
            "literal $HOME ; `false` 'quoted'"
        ]
    );
    assert_eq!(argv[1], vec!["--model", "sonnet[1m]", "--effort", "medium"]);
    assert!(argv[2].is_empty());
    assert_eq!(std::fs::read_to_string(&profile).unwrap(), profile_text);
    let meta = host.meta.lock().unwrap();
    assert_eq!(meta[&(7, META_KEY.into())], "parent-unchanged");
    assert_eq!(meta[&(8, META_KEY.into())], "sibling-unchanged");
    std::fs::write(
        &executable,
        "#!/usr/bin/python3\nimport json,sys\nprint(json.dumps(sys.argv[1:]))\nsys.exit(42)\n",
    )
    .unwrap();
    let rejected = Command::new("sh")
        .arg("-c")
        .arg(commands[1].trim_end_matches('\r'))
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .output()
        .unwrap();
    assert_eq!(rejected.status.code(), Some(42));
    assert_eq!(
        serde_json::from_slice::<Vec<String>>(&rejected.stdout).unwrap(),
        argv[1]
    );
    tracing::info!("mock rejection: shell rc=42; one invocation, no fallback-model argument");
    tracing::info!(
        "mock_cli child argv: {}",
        serde_json::to_string(&argv).unwrap()
    );
}

#[test]
fn hook_restore_and_reboot_preserve_options_and_settings_without_permission_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let options = SessionOptions::parse(
        &json!({"model":"provider/model[1m]", "effort":"ultracode"}),
        &tr(),
    )
    .unwrap();
    store(Some(dir.path()), "sess-a", &options);
    let host = Host::default();
    let mut calls = vec![PlannedCall::MetaSet {
        surface_id: 999,
        key: RESTORE_COMMAND_META_KEY,
        value: crate::reboot::resume_command_line("sess-a", Some("/profile.json")),
    }];
    session_start(&mut calls, &host, 999, Some(dir.path()), "sess-a", &tr());
    assert!(matches!(&calls[0], PlannedCall::MetaSet {value, ..}
        if value == "claude -r sess-a --settings \"/profile.json\" --model 'provider/model[1m]' --effort ultracode"));
    assert!(matches!(&calls[1], PlannedCall::MetaSet {key, value, ..}
        if *key == META_KEY && value == &options.json().to_string()));
    let reboot =
        crate::reboot::resume_with_options("sess-a", Some("/profile.json"), Some("plan"), &options);
    assert_eq!(
        reboot,
        "claude -r sess-a --settings \"/profile.json\" --permission-mode plan --model 'provider/model[1m]' --effort ultracode\r"
    );
    std::fs::write(record_path(Some(dir.path()), "sess-a").unwrap(), "bad JSON").unwrap();
    session_start(&mut calls, &host, 999, Some(dir.path()), "sess-a", &tr());
    assert!(!calls.iter().any(
        |call| matches!(call, PlannedCall::MetaSet { key, .. } if *key == RESTORE_COMMAND_META_KEY)
    ));
}

#[test]
fn registered_profile_is_not_edited_by_explicit_model_and_effort() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("profile.json");
    let body = r#"{"model":"profile-model","effortLevel":"low","env":{"KEEP":"yes"}}"#;
    std::fs::write(&source, body).unwrap();
    crate::profile::register(Some(dir.path()), "worker", &source).unwrap();
    let params = json!({"profile": "worker", "model": "opus", "effort": "high"});
    let file = crate::handlers::resolve_profile_file_param(Some(dir.path()), &params, &tr())
        .unwrap()
        .unwrap();
    let options = SessionOptions::parse(&params, &tr()).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(&file).unwrap()).unwrap(),
        serde_json::from_str::<Value>(body).unwrap()
    );
    let command = format!("claude --settings {}{}", quote_path(&file), options.flags());
    assert!(command.ends_with(" --model opus --effort high"));
    assert_eq!(std::fs::read_to_string(&source).unwrap(), body);
}
