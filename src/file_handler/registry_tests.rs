//! `FileHandlerRegistry` 단위 테스트.

#![cfg(test)]

use super::*;
use crate::file_format::DetectorId;

fn load_host(reg: &FileHandlerRegistry) {
    reg.install_host_defaults(include_str!("defaults/default-file-handlers.toml"));
}

#[test]
fn host_default_loads_handlers_for_markdown() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let v = reg.handlers_for(&DetectorId("markdown".into()));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].id.as_str(), "host/markdown-viewer");
    matches!(v[0].action, HandlerAction::OpenSurface { .. });
}

#[test]
fn plugin_handler_with_lower_priority_sorts_first() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
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
    assert_eq!(v[1].id.as_str(), "host/markdown-viewer");
}

#[test]
fn user_can_disable_host_handler() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let user_toml = r#"
        [[handler]]
        id = "host/markdown-viewer"
        disabled = true
    "#;
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
    assert_eq!(v[0].id.as_str(), "host/markdown-viewer");
}

#[test]
fn reload_user_config_replaces_user_handlers_keeps_host() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    // 1차: user 가 markdown-viewer priority 만 override.
    std::fs::write(
        &p,
        r#"
            [[handler]]
            id = "host/markdown-viewer"
            priority = 10
        "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    let v = reg.handlers_for(&DetectorId("markdown".into()));
    assert_eq!(v[0].priority, 10);

    // 2차: user 가 priority override 빼고 새 user/handler 추가 → reload.
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
    // host/markdown-viewer 는 호스트 default priority (= 50) 로 복귀.
    let mdv = v.iter().find(|h| h.id.as_str() == "host/markdown-viewer").unwrap();
    assert_eq!(mdv.priority, 50);
    // user/my-md 가 잡혀야 함.
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
    assert!(reg
        .handlers_for(&DetectorId("markdown".into()))
        .iter()
        .any(|h| h.id.as_str() == "user/my-md"));

    // 파일을 깨뜨림 → reload 거부, 기존 user 항목 보존.
    std::fs::write(&p, "[[handler\n id = broken").unwrap();
    reg.reload_user_config(&p);
    assert!(reg
        .handlers_for(&DetectorId("markdown".into()))
        .iter()
        .any(|h| h.id.as_str() == "user/my-md"));
}

#[test]
fn handlers_for_priority_tiebreak_uses_owner_order() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    // plugin 과 user 모두 priority 50 (host 도 50)
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
    // priority 모두 50 → tie-break: user > plugin > host
    let owners: Vec<&str> = v.iter().map(|h| h.id.as_str()).collect();
    assert_eq!(owners[0], "user/my-viewer");
    assert_eq!(owners[1], "com.example.x/viewer");
    assert_eq!(owners[2], "host/markdown-viewer");
}

#[test]
fn all_handlers_returns_every_enabled() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    let v = reg.all_handlers();
    // host default 4개
    assert_eq!(v.len(), 4);
}

// ── cross-module integration: file_format + file_handler ──────────
//
// 시나리오: 사용자가 PDF 로 새 detector 와 핸들러를 등록한 뒤
// 1) `identify(*.pdf)` 가 user detector 를 반환하고
// 2) `handlers_for(pdf)` 가 user handler 를 반환하는지 확인.

use crate::file_format::{
    DetectDepth, FileFormatRegistry, FileTarget,
};

fn make_user_toml(toml_text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, toml_text).unwrap();
    dir
}

#[test]
fn user_pdf_detector_and_handler_round_trip() {
    let formats = FileFormatRegistry::new();
    formats.install_host_defaults(include_str!(
        "../file_format/defaults/default-file-format.toml"
    ));

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

    // identify
    let id = formats.identify(
        &FileTarget::new(std::path::PathBuf::from("docs/spec.pdf")),
        DetectDepth::Cheap,
    );
    assert_eq!(id, Some(crate::file_format::DetectorId("pdf".into())));

    // handlers_for
    let v = handlers.handlers_for(&crate::file_format::DetectorId("pdf".into()));
    assert_eq!(v.len(), 1);
    assert_eq!(v[0].id.as_str(), "user/pdf-preview");
    assert!(matches!(v[0].action, HandlerAction::System));
}

// ── export_user_config / save_user_config (MD4) ─────────────────────

#[test]
fn export_user_handler_emits_only_user_origin() {
    let reg = FileHandlerRegistry::new();
    load_host(&reg);
    // 사용자가 host/markdown-viewer 를 disable 하고 자기 핸들러 user/my-md 추가.
    let user_toml = r#"
        [[handler]]
        id = "host/markdown-viewer"
        disabled = true

        [[handler]]
        id = "user/my-md"
        detector = "markdown"
        priority = 20
        display_name_i18n_key = "user.md"
        [handler.action]
        kind = "system"
    "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let exported = reg.export_user_config();
    assert!(exported.contains("host/markdown-viewer"));
    assert!(exported.contains("disabled = true"));
    assert!(exported.contains("user/my-md"));
    assert!(exported.contains("\"markdown\""));
    // host default 의 markdown-viewer action(OpenSurface) 는 user 가 손대지 않았으므로
    // export 결과의 host/markdown-viewer entry 에는 action 이 없어야 한다.
    let lines: Vec<&str> = exported.split("[[handler]]").collect();
    let md_section = lines
        .iter()
        .find(|s| s.contains("host/markdown-viewer"))
        .expect("section present");
    assert!(
        !md_section.contains("kind = \"open_surface\""),
        "user export should not leak host action: {md_section}"
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
    // user/my-md 의 priority 가 보존되었는지
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
    formats.install_host_defaults(include_str!(
        "../file_format/defaults/default-file-format.toml"
    ));
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

// ── DetectorInfo 주입 (Phase E ME1) ──────────────────────────────

#[test]
fn attach_detector_info_stores_arc_and_returns_clone() {
    use crate::file_format::FileFormatRegistry;
    let formats = std::sync::Arc::new(FileFormatRegistry::new());
    formats.install_host_defaults(include_str!(
        "../file_format/defaults/default-file-format.toml"
    ));

    let handlers = FileHandlerRegistry::new();
    assert!(handlers.detector_info().is_none());

    handlers.attach_detector_info(formats.clone());
    let info = handlers
        .detector_info()
        .expect("detector_info should be Some after attach");
    // 주입된 info 로 markdown 의 광고된 확장자 조회 가능.
    let exts = info.advertised_extensions(&DetectorId("markdown".into()));
    assert!(exts.contains(&"md".to_string()));
}

#[test]
fn attach_detector_info_second_call_is_ignored() {
    use crate::file_format::FileFormatRegistry;
    let formats_a = std::sync::Arc::new(FileFormatRegistry::new());
    let formats_b = std::sync::Arc::new(FileFormatRegistry::new());
    formats_a.install_host_defaults(include_str!(
        "../file_format/defaults/default-file-format.toml"
    ));
    // formats_b 는 host default 안 깐 빈 registry.

    let handlers = FileHandlerRegistry::new();
    handlers.attach_detector_info(formats_a.clone());
    // 2번째 호출은 무시 → formats_b 가 주입되지 않음.
    handlers.attach_detector_info(formats_b.clone());

    let info = handlers.detector_info().expect("Some after first attach");
    // 첫번째 (formats_a) 가 보유한 markdown 광고가 보여야 함.
    let exts = info.advertised_extensions(&DetectorId("markdown".into()));
    assert!(!exts.is_empty(), "first registry should still be attached");
}
