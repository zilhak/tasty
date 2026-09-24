//! `FileHandlerRegistry` 단위 테스트.

use super::*;
use tasty_file_format::DetectorId;

fn load_host(reg: &FileHandlerRegistry) {
    reg.install_host_defaults(include_str!("defaults/default-file-handlers.toml"));
}

fn install_markdown_plugin(reg: &FileHandlerRegistry) {
    let decls = vec![HandlerDecl::<PluginHandlerActionDecl> {
        id: "viewer".into(),
        detector: "markdown".into(),
        priority: 50,
        display_name_i18n_key: None,
        disabled: false,
        action: PluginHandlerActionDecl::OpenSurface {
            surface_kind: "markdown".into(),
            param_key: "file".into(),
        },
    }];
    reg.install_plugin_handlers("com.tasty.markdown", &decls);
}

const MD_VIEWER_ID: &str = "com.tasty.markdown/viewer";

#[test]
fn markdown_plugin_loads_handlers_for_markdown() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    install_markdown_plugin(&reg);
    let v = reg.handlers_for(&DetectorId("markdown".into()));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].id.as_str(), MD_VIEWER_ID);
    matches!(v[0].action, HandlerAction::OpenSurface { .. });
}

#[test]
fn plugin_handler_with_lower_priority_sorts_first() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    install_markdown_plugin(&reg);
    let decls = vec![HandlerDecl::<PluginHandlerActionDecl> {
        id: "viewer".into(),
        detector: "markdown".into(),
        priority: 10,
        display_name_i18n_key: None,
        disabled: false,
        action: PluginHandlerActionDecl::OpenSurface {
            surface_kind: "mdx_view".into(),
            param_key: "file".into(),
        },
    }];
    reg.install_plugin_handlers("com.example.mdx", &decls);
    let v = reg.handlers_for(&DetectorId("markdown".into()));
    assert_eq!(v.len(), 2);
    assert_eq!(v[0].id.as_str(), "com.example.mdx/viewer");
    assert_eq!(v[1].id.as_str(), MD_VIEWER_ID);
}

#[test]
fn user_can_disable_plugin_handler() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    install_markdown_plugin(&reg);
    let user_toml = format!(
        r#"
        [[handler]]
        id = "{MD_VIEWER_ID}"
        disabled = true
    "#
    );
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);
    let v = reg.handlers_for(&DetectorId("markdown".into()));
    assert!(v.is_empty());
}

#[test]
fn uninstall_plugin_removes_only_its_handlers() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    install_markdown_plugin(&reg);
    let decls = vec![HandlerDecl::<PluginHandlerActionDecl> {
        id: "viewer".into(),
        detector: "markdown".into(),
        priority: 10,
        display_name_i18n_key: None,
        disabled: false,
        action: PluginHandlerActionDecl::Ipc {
            method: "com.example.mdx.open".into(),
        },
    }];
    reg.install_plugin_handlers("com.example.mdx", &decls);
    assert_eq!(reg.handlers_for(&DetectorId("markdown".into())).len(), 2);
    reg.uninstall_plugin("com.example.mdx");
    let v = reg.handlers_for(&DetectorId("markdown".into()));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].id.as_str(), MD_VIEWER_ID);
}

#[test]
fn reload_user_config_replaces_user_handlers_keeps_plugin() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    install_markdown_plugin(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        format!(
            r#"
                [[handler]]
                id = "{MD_VIEWER_ID}"
                priority = 10
            "#
        ),
    )
    .unwrap();
    reg.install_user_config(&p);
    let v = reg.handlers_for(&DetectorId("markdown".into()));
    assert_eq!(v[0].priority, 10);

    std::fs::write(
        &p,
        r#"
            [[handler]]
            id = "user/my-md"
            detector = "markdown"
            priority = 20
            [handler.action]
            kind = "system"
        "#,
    )
    .unwrap();
    reg.reload_user_config(&p);

    let v = reg.handlers_for(&DetectorId("markdown".into()));
    let mdv = v.iter().find(|h| h.id.as_str() == MD_VIEWER_ID).unwrap();
    assert_eq!(mdv.priority, 50);
    assert!(v.iter().any(|h| h.id.as_str() == "user/my-md"));
}

#[test]
fn reload_user_config_parse_error_keeps_previous_state() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        r#"
            [[handler]]
            id = "user/my-md"
            detector = "markdown"
            priority = 20
            [handler.action]
            kind = "system"
        "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    assert!(
        reg.handlers_for(&DetectorId("markdown".into()))
            .iter()
            .any(|h| h.id.as_str() == "user/my-md")
    );

    std::fs::write(&p, "[[handler\n id = broken").unwrap();
    reg.reload_user_config(&p);
    assert!(
        reg.handlers_for(&DetectorId("markdown".into()))
            .iter()
            .any(|h| h.id.as_str() == "user/my-md")
    );
}

#[test]
fn handlers_for_priority_tiebreak_uses_owner_order() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    install_markdown_plugin(&reg);
    let p = vec![HandlerDecl::<PluginHandlerActionDecl> {
        id: "viewer".into(),
        detector: "markdown".into(),
        priority: 50,
        display_name_i18n_key: None,
        disabled: false,
        action: PluginHandlerActionDecl::Ipc {
            method: "com.example.x.open".into(),
        },
    }];
    reg.install_plugin_handlers("com.example.x", &p);
    let user_toml = r#"
        [[handler]]
        id = "user/my-viewer"
        detector = "markdown"
        priority = 50
        [handler.action]
        kind = "system"
    "#;
    let dir = tempfile::tempdir().unwrap();
    let pth = dir.path().join("file-handlers.toml");
    std::fs::write(&pth, user_toml).unwrap();
    reg.install_user_config(&pth);

    let v = reg.handlers_for(&DetectorId("markdown".into()));
    let owners: Vec<&str> = v.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(owners[0], "user/my-viewer");
    assert_eq!(owners[1], "com.example.x/viewer");
    assert_eq!(owners[2], MD_VIEWER_ID);
}

#[test]
fn all_handlers_returns_every_enabled() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let v = reg.all_handlers();
    assert_eq!(v.len(), 2);
}

use tasty_file_format::{DetectDepth, FileFormatRegistry, FileTarget};

fn make_user_toml(toml_text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, toml_text).unwrap();
    dir
}

#[test]
fn user_pdf_detector_and_handler_round_trip() {
    let formats = FileFormatRegistry::new();
    formats.install_host_defaults(tasty_file_format::HOST_DEFAULTS_TOML);

    let handlers = FileHandlerRegistry::new();
    load_host(&handlers);

    let user_toml = r#"
        [[detector]]
        id = "pdf"
        [[detector.rule]]
        kind = "extension"
        values = ["pdf"]

        [[handler]]
        id = "user/pdf-preview"
        detector = "pdf"
        priority = 30
        [handler.action]
        kind = "system"
    "#;
    let dir = make_user_toml(user_toml);
    let p = dir.path().join("file-handlers.toml");
    formats.install_user_config(&p);
    handlers.install_user_config(&p);

    let id = formats.identify(
        &FileTarget::new(std::path::PathBuf::from("docs/spec.pdf")),
        DetectDepth::Cheap,
    );
    assert_eq!(id, Some(tasty_file_format::DetectorId("pdf".into())));

    let v = handlers.handlers_for(&tasty_file_format::DetectorId("pdf".into()));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].id.as_str(), "user/pdf-preview");
    assert!(matches!(v[0].action, HandlerAction::System));
}

#[test]
fn export_user_handler_emits_only_user_origin() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    install_markdown_plugin(&reg);
    let user_toml = format!(
        r#"
        [[handler]]
        id = "{MD_VIEWER_ID}"
        disabled = true

        [[handler]]
        id = "user/my-md"
        detector = "markdown"
        priority = 20
        display_name_i18n_key = "user.md"
        [handler.action]
        kind = "system"
    "#
    );
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let exported = reg.export_user_config();
    assert!(exported.contains(MD_VIEWER_ID));
    assert!(exported.contains("disabled = true"));
    assert!(exported.contains("user/my-md"));
    assert!(exported.contains("\"markdown\""));
    let lines: Vec<&str> = exported.split("[[handler]]").collect();
    let md_section = lines
        .iter()
        .find(|s| s.contains(MD_VIEWER_ID))
        .expect("section present");
    assert!(
        !md_section.contains("kind = \"open_surface\""),
        "user export should not leak plugin action: {md_section}"
    );
}

#[test]
fn export_user_handler_round_trip() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let user_toml = r#"
        [[handler]]
        id = "user/my-md"
        detector = "markdown"
        priority = 25
        [handler.action]
        kind = "open_surface"
        surface_kind = "markdown"
        param_key = "file"
    "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let exported = reg.export_user_config();

    let reg2 = FileHandlerRegistry::new();
    load_host(&reg2);
    let p2 = dir.path().join("re-emit.toml");
    std::fs::write(&p2, &exported).unwrap();
    reg2.install_user_config(&p2);

    let v1 = reg.handlers_for(&DetectorId("markdown".into()));
    let v2 = reg2.handlers_for(&DetectorId("markdown".into()));
    let ids1: Vec<_> = v1.iter().map(|h| h.id.as_str().to_string()).collect();
    let ids2: Vec<_> = v2.iter().map(|h| h.id.as_str().to_string()).collect();
    assert_eq!(ids1, ids2);
    let h2 = v2
        .iter()
        .find(|h| h.id.as_str() == "user/my-md")
        .expect("user handler present");
    assert_eq!(h2.priority, 25);
}

#[test]
fn save_user_handler_atomic_write() {
    let reg = FileHandlerRegistry::new();
    let user_toml = r#"
        [[handler]]
        id = "user/my-md"
        detector = "markdown"
        priority = 25
        [handler.action]
        kind = "system"
    "#;
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.toml");
    std::fs::write(&src, user_toml).unwrap();
    reg.install_user_config(&src);

    let dst = dir.path().join("subdir").join("dst.toml");
    reg.save_user_config(&dst).unwrap();
    assert!(dst.exists());
    let written = std::fs::read_to_string(&dst).unwrap();
    assert!(written.contains("user/my-md"));
    assert!(written.contains("kind = \"system\""));
}

#[test]
fn export_empty_when_no_user_contributions() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    assert_eq!(reg.export_user_config(), "");
}

#[test]
fn directory_target_does_not_match_file_detectors() {
    let formats = FileFormatRegistry::new();
    formats.install_host_defaults(tasty_file_format::HOST_DEFAULTS_TOML);
    let handlers = FileHandlerRegistry::new();
    load_host(&handlers);

    let dir = tempfile::tempdir().unwrap();
    let target = FileTarget::new(dir.path().to_path_buf());
    let id = formats
        .identify(&target, DetectDepth::Cheap)
        .expect("directory should identify");
    assert_eq!(id.as_str(), "$directory");
    let v = handlers.handlers_for(&id);
    assert!(!v.is_empty(), "host should register a directory handler");
}

fn install_markdown_plugin_detector(formats: &tasty_file_format::FileFormatRegistry) {
    use tasty_file_format::{DetectorDecl, DetectorRuleDecl};
    let decls = vec![DetectorDecl {
        id: "markdown".into(),
        display_name_i18n_key: Some("file_handler.format.markdown".into()),
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::Extension {
            values: vec!["md".into(), "markdown".into()],
        }],
    }];
    formats.install_plugin_detectors("com.tasty.markdown", &decls);
}

#[test]
fn attach_detector_info_stores_arc_and_returns_clone() {
    use tasty_file_format::FileFormatRegistry;
    let formats = std::sync::Arc::new(FileFormatRegistry::new());
    formats.install_host_defaults(tasty_file_format::HOST_DEFAULTS_TOML);
    install_markdown_plugin_detector(&formats);

    let handlers = FileHandlerRegistry::new();
    assert!(handlers.detector_info().is_none());

    handlers.attach_detector_info(formats.clone());
    let info = handlers
        .detector_info()
        .expect("detector_info should be Some after attach");
    let exts = info.advertised_extensions(&DetectorId("markdown".into()));
    assert!(exts.contains(&"md".to_string()));
}

fn markdown_detector_json() -> Vec<serde_json::Value> {
    vec![serde_json::json!({
        "id": "markdown",
        "display_name_i18n_key": "file_handler.format.markdown",
        "rule": [{ "kind": "extension", "values": ["md", "markdown"] }],
    })]
}

fn markdown_handler_json() -> Vec<serde_json::Value> {
    vec![serde_json::json!({
        "id": "viewer",
        "detector": "markdown",
        "priority": 50,
        "display_name_i18n_key": "file_handler.host.markdown-viewer",
        "action": { "kind": "open_surface", "surface_kind": "markdown", "param_key": "file" },
    })]
}

#[test]
fn boot_registration_via_manifest_json_enables_dispatch() {
    use tasty_plugin_protocol::host_port::{FileFormatRegistryPort, FileHandlerRegistryPort};
    let formats = FileFormatRegistry::new();
    formats.install_host_defaults(tasty_file_format::HOST_DEFAULTS_TOML);
    let handlers = FileHandlerRegistry::new();
    load_host(&handlers);

    FileFormatRegistryPort::install_plugin_detectors(
        &formats,
        "com.tasty.markdown",
        &markdown_detector_json(),
    );
    FileHandlerRegistryPort::install_plugin_handlers(
        &handlers,
        "com.tasty.markdown",
        &markdown_handler_json(),
    );

    let id = formats.identify(
        &FileTarget::new(std::path::PathBuf::from("README.md")),
        DetectDepth::Cheap,
    );
    assert_eq!(id, Some(DetectorId("markdown".into())));
    let v = handlers.handlers_for(&DetectorId("markdown".into()));
    assert!(v.iter().any(|h| h.id.as_str() == MD_VIEWER_ID));
}

#[test]
fn boot_registration_idempotent_and_first_is_deterministic() {
    use tasty_plugin_protocol::host_port::FileHandlerRegistryPort;
    let handlers = FileHandlerRegistry::new();
    load_host(&handlers);

    let other = vec![serde_json::json!({
        "id": "viewer",
        "detector": "markdown",
        "priority": 50,
        "action": { "kind": "ipc", "method": "com.example.mdx.open" },
    })];

    FileHandlerRegistryPort::install_plugin_handlers(
        &handlers,
        "com.tasty.markdown",
        &markdown_handler_json(),
    );
    FileHandlerRegistryPort::install_plugin_handlers(&handlers, "com.example.mdx", &other);
    let first_id = handlers.handlers_for(&DetectorId("markdown".into()))[0]
        .id
        .as_str()
        .to_string();

    FileHandlerRegistryPort::install_plugin_handlers(
        &handlers,
        "com.tasty.markdown",
        &markdown_handler_json(),
    );
    FileHandlerRegistryPort::install_plugin_handlers(&handlers, "com.example.mdx", &other);

    let v = handlers.handlers_for(&DetectorId("markdown".into()));
    assert_eq!(v.len(), 2, "재install 후에도 핸들러 2개 — 중복 누적 없음");
    assert_eq!(v[0].id.as_str(), first_id);
    assert_eq!(v[0].id.as_str(), "com.example.mdx/viewer");
}

#[test]
fn attach_detector_info_second_call_is_ignored() {
    use tasty_file_format::FileFormatRegistry;
    let formats_a = std::sync::Arc::new(FileFormatRegistry::new());
    let formats_b = std::sync::Arc::new(FileFormatRegistry::new());
    formats_a.install_host_defaults(tasty_file_format::HOST_DEFAULTS_TOML);
    install_markdown_plugin_detector(&formats_a);

    let handlers = FileHandlerRegistry::new();
    handlers.attach_detector_info(formats_a.clone());
    handlers.attach_detector_info(formats_b.clone());

    let info = handlers.detector_info().expect("Some after first attach");
    let exts = info.advertised_extensions(&DetectorId("markdown".into()));
    assert!(!exts.is_empty(), "first registry should still be attached");
}

#[test]
fn a_poisoned_registry_still_installs_and_resolves() {
    let reg = std::sync::Arc::new(FileHandlerRegistry::new());

    let held = std::sync::Arc::clone(&reg);
    let joined = std::thread::spawn(move || {
        let _guard = held.inner.write().expect("fresh rwlock");
        panic!("a thread dies while holding the registry");
    })
    .join();
    assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
    assert!(reg.inner.read().is_err(), "poison 됐어야 한다");

    load_host(&reg);
    install_markdown_plugin(&reg);

    assert!(
        reg.handler(&HandlerId(MD_VIEWER_ID.to_string())).is_some(),
        "poison 이후에도 설치가 반영돼야 한다"
    );
    assert!(
        !reg.all_handlers().is_empty(),
        "poison 이후에도 조회가 빈 결과가 아니어야 한다"
    );
}

#[test]
fn reload_user_config_reports_the_entries_it_dropped() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        r#"
            [[handler]]
            id = "md-as-html"
            detector = "markdown"
            [handler.action]
            kind = "system"

            [[handler]]
            id = "user/no-action"
            detector = "markdown"

            [[handler]]
            id = "user/good"
            detector = "markdown"
            [handler.action]
            kind = "system"

            [[handler]]
            id = "host/html-system"
            priority = 7
        "#,
    )
    .unwrap();

    let rejected = reg.reload_user_config(&p);

    assert_eq!(
        rejected,
        vec![
            RejectedUserHandler {
                id: "md-as-html".into(),
                reason: UserHandlerRejectReason::MissingOwnerPrefix,
            },
            RejectedUserHandler {
                id: "user/no-action".into(),
                reason: UserHandlerRejectReason::MissingDetectorOrAction,
            },
        ]
    );
    let ids: Vec<String> = reg
        .all_handlers()
        .into_iter()
        .map(|h| h.id.as_str().to_string())
        .collect();
    assert!(
        !ids.iter()
            .any(|i| i == "user/no-action" || i == "md-as-html")
    );
    assert!(ids.iter().any(|i| i == "user/good"));
    let patched = reg
        .get(&HandlerId("host/html-system".into()))
        .expect("host handler 는 patch 로 살아 있다");
    assert_eq!(patched.priority, 7);
}

#[test]
fn reload_user_config_reports_nothing_when_nothing_was_dropped() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        r#"
            [[handler]]
            id = "user/good"
            detector = "markdown"
            [handler.action]
            kind = "system"
        "#,
    )
    .unwrap();
    assert!(reg.reload_user_config(&p).is_empty());

    std::fs::write(&p, "[[handler\n id = broken").unwrap();
    assert!(reg.reload_user_config(&p).is_empty());
}

#[test]
fn reject_reason_codes_are_stable() {
    assert_eq!(
        UserHandlerRejectReason::MissingOwnerPrefix.as_str(),
        "missing_owner_prefix"
    );
    assert_eq!(
        UserHandlerRejectReason::MissingDetectorOrAction.as_str(),
        "missing_detector_or_action"
    );
    assert_eq!(
        UserHandlerRejectReason::TargetNotContributed.as_str(),
        "target_not_contributed"
    );
}

#[test]
fn a_patch_whose_plugin_has_not_contributed_is_reported_as_target_not_contributed() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        format!(
            r#"
                [[handler]]
                id = "{MD_VIEWER_ID}"
                priority = 10

                [[handler]]
                id = "com.tasty.markdown/viewer-typo"
                priority = 10
            "#
        ),
    )
    .unwrap();

    let rejected = reg.reload_user_config(&p);

    assert_eq!(
        rejected,
        vec![
            RejectedUserHandler {
                id: MD_VIEWER_ID.into(),
                reason: UserHandlerRejectReason::TargetNotContributed,
            },
            RejectedUserHandler {
                id: "com.tasty.markdown/viewer-typo".into(),
                reason: UserHandlerRejectReason::TargetNotContributed,
            },
        ]
    );
}

#[test]
fn a_patch_leaves_the_report_once_its_plugin_contributes() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        format!(
            r#"
                [[handler]]
                id = "{MD_VIEWER_ID}"
                priority = 10

                [[handler]]
                id = "com.tasty.markdown/viewer-typo"
                priority = 10
            "#
        ),
    )
    .unwrap();
    assert_eq!(reg.reload_user_config(&p).len(), 2);

    install_markdown_plugin(&reg);
    let viewer = reg
        .get(&HandlerId(MD_VIEWER_ID.into()))
        .expect("plugin 이 contribute 한 뒤에는 handler 가 있다");
    assert_eq!(
        viewer.priority, 10,
        "plugin 이 나중에 와도 user patch 의 priority 가 이겨야 한다"
    );
    let rejected = reg.reload_user_config(&p);

    assert_eq!(
        rejected,
        vec![RejectedUserHandler {
            id: "com.tasty.markdown/viewer-typo".into(),
            reason: UserHandlerRejectReason::TargetNotContributed,
        }]
    );
    let viewer = reg
        .get(&HandlerId(MD_VIEWER_ID.into()))
        .expect("plugin 이 contribute 한 뒤에는 patch 가 적용된 handler 가 있다");
    assert_eq!(viewer.priority, 10);
}

#[test]
fn a_user_patch_wins_over_a_plugin_installed_after_the_boot_load() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        format!(
            r#"
                [[handler]]
                id = "{MD_VIEWER_ID}"
                priority = 10
            "#
        ),
    )
    .unwrap();

    reg.install_user_config(&p);
    install_markdown_plugin(&reg);

    let viewer = reg.get(&HandlerId(MD_VIEWER_ID.into())).unwrap();
    assert_eq!(
        viewer.priority, 10,
        "user patch 가 plugin 기본값 50 을 덮어야 한다"
    );
    assert!(matches!(viewer.owner, HandlerOwner::User));
}

#[test]
fn a_user_patch_wins_over_a_plugin_that_contributes_later_without_a_reload() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    install_markdown_plugin(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        format!(
            r#"
                [[handler]]
                id = "{MD_VIEWER_ID}"
                priority = 10
            "#
        ),
    )
    .unwrap();
    assert!(reg.reload_user_config(&p).is_empty());

    reg.uninstall_plugin("com.tasty.markdown");
    install_markdown_plugin(&reg);

    let viewer = reg.get(&HandlerId(MD_VIEWER_ID.into())).unwrap();
    assert_eq!(
        viewer.priority, 10,
        "다시 contribute 한 plugin 이 user patch 를 덮으면 안 된다"
    );
}
