//! 훅 핸들러 병합·정렬·바인딩 제한·사용자 설정 저장과 재로딩 검사.

use super::*;
use crate::hook_handler::types::{HookHandlerId, IpcCall, validate_binding};

fn load_host(reg: &HookHandlerRegistry) {
    reg.install_host_defaults(include_str!("defaults/default-hook-handlers.toml"));
}

const HOST_NOTIFY_ID: &str = "host/webhook-notify";
const HOST_COMMAND_COMPLETED_ID: &str = "host/command-completed";

fn plugin_ipc(
    short: &str,
    source: HookSource,
    priority: i32,
    method: &str,
) -> HookHandlerDecl<PluginHookHandlerActionDecl> {
    HookHandlerDecl::<PluginHookHandlerActionDecl> {
        id: short.into(),
        source,
        priority,
        display_name_i18n_key: None,
        disabled: false,
        action: PluginHookHandlerActionDecl::IpcSequence {
            calls: vec![IpcCall {
                method: method.into(),
                params: serde_json::json!({}),
            }],
        },
    }
}

fn write_user_toml(dir: &tempfile::TempDir, body: &str) -> std::path::PathBuf {
    let p = dir.path().join("hook-handlers.toml");
    std::fs::write(&p, body).unwrap();
    p
}

#[test]
fn host_defaults_load() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let h = reg
        .get(&HookHandlerId::new(HOST_NOTIFY_ID))
        .expect("host handler");
    assert_eq!(h.source, HookSource::Webhook);
    assert_eq!(h.owner, HookHandlerOwner::Host);
    assert!(matches!(h.action, HookHandlerAction::IpcSequence { .. }));
    assert!(reg.contains(&HookHandlerId::new(HOST_NOTIFY_ID)));
}

#[test]
fn all_handlers_returns_every_enabled() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    assert_eq!(reg.all_handlers().len(), 2);
    assert_eq!(
        reg.list_handlers(),
        vec![
            HookHandlerId::new(HOST_COMMAND_COMPLETED_ID),
            HookHandlerId::new(HOST_NOTIFY_ID),
        ]
    );
}

#[test]
fn all_handlers_including_disabled_shows_disabled() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    reg.set_user_handler_disabled(&HookHandlerId::new(HOST_NOTIFY_ID), true);
    reg.set_user_handler_disabled(&HookHandlerId::new(HOST_COMMAND_COMPLETED_ID), true);
    assert!(reg.all_handlers().is_empty());
    let full = reg.all_handlers_including_disabled();
    assert_eq!(full.len(), 2);
    assert!(full.iter().all(|h| h.disabled));
    assert!(
        full.iter()
            .any(|h| h.id == HookHandlerId::new(HOST_NOTIFY_ID))
    );
    assert!(
        full.iter()
            .any(|h| h.id == HookHandlerId::new(HOST_COMMAND_COMPLETED_ID))
    );
}

#[test]
fn plugin_install_and_lower_priority_sorts_first() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    reg.install_plugin_handlers(
        "com.example.hook",
        &[plugin_ipc(
            "relay",
            HookSource::Webhook,
            10,
            "notification.create",
        )],
    );
    let v = reg.handlers_for_source(TriggerSource::Webhook);
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].id.as_str(), "com.example.hook/relay");
    assert_eq!(v[1].id.as_str(), HOST_NOTIFY_ID);
}

#[test]
fn uninstall_plugin_removes_only_its_handlers() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    reg.install_plugin_handlers(
        "com.example.hook",
        &[plugin_ipc(
            "relay",
            HookSource::Any,
            20,
            "notification.create",
        )],
    );
    assert_eq!(reg.all_handlers().len(), 3);
    reg.uninstall_plugin("com.example.hook");
    let v = reg.all_handlers();
    assert_eq!(v.len(), 2);
    assert!(v.iter().any(|h| h.id.as_str() == HOST_NOTIFY_ID));
    assert!(v.iter().any(|h| h.id.as_str() == HOST_COMMAND_COMPLETED_ID));
}

#[test]
fn plugin_reinstall_is_idempotent() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let decls = [plugin_ipc(
        "relay",
        HookSource::Any,
        30,
        "notification.create",
    )];
    reg.install_plugin_handlers("com.example.hook", &decls);
    reg.install_plugin_handlers("com.example.hook", &decls);
    assert_eq!(reg.all_handlers().len(), 3);
}

#[test]
fn owner_tiebreak_user_gt_plugin_gt_host() {
    let reg = HookHandlerRegistry::new();
    reg.install_host_defaults(
        r#"
        [[handler]]
        id = "same"
        source = "any"
        priority = 50
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = {} }]
        "#,
    );
    reg.install_plugin_handlers(
        "com.example.hook",
        &[plugin_ipc(
            "same",
            HookSource::Any,
            50,
            "notification.create",
        )],
    );
    let dir = tempfile::tempdir().unwrap();
    let p = write_user_toml(
        &dir,
        r#"
        [[handler]]
        id = "user/same"
        source = "any"
        priority = 50
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = {} }]
        "#,
    );
    reg.install_user_config(&p);

    let v = reg.handlers_for_source(TriggerSource::Hook);
    let ids: Vec<&str> = v.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(ids[0], "user/same");
    assert_eq!(ids[1], "com.example.hook/same");
    assert_eq!(ids[2], "host/same");
}

/// plugin 설치 순서나 재시작과 무관하게 사용자 설정이 우선해야 한다.
const PLUGIN_PATCH_ID: &str = "com.example.hookp/notify";

fn user_patch_for_plugin(dir: &tempfile::TempDir) -> std::path::PathBuf {
    write_user_toml(
        dir,
        &format!(
            r#"
            [[handler]]
            id = "{PLUGIN_PATCH_ID}"
            priority = 10
            display_name_i18n_key = "user.key"
            "#
        ),
    )
}

fn plugin_notify() -> HookHandlerDecl<PluginHookHandlerActionDecl> {
    let mut d = plugin_ipc("notify", HookSource::Webhook, 50, "notification.create");
    d.display_name_i18n_key = Some("plugin.key".into());
    d
}

#[test]
fn a_user_patch_wins_over_a_plugin_installed_after_the_boot_load() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    reg.install_user_config(&user_patch_for_plugin(&dir));
    reg.install_plugin_handlers("com.example.hookp", &[plugin_notify()]);
    let h = reg.get(&HookHandlerId::new(PLUGIN_PATCH_ID)).unwrap();
    assert_eq!(h.priority, 10);
    assert_eq!(h.display_name_i18n_key.as_deref(), Some("user.key"));
    assert_eq!(h.owner, HookHandlerOwner::User);
    assert_eq!(h.source, HookSource::Webhook);
}

#[test]
fn a_user_patch_wins_over_a_plugin_that_contributes_later_without_a_reload() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    reg.install_plugin_handlers("com.example.hookp", &[plugin_notify()]);
    let dir = tempfile::tempdir().unwrap();
    reg.reload_user_config(&user_patch_for_plugin(&dir));
    assert_eq!(
        reg.get(&HookHandlerId::new(PLUGIN_PATCH_ID))
            .unwrap()
            .priority,
        10
    );
    reg.uninstall_plugin("com.example.hookp");
    reg.install_plugin_handlers("com.example.hookp", &[plugin_notify()]);
    let h = reg.get(&HookHandlerId::new(PLUGIN_PATCH_ID)).unwrap();
    assert_eq!(h.priority, 10);
    assert_eq!(h.display_name_i18n_key.as_deref(), Some("user.key"));
    assert_eq!(h.owner, HookHandlerOwner::User);
}

#[test]
fn user_can_disable_host_handler() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = write_user_toml(
        &dir,
        &format!(
            r#"
            [[handler]]
            id = "{HOST_NOTIFY_ID}"
            disabled = true
            "#
        ),
    );
    reg.install_user_config(&p);
    assert!(reg.handlers_for_source(TriggerSource::Webhook).is_empty());
    let h = reg.get(&HookHandlerId::new(HOST_NOTIFY_ID)).unwrap();
    assert!(h.disabled);
    assert_eq!(h.owner, HookHandlerOwner::User);
    assert!(matches!(h.action, HookHandlerAction::IpcSequence { .. }));
}

#[test]
fn user_patch_overrides_priority_only() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = write_user_toml(
        &dir,
        &format!(
            r#"
            [[handler]]
            id = "{HOST_NOTIFY_ID}"
            priority = 5
            "#
        ),
    );
    reg.install_user_config(&p);
    let h = reg.get(&HookHandlerId::new(HOST_NOTIFY_ID)).unwrap();
    assert_eq!(h.priority, 5);
    assert!(matches!(h.action, HookHandlerAction::IpcSequence { .. }));
}

#[test]
fn upsert_user_handler_adds_user_origin() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    reg.upsert_user_handler(UserHookHandlerUpsertDecl {
        id: "user/my-hook".into(),
        source: Some(HookSource::Any),
        priority: Some(15),
        display_name_i18n_key: None,
        disabled: None,
        action: Some(UserHookHandlerActionDecl::IpcSequence {
            calls: vec![IpcCall {
                method: "notification.create".into(),
                params: serde_json::json!({ "body": "hi" }),
            }],
        }),
    })
    .expect("upsert ok");
    let h = reg.get(&HookHandlerId::new("user/my-hook")).unwrap();
    assert_eq!(h.owner, HookHandlerOwner::User);
    assert_eq!(h.priority, 15);
}

/// 일부 필드 편집이 나머지 사용자 값을 지우지 않는지 확인한다.
#[test]
fn upsert_user_handler_keeps_fields_the_patch_did_not_mention() {
    let reg = HookHandlerRegistry::new();
    reg.upsert_user_handler(UserHookHandlerUpsertDecl {
        id: "user/keep".into(),
        source: Some(HookSource::Webhook),
        priority: Some(5),
        display_name_i18n_key: Some("k".into()),
        disabled: Some(false),
        action: Some(UserHookHandlerActionDecl::IpcSequence {
            calls: vec![IpcCall {
                method: "notification.create".into(),
                params: serde_json::json!({ "body": "one" }),
            }],
        }),
    })
    .expect("create ok");

    reg.upsert_user_handler(UserHookHandlerUpsertDecl {
        id: "user/keep".into(),
        source: None,
        priority: None,
        display_name_i18n_key: None,
        disabled: None,
        action: Some(UserHookHandlerActionDecl::IpcSequence {
            calls: vec![
                IpcCall {
                    method: "notification.create".into(),
                    params: serde_json::json!({ "body": "one" }),
                },
                IpcCall {
                    method: "notification.create".into(),
                    params: serde_json::json!({ "body": "two" }),
                },
            ],
        }),
    })
    .expect("edit ok");

    let h = reg
        .get(&HookHandlerId::new("user/keep"))
        .expect("편집한 핸들러가 조회에서 사라지면 안 된다");
    assert_eq!(h.source, HookSource::Webhook, "안 준 source 가 지워졌다");
    assert_eq!(h.priority, 5, "안 준 priority 가 지워졌다");
    assert_eq!(h.display_name_i18n_key.as_deref(), Some("k"));
    match &h.action {
        HookHandlerAction::IpcSequence { calls } => {
            assert_eq!(calls.len(), 2, "새 action이 반영돼야 한다")
        }
        other => panic!("expected ipc_sequence, got {other:?}"),
    }

    let toml = reg.export_user_config();
    assert!(
        toml.contains("webhook"),
        "영속 텍스트에 source 가 없다:\n{toml}"
    );
    assert!(
        toml.contains("priority"),
        "영속 텍스트에 priority 가 없다:\n{toml}"
    );
}

/// 기존 source를 유지한 채 셸 명령만 수정할 수 있어야 한다.
#[test]
fn upsert_user_handler_shell_edit_keeps_its_hook_source() {
    let reg = HookHandlerRegistry::new();
    reg.upsert_user_handler(UserHookHandlerUpsertDecl {
        id: "user/shell".into(),
        source: Some(HookSource::Hook),
        priority: None,
        display_name_i18n_key: None,
        disabled: None,
        action: Some(UserHookHandlerActionDecl::ShellCommand {
            command: "echo one".into(),
            args: Vec::new(),
        }),
    })
    .expect("create ok");

    reg.upsert_user_handler(UserHookHandlerUpsertDecl {
        id: "user/shell".into(),
        source: None,
        priority: None,
        display_name_i18n_key: None,
        disabled: None,
        action: Some(UserHookHandlerActionDecl::ShellCommand {
            command: "echo two".into(),
            args: Vec::new(),
        }),
    })
    .expect("source 를 다시 안 적어도 hook 이 이어져야 한다");

    let h = reg
        .get(&HookHandlerId::new("user/shell"))
        .expect("수정한 핸들러가 조회돼야 한다");
    assert_eq!(h.source, HookSource::Hook);
}

#[test]
fn upsert_user_handler_rejects_missing_owner_prefix() {
    let reg = HookHandlerRegistry::new();
    let err = reg
        .upsert_user_handler(UserHookHandlerUpsertDecl {
            id: "no-slash".into(),
            source: Some(HookSource::Any),
            priority: None,
            display_name_i18n_key: None,
            disabled: None,
            action: None,
        })
        .expect_err("missing prefix must reject");
    assert!(matches!(err, HookHandlerDeclError::InvalidShortName(_)));
}

#[test]
fn remove_and_clear_user_override() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    reg.set_user_handler_disabled(&HookHandlerId::new(HOST_NOTIFY_ID), true);
    assert!(
        reg.get(&HookHandlerId::new(HOST_NOTIFY_ID))
            .unwrap()
            .disabled
    );
    reg.clear_user_handler_override(&HookHandlerId::new(HOST_NOTIFY_ID));
    assert!(
        !reg.get(&HookHandlerId::new(HOST_NOTIFY_ID))
            .unwrap()
            .disabled
    );
}

#[test]
fn reload_user_config_replaces_user_keeps_host() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = write_user_toml(
        &dir,
        &format!(
            r#"
            [[handler]]
            id = "{HOST_NOTIFY_ID}"
            priority = 5
            "#
        ),
    );
    reg.install_user_config(&p);
    assert_eq!(
        reg.get(&HookHandlerId::new(HOST_NOTIFY_ID))
            .unwrap()
            .priority,
        5
    );

    std::fs::write(
        &p,
        r#"
        [[handler]]
        id = "user/fresh"
        source = "any"
        priority = 20
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = {} }]
        "#,
    )
    .unwrap();
    reg.reload_user_config(&p);

    assert_eq!(
        reg.get(&HookHandlerId::new(HOST_NOTIFY_ID))
            .unwrap()
            .priority,
        100
    );
    assert!(reg.contains(&HookHandlerId::new("user/fresh")));
}

#[test]
fn reload_parse_error_keeps_previous_state() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = write_user_toml(
        &dir,
        r#"
        [[handler]]
        id = "user/fresh"
        source = "any"
        priority = 20
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = {} }]
        "#,
    );
    reg.install_user_config(&p);
    assert!(reg.contains(&HookHandlerId::new("user/fresh")));

    std::fs::write(&p, "[[handler\n id = broken").unwrap();
    reg.reload_user_config(&p);
    assert!(reg.contains(&HookHandlerId::new("user/fresh")));
}

#[test]
fn handlers_for_source_gates_by_source() {
    let reg = HookHandlerRegistry::new();
    reg.install_host_defaults(
        r#"
        [[handler]]
        id = "hook-only"
        source = "hook"
        priority = 100
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = {} }]

        [[handler]]
        id = "webhook-only"
        source = "webhook"
        priority = 100
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = {} }]

        [[handler]]
        id = "both"
        source = "any"
        priority = 100
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = {} }]
        "#,
    );
    let hook_ids: Vec<String> = reg
        .handlers_for_source(TriggerSource::Hook)
        .iter()
        .map(|h| h.id.as_str().to_string())
        .collect();
    assert!(hook_ids.contains(&"host/hook-only".to_string()));
    assert!(hook_ids.contains(&"host/both".to_string()));
    assert!(!hook_ids.contains(&"host/webhook-only".to_string()));

    let wh_ids: Vec<String> = reg
        .handlers_for_source(TriggerSource::Webhook)
        .iter()
        .map(|h| h.id.as_str().to_string())
        .collect();
    assert!(wh_ids.contains(&"host/webhook-only".to_string()));
    assert!(wh_ids.contains(&"host/both".to_string()));
    assert!(!wh_ids.contains(&"host/hook-only".to_string()));
}

#[test]
fn shell_handler_bindable_to_hook_not_webhook() {
    let reg = HookHandlerRegistry::new();
    reg.install_host_defaults(
        r#"
        [[handler]]
        id = "sh"
        source = "hook"
        priority = 100
        [handler.action]
        kind = "shell_command"
        command = "echo"
        args = ["hi"]
        "#,
    );
    let h = reg.get(&HookHandlerId::new("host/sh")).unwrap();
    assert!(matches!(h.action, HookHandlerAction::ShellCommand { .. }));
    assert!(
        reg.handlers_for_source(TriggerSource::Hook)
            .iter()
            .any(|h| h.id.as_str() == "host/sh")
    );
    assert!(reg.handlers_for_source(TriggerSource::Webhook).is_empty());
}

#[test]
fn user_shell_with_non_hook_source_dropped_in_finalize() {
    let reg = HookHandlerRegistry::new();
    let dir = tempfile::tempdir().unwrap();
    let p = write_user_toml(
        &dir,
        r#"
        [[handler]]
        id = "user/bad-shell"
        source = "any"
        priority = 100
        [handler.action]
        kind = "shell_command"
        command = "echo"
        args = ["hi"]
        "#,
    );
    reg.install_user_config(&p);
    assert!(reg.get(&HookHandlerId::new("user/bad-shell")).is_none());
}

#[test]
fn upsert_full_handler_shell_must_be_hook_source() {
    let reg = HookHandlerRegistry::new();
    let err = reg
        .upsert_full_handler(HookHandler {
            id: HookHandlerId::new("user/x"),
            source: HookSource::Webhook,
            priority: 100,
            owner: HookHandlerOwner::User,
            action: HookHandlerAction::ShellCommand {
                command: "echo".into(),
                args: vec![],
            },
            display_name_i18n_key: None,
            disabled: false,
        })
        .expect_err("shell+webhook must reject");
    assert!(matches!(err, RegistryError::ShellMustBeHookSource { .. }));
}

#[test]
fn upsert_user_handler_shell_must_be_hook_source() {
    let reg = HookHandlerRegistry::new();
    let err = reg
        .upsert_user_handler(UserHookHandlerUpsertDecl {
            id: "user/sh".into(),
            source: Some(HookSource::Webhook),
            priority: None,
            display_name_i18n_key: None,
            disabled: None,
            action: Some(UserHookHandlerActionDecl::ShellCommand {
                command: "echo".into(),
                args: vec![],
            }),
        })
        .expect_err("shell+webhook must reject");
    assert!(matches!(
        err,
        HookHandlerDeclError::ShellMustBeHookSource { .. }
    ));
}

#[test]
fn validate_binding_rejects_source_mismatch_and_shell_webhook() {
    let reg = HookHandlerRegistry::new();
    reg.install_host_defaults(
        r#"
        [[handler]]
        id = "sh"
        source = "hook"
        priority = 100
        [handler.action]
        kind = "shell_command"
        command = "echo"
        "#,
    );
    let sh = reg.get(&HookHandlerId::new("host/sh")).unwrap();
    assert!(validate_binding(&sh, TriggerSource::Webhook).is_err());
    assert!(validate_binding(&sh, TriggerSource::Hook).is_ok());
}

#[test]
fn export_emits_only_user_origin() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = write_user_toml(
        &dir,
        &format!(
            r#"
            [[handler]]
            id = "{HOST_NOTIFY_ID}"
            disabled = true

            [[handler]]
            id = "user/my-hook"
            source = "any"
            priority = 20
            [handler.action]
            kind = "ipc_sequence"
            calls = [{{ method = "notification.create", params = {{}} }}]
            "#
        ),
    );
    reg.install_user_config(&p);
    let exported = reg.export_user_config();
    assert!(exported.contains(HOST_NOTIFY_ID));
    assert!(exported.contains("disabled = true"));
    assert!(exported.contains("user/my-hook"));
    let sections: Vec<&str> = exported.split("[[handler]]").collect();
    let host_section = sections
        .iter()
        .find(|s| s.contains(HOST_NOTIFY_ID))
        .expect("host section present");
    assert!(
        !host_section.contains("ipc_sequence"),
        "user export must not leak host action: {host_section}"
    );
}

#[test]
fn export_round_trip_preserves_user_handler() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = write_user_toml(
        &dir,
        r#"
        [[handler]]
        id = "user/my-hook"
        source = "webhook"
        priority = 25
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = { body = "hi" } }]
        "#,
    );
    reg.install_user_config(&p);
    let exported = reg.export_user_config();

    let reg2 = HookHandlerRegistry::new();
    load_host(&reg2);
    let p2 = dir.path().join("re-emit.toml");
    std::fs::write(&p2, &exported).unwrap();
    reg2.install_user_config(&p2);

    let h = reg2
        .get(&HookHandlerId::new("user/my-hook"))
        .expect("round-trip handler");
    assert_eq!(h.priority, 25);
    assert_eq!(h.source, HookSource::Webhook);
    assert!(matches!(h.action, HookHandlerAction::IpcSequence { .. }));
}

#[test]
fn save_user_config_atomic_write_creates_parent() {
    let reg = HookHandlerRegistry::new();
    let dir = tempfile::tempdir().unwrap();
    let src = write_user_toml(
        &dir,
        r#"
        [[handler]]
        id = "user/my-hook"
        source = "any"
        priority = 25
        [handler.action]
        kind = "ipc_sequence"
        calls = [{ method = "notification.create", params = {} }]
        "#,
    );
    reg.install_user_config(&src);
    let dst = dir.path().join("subdir").join("dst.toml");
    reg.save_user_config(&dst).unwrap();
    assert!(dst.exists());
    let written = std::fs::read_to_string(&dst).unwrap();
    assert!(written.contains("user/my-hook"));
    assert!(written.contains("ipc_sequence"));
}

#[test]
fn export_empty_when_no_user_contributions() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    assert_eq!(reg.export_user_config(), "");
}

// 전역 등록부를 사용하므로 다른 검사와 겹치지 않는 plugin ID만 조작한다.

#[test]
fn host_port_decodes_and_installs_plugin_hook_handlers() {
    use tasty_plugin_protocol::host_port::HookHandlerRegistryPort;
    let port = HostHookHandlerPort;
    let pid = "com.test.s11-port-ok";
    let handlers = vec![
        serde_json::json!({
            "id": "notify",
            "source": "webhook",
            "priority": 100,
            "action": {
                "kind": "ipc_sequence",
                "calls": [{ "method": "notification.create", "params": { "body": "hi" } }]
            }
        }),
        serde_json::json!({
            "id": "Bad_Name",
            "source": "hook",
            "priority": 10,
            "action": { "kind": "ipc_sequence", "calls": [] }
        }),
    ];
    port.install_plugin_hook_handlers(pid, &handlers);

    let good = HookHandlerId::new(format!("{pid}/notify"));
    let bad = HookHandlerId::new(format!("{pid}/Bad_Name"));
    assert!(global().contains(&good), "valid handler must be installed");
    assert!(
        !global().contains(&bad),
        "invalid short-name must be dropped, not installed"
    );

    port.uninstall_plugin(pid);
    assert!(!global().contains(&good));
}

#[test]
fn host_port_shell_command_json_is_rejected_by_type() {
    use tasty_plugin_protocol::host_port::HookHandlerRegistryPort;
    let port = HostHookHandlerPort;
    let pid = "com.test.s11-port-shell";
    let handlers = vec![serde_json::json!({
        "id": "sh",
        "source": "hook",
        "priority": 1,
        "action": { "kind": "shell_command", "command": "echo", "args": ["hi"] }
    })];
    port.install_plugin_hook_handlers(pid, &handlers);
    assert!(
        !global().contains(&HookHandlerId::new(format!("{pid}/sh"))),
        "plugin shell_command must not install (type-level exclusion)"
    );
}

#[test]
fn a_poisoned_registry_still_installs_and_lists() {
    let reg = std::sync::Arc::new(HookHandlerRegistry::new());

    let held = std::sync::Arc::clone(&reg);
    let joined = std::thread::spawn(move || {
        let _guard = held.inner.write().expect("fresh rwlock");
        panic!("a thread dies while holding the registry");
    })
    .join();
    assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
    assert!(reg.inner.read().is_err(), "poison 됐어야 한다");

    load_host(&reg);
    assert!(
        !reg.all_handlers().is_empty(),
        "poison 이후에도 host default 설치가 반영돼야 한다"
    );
}
