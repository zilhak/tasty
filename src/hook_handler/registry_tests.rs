//! `HookHandlerRegistry` 단위 테스트 (파일 핸들러 `registry_tests.rs` 미러).
//!
//! 3출처 병합(host embedded TOML + plugin + user config) · patch semantics ·
//! owner tie-break · lazy finalize · source 게이트 · 셸 불변식 · user config
//! export/save/reload 를 커버한다.

use super::*;
use crate::hook_handler::types::{IpcCall, validate_binding};

/// host embedded default 를 install 한다. 기본 핸들러 = `host/webhook-notify`
/// (source=webhook, ipc_sequence).
fn load_host(reg: &HookHandlerRegistry) {
    reg.install_host_defaults(include_str!("defaults/default-hook-handlers.toml"));
}

const HOST_NOTIFY_ID: &str = "host/webhook-notify";

fn plugin_ipc(short: &str, source: HookSource, priority: i32, method: &str) -> HookHandlerDecl<PluginHookHandlerActionDecl> {
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

// ── host defaults ────────────────────────────────────────────────────────

#[test]
fn host_defaults_load() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let h = reg.get(&HookHandlerId::new(HOST_NOTIFY_ID)).expect("host handler");
    assert_eq!(h.source, HookSource::Webhook);
    assert_eq!(h.owner, HookHandlerOwner::Host);
    assert!(matches!(h.action, HookHandlerAction::IpcSequence { .. }));
    assert!(reg.contains(&HookHandlerId::new(HOST_NOTIFY_ID)));
}

#[test]
fn all_handlers_returns_every_enabled() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    // host default 1개 (webhook-notify).
    assert_eq!(reg.all_handlers().len(), 1);
    assert_eq!(reg.list_handlers(), vec![HookHandlerId::new(HOST_NOTIFY_ID)]);
}

// ── plugin install / 정렬 ─────────────────────────────────────────────────

#[test]
fn plugin_install_and_lower_priority_sorts_first() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    reg.install_plugin_handlers(
        "com.example.hook",
        &[plugin_ipc("relay", HookSource::Webhook, 10, "notification.create")],
    );
    let v = reg.handlers_for_source(TriggerSource::Webhook);
    assert_eq!(v.len(), 2);
    // priority 10 < 100 → plugin 먼저.
    assert_eq!(v[0].id.as_str(), "com.example.hook/relay");
    assert_eq!(v[1].id.as_str(), HOST_NOTIFY_ID);
}

#[test]
fn uninstall_plugin_removes_only_its_handlers() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    reg.install_plugin_handlers(
        "com.example.hook",
        &[plugin_ipc("relay", HookSource::Any, 20, "notification.create")],
    );
    assert_eq!(reg.all_handlers().len(), 2);
    reg.uninstall_plugin("com.example.hook");
    let v = reg.all_handlers();
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].id.as_str(), HOST_NOTIFY_ID);
}

#[test]
fn plugin_reinstall_is_idempotent() {
    let reg = HookHandlerRegistry::new();
    load_host(&reg);
    let decls = [plugin_ipc("relay", HookSource::Any, 30, "notification.create")];
    reg.install_plugin_handlers("com.example.hook", &decls);
    reg.install_plugin_handlers("com.example.hook", &decls);
    // 같은 owner 재install → retain 으로 교체, 중복 누적 없음.
    assert_eq!(reg.all_handlers().len(), 2);
}

// ── owner tie-break (user > plugin > host) ────────────────────────────────

#[test]
fn owner_tiebreak_user_gt_plugin_gt_host() {
    let reg = HookHandlerRegistry::new();
    // host handler priority 를 50 으로 맞추기 위해 별도 host toml 로 install.
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
        &[plugin_ipc("same", HookSource::Any, 50, "notification.create")],
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
    // priority 모두 50 → tie-break user > plugin > host.
    assert_eq!(ids[0], "user/same");
    assert_eq!(ids[1], "com.example.hook/same");
    assert_eq!(ids[2], "host/same");
}

// ── user override (patch semantics) ───────────────────────────────────────

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
    // 활성 목록에서 사라진다.
    assert!(reg.handlers_for_source(TriggerSource::Webhook).is_empty());
    // 하지만 get() 은 여전히 (disabled=true) 반환.
    let h = reg.get(&HookHandlerId::new(HOST_NOTIFY_ID)).unwrap();
    assert!(h.disabled);
    // patch semantics: 마지막 출처(user)가 owner 를 이긴다(파일 핸들러
    // registry.rs 선례와 동일 — user override 시 owner=User → tie-break 우선).
    assert_eq!(h.owner, HookHandlerOwner::User);
    // 원 action 은 host 것이 보존된다(patch 로 덮이지 않음).
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
    // action 은 host 것 보존.
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
    assert!(reg.get(&HookHandlerId::new(HOST_NOTIFY_ID)).unwrap().disabled);
    // clear override → host 기본(enabled) 로 복귀.
    reg.clear_user_handler_override(&HookHandlerId::new(HOST_NOTIFY_ID));
    assert!(!reg.get(&HookHandlerId::new(HOST_NOTIFY_ID)).unwrap().disabled);
}

// ── reload ────────────────────────────────────────────────────────────────

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
    assert_eq!(reg.get(&HookHandlerId::new(HOST_NOTIFY_ID)).unwrap().priority, 5);

    // 2차: user override 제거하고 새 user 핸들러 추가 → reload.
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

    // host 는 default priority (=100) 로 복귀.
    assert_eq!(reg.get(&HookHandlerId::new(HOST_NOTIFY_ID)).unwrap().priority, 100);
    // user/fresh 등장.
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
    // 파싱 실패 → 기존 user 항목 보존.
    assert!(reg.contains(&HookHandlerId::new("user/fresh")));
}

// ── source 게이트 ──────────────────────────────────────────────────────────

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

// ── 셸 불변식 (구조적 강제) ────────────────────────────────────────────────

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
    // hook 트리거엔 잡히고, webhook 트리거엔 안 잡힌다.
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
    // user config 는 parse 단계에서 셸 게이트가 없다 → finalize 가 구조적으로 drop.
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
    // 셸 + non-hook source → finalize 에서 drop → 조회 불가.
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
    assert!(matches!(err, HookHandlerDeclError::ShellMustBeHookSource { .. }));
}

// ── validate_binding (types) ───────────────────────────────────────────────

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
    // hook 전용 → webhook 바인딩 거부(source mismatch).
    assert!(validate_binding(&sh, TriggerSource::Webhook).is_err());
    // hook 트리거엔 OK.
    assert!(validate_binding(&sh, TriggerSource::Hook).is_ok());
}

// ── export / save ──────────────────────────────────────────────────────────

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
    // host handler 의 원 action(host default) 은 user 가 손대지 않았으므로
    // export 의 host 엔트리엔 action 이 없어야 한다(action leak 금지).
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

    let h = reg2.get(&HookHandlerId::new("user/my-hook")).expect("round-trip handler");
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
