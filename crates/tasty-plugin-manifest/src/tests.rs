//! Manifest 파싱/검증 단위 테스트.

use super::*;

fn parse(src: &str) -> anyhow::Result<Manifest> {
    let m: Manifest = toml::from_str(src)?;
    m.validate()?;
    Ok(m)
}

#[test]
fn rejects_unsupported_api() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "999"
        [entry]
        type = "process"
        command = "x"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn rejects_unsupported_manifest_version() {
    let s = r#"
        manifest_version = 99
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn rejects_invalid_plugin_id_no_dot() {
    let s = r#"
        manifest_version = 1
        id = "explorer"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn rejects_invalid_kind_uppercase() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[surface_kinds]]
        kind = "Explorer"
        display_name_i18n_key = "k"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn accepts_minimal_valid() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1.0"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.id, "com.example.x");
}

#[test]
fn rejects_unknown_permission() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        permissions = ["fs.read", "made.up.permission"]
        [entry]
        type = "process"
        command = "x"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("unknown permission"), "got: {err}");
}

#[test]
fn parsed_permissions_returns_enum_set() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        permissions = ["fs.read", "surface.write", "notification"]
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    let perms = m.parsed_permissions().expect("should resolve");
    assert!(perms.contains(&Permission::FsRead));
    assert!(perms.contains(&Permission::SurfaceWrite));
    assert!(perms.contains(&Permission::Notification));
    assert_eq!(perms.len(), 3);
}

#[test]
fn accepts_full_manifest() {
    // TOML rule: top-level keys must come before any table headers.
    let s = r#"
        manifest_version = 1
        id = "com.example.explorer"
        name = "Explorer"
        version = "1.2.0"
        authors = ["alice@example.com"]
        description = "File explorer"
        homepage = "https://example.com"
        api_version = "1"
        permissions = ["fs.read", "surface.read"]

        [entry]
        type = "process"
        command = "tasty-plugin-explorer"
        args = []

        [[surface_kinds]]
        kind = "explorer"
        display_name_i18n_key = "surface.kind.explorer"
        icon = "📁"

        [[contributes.commands]]
        id = "explorer.refresh"
        title_i18n_key = "explorer.command.refresh"
        default_keybinding = "F5"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.surface_kinds.len(), 1);
    assert_eq!(m.surface_kinds[0].kind, "explorer");
    assert_eq!(m.permissions.len(), 2);
    assert_eq!(m.contributes.commands.len(), 1);
    // binding_mode 미지정 → Independent 기본값
    assert_eq!(
        m.contributes.commands[0].binding_mode,
        BindingMode::Independent
    );
    // lang_dir 미지정 → "lang" 기본값
    assert_eq!(m.lang_dir, "lang");
}

#[test]
fn binding_mode_independent() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[contributes.commands]]
        id = "x.foo"
        title_i18n_key = "x.foo"
        binding_mode = "independent"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(
        m.contributes.commands[0].binding_mode,
        BindingMode::Independent
    );
}

#[test]
fn binding_mode_inherit() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[contributes.commands]]
        id = "x.copy"
        title_i18n_key = "x.copy"
        binding_mode = "inherit:clipboard.copy"
    "#;
    let m = parse(s).expect("should parse");
    match &m.contributes.commands[0].binding_mode {
        BindingMode::InheritHost(action) => assert_eq!(action, "clipboard.copy"),
        _ => panic!("expected InheritHost"),
    }
}

#[test]
fn binding_mode_inherit_empty_action_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[contributes.commands]]
        id = "x.copy"
        title_i18n_key = "x.copy"
        binding_mode = "inherit:"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn binding_mode_unknown_value_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[contributes.commands]]
        id = "x.foo"
        title_i18n_key = "x.foo"
        binding_mode = "wat"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn manifest_with_ipc_namespace_parses() {
    let s = r#"
        manifest_version = 1
        id = "com.example.codex"
        name = "Codex"
        version = "0.1.0"
        api_version = "1"
        [entry]
        type = "process"
        command = "tasty-plugin-codex"
        [[contributes.ipc_namespace]]
        prefix = "codex"
        description_i18n_key = "codex.namespace.desc"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.contributes.ipc_namespace.len(), 1);
    assert_eq!(m.contributes.ipc_namespace[0].prefix, "codex");
}

#[test]
fn manifest_with_cli_parses() {
    let s = r#"
        manifest_version = 1
        id = "com.example.codex"
        name = "Codex"
        version = "0.1.0"
        api_version = "1"

        [entry]
        type = "process"
        command = "tasty-plugin-codex"

        [[contributes.ipc_namespace]]
        prefix = "codex"

        [[contributes.cli]]
        name = "codex"
        subcommands = [
          { name = "spawn", ipc_method = "codex.spawn", args = "spawn_args" },
          { name = "wait",  ipc_method = "codex.wait",  args = "no_args" },
        ]

        [contributes.cli.arg_groups.spawn_args]
        flags = [
          { name = "surface", type = "u32",    flag = "--surface", required = false },
          { name = "prompt",  type = "string", flag = "--prompt",  required = false },
        ]

        [contributes.cli.arg_groups.no_args]
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.contributes.cli.len(), 1);
    let cli = &m.contributes.cli[0];
    assert_eq!(cli.name, "codex");
    assert_eq!(cli.subcommands.len(), 2);
    assert!(cli.arg_groups.contains_key("spawn_args"));
    assert!(cli.arg_groups.contains_key("no_args"));
    let spawn_args = &cli.arg_groups["spawn_args"];
    assert_eq!(spawn_args.flags.len(), 2);
    assert_eq!(spawn_args.flags[0].ty, CliArgType::U32);
}

#[test]
fn manifest_reserved_ipc_prefix_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.evil"
        name = "Evil"
        version = "0.1.0"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[contributes.ipc_namespace]]
        prefix = "surface"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("reserved"), "got: {err}");
}

#[test]
fn manifest_reserved_cli_name_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.evil"
        name = "Evil"
        version = "0.1.0"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[contributes.cli]]
        name = "split"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("reserved"), "got: {err}");
}

#[test]
fn manifest_cli_args_ref_missing_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.codex"
        name = "Codex"
        version = "0.1.0"
        api_version = "1"

        [entry]
        type = "process"
        command = "x"

        [[contributes.ipc_namespace]]
        prefix = "codex"

        [[contributes.cli]]
        name = "codex"
        subcommands = [
          { name = "spawn", ipc_method = "codex.spawn", args = "missing" },
        ]
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("unknown arg group"), "got: {err}");
}

#[test]
fn manifest_ipc_method_outside_namespace_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.codex"
        name = "Codex"
        version = "0.1.0"
        api_version = "1"

        [entry]
        type = "process"
        command = "x"

        [[contributes.ipc_namespace]]
        prefix = "codex"

        [[contributes.cli]]
        name = "codex"
        subcommands = [
          { name = "evil", ipc_method = "claude.spawn", args = "no_args" },
        ]

        [contributes.cli.arg_groups.no_args]
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("not declared"), "got: {err}");
}

#[test]
fn manifest_cli_ipc_method_no_prefix_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.codex"
        name = "Codex"
        version = "0.1.0"
        api_version = "1"

        [entry]
        type = "process"
        command = "x"

        [[contributes.ipc_namespace]]
        prefix = "codex"

        [[contributes.cli]]
        name = "codex"
        subcommands = [
          { name = "evil", ipc_method = "noprefix", args = "no_args" },
        ]

        [contributes.cli.arg_groups.no_args]
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("no namespace prefix"), "got: {err}");
}

#[test]
fn manifest_duplicate_ipc_prefix_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.codex"
        name = "Codex"
        version = "0.1.0"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[contributes.ipc_namespace]]
        prefix = "codex"
        [[contributes.ipc_namespace]]
        prefix = "codex"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("declared twice"), "got: {err}");
}

#[test]
fn manifest_flag_without_double_dash_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.codex"
        name = "Codex"
        version = "0.1.0"
        api_version = "1"

        [entry]
        type = "process"
        command = "x"

        [[contributes.ipc_namespace]]
        prefix = "codex"

        [[contributes.cli]]
        name = "codex"
        subcommands = [
          { name = "spawn", ipc_method = "codex.spawn", args = "spawn_args" },
        ]

        [contributes.cli.arg_groups.spawn_args]
        flags = [
          { name = "surface", type = "u32", flag = "-s", required = false },
        ]
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("must start with '--'"), "got: {err}");
}

#[test]
fn permission_ipc_invoke_token_round_trip() {
    let p = Permission::from_token("ipc.invoke:codex").expect("should parse");
    match &p {
        Permission::IpcInvoke(prefix) => assert_eq!(prefix, "codex"),
        _ => panic!("expected IpcInvoke"),
    }
    assert_eq!(p.as_token(), "ipc.invoke:codex");
}

#[test]
fn permission_ipc_invoke_empty_prefix_rejected() {
    assert!(Permission::from_token("ipc.invoke:").is_none());
}

#[test]
fn permission_ipc_invoke_invalid_prefix_rejected() {
    // 대문자 거부 (lowercase ascii only)
    assert!(Permission::from_token("ipc.invoke:Codex").is_none());
    // '.' 포함 거부
    assert!(Permission::from_token("ipc.invoke:co.dex").is_none());
    // 숫자 시작 거부
    assert!(Permission::from_token("ipc.invoke:1codex").is_none());
}

#[test]
fn permission_ipc_invoke_reserved_prefix_rejected() {
    // 호스트 예약 prefix는 plugin이 점유할 수 없으므로 토큰도 거부.
    assert!(Permission::from_token("ipc.invoke:surface").is_none());
    assert!(Permission::from_token("ipc.invoke:pane").is_none());
}

#[test]
fn manifest_accepts_ipc_invoke_permission() {
    let s = r#"
        manifest_version = 1
        id = "com.example.codex-helper"
        name = "Helper"
        version = "0.1.0"
        api_version = "1"
        permissions = ["ipc.invoke:codex", "surface.read"]
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    let perms = m.parsed_permissions().expect("resolve");
    assert!(perms.contains(&Permission::IpcInvoke("codex".into())));
    assert!(perms.contains(&Permission::SurfaceRead));
}

#[test]
fn lang_dir_custom() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        lang_dir = "i18n"
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.lang_dir, "i18n");
}

/// 번들된 com.tasty.image plugin의 실제 매니페스트가 파서를 통과하고
/// surface_kind가 host-rendered로 인식되는지 확인.
#[test]
fn bundled_image_plugin_manifest_validates() {
    // CARGO_MANIFEST_DIR = crates/tasty-plugin-manifest → 형제 crate 경로.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("tasty-plugin-image");
    let m = Manifest::load(&path).expect("image plugin manifest should load");
    assert_eq!(m.id, "com.tasty.image");
    assert_eq!(m.surface_kinds.len(), 1);
    assert_eq!(m.surface_kinds[0].kind, "image");
    assert_eq!(m.surface_kinds[0].rendering, SurfaceKindRendering::Host);
    // ipc_namespace prefix가 "image"여야 하고 cli 매핑이 모두 image.* 메서드.
    assert!(
        m.contributes
            .ipc_namespace
            .iter()
            .any(|n| n.prefix == "image")
    );
    assert!(m.contributes.cli.iter().any(|c| c.name == "image"));
}

#[test]
fn surface_kind_rendering_defaults_to_remote() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[surface_kinds]]
        kind = "explorer"
        display_name_i18n_key = "k"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.surface_kinds[0].rendering, SurfaceKindRendering::Remote);
}

#[test]
fn surface_kind_rendering_host_parses() {
    let s = r#"
        manifest_version = 1
        id = "com.tasty.image"
        name = "Image"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[surface_kinds]]
        kind = "image"
        display_name_i18n_key = "surface.kind.image"
        rendering = "host"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.surface_kinds[0].rendering, SurfaceKindRendering::Host);
}

#[test]
fn surface_kind_rendering_unknown_value_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[surface_kinds]]
        kind = "explorer"
        display_name_i18n_key = "k"
        rendering = "exotic"
    "#;
    // serde가 lowercase enum의 알 수 없는 variant를 reject.
    assert!(parse(s).is_err());
}

#[test]
fn event_subscribe_accepts_exact_key_and_wildcard() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_subscribe = ["surface.created", "surface.*", "command.invoked"]
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.event_subscribe.len(), 3);
}

#[test]
fn event_subscribe_rejects_bare_wildcard() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_subscribe = ["*"]
        [entry]
        type = "process"
        command = "x"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(
        err.contains("invalid event_subscribe pattern"),
        "got: {err}"
    );
}

#[test]
fn event_subscribe_rejects_leading_wildcard() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_subscribe = ["*.created"]
        [entry]
        type = "process"
        command = "x"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn event_subscribe_rejects_middle_wildcard() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_subscribe = ["surface.*.created"]
        [entry]
        type = "process"
        command = "x"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn event_subscribe_rejects_single_segment() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_subscribe = ["surface"]
        [entry]
        type = "process"
        command = "x"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn event_subscribe_rejects_partial_wildcard() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_subscribe = ["surf*.created"]
        [entry]
        type = "process"
        command = "x"
    "#;
    assert!(parse(s).is_err());
}

#[test]
fn event_publish_rejects_reserved_namespace() {
    let s = r#"
        manifest_version = 1
        id = "com.example.evil"
        name = "Evil"
        version = "0.1"
        api_version = "1"
        event_publish = ["surface.created"]
        [entry]
        type = "process"
        command = "x"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("reserved namespace"), "got: {err}");
}

#[test]
fn event_publish_accepts_plugin_namespace() {
    let s = r#"
        manifest_version = 1
        id = "com.example.claude"
        name = "Claude"
        version = "0.1"
        api_version = "1"
        event_publish = ["claude.activity.changed", "claude.session.*"]
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.event_publish.len(), 2);
}

#[test]
fn event_subscribe_accepts_reserved_namespace() {
    // subscribe는 어떤 namespace도 허용 (예약은 publish 전용 제약).
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_subscribe = ["surface.*", "system.shutdown"]
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.event_subscribe.len(), 2);
}

#[test]
fn events_emitted_parses_and_defaults_stable() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_publish = ["com.example.x.*"]
        [[events_emitted]]
        key = "com.example.x.child_state_changed"
        description = "child state"
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    assert_eq!(m.events_emitted.len(), 1);
    let decl = &m.events_emitted[0];
    assert_eq!(decl.key, "com.example.x.child_state_changed");
    assert_eq!(decl.stability, EventStability::Stable);
    assert!(decl.payload_schema.is_none());
}

#[test]
fn events_emitted_rejects_wildcard_key() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_publish = ["com.example.x.*"]
        [[events_emitted]]
        key = "com.example.x.*"
        [entry]
        type = "process"
        command = "x"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("invalid events_emitted key"), "got: {err}");
}

#[test]
fn events_emitted_rejects_reserved_namespace() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_publish = ["surface.created"]
        [[events_emitted]]
        key = "surface.created"
        [entry]
        type = "process"
        command = "x"
    "#;
    // event_publish 검증이 먼저 reserved를 잡지만, 다른 검증 단계라도 결국 거부됨.
    assert!(parse(s).is_err());
}

#[test]
fn events_emitted_rejects_uncovered_key() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_publish = ["com.example.x.foo.*"]
        [[events_emitted]]
        key = "com.example.x.bar"
        [entry]
        type = "process"
        command = "x"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("not covered by"), "got: {err}");
}

#[test]
fn events_emitted_rejects_duplicate_key() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_publish = ["com.example.x.*"]
        [[events_emitted]]
        key = "com.example.x.foo"
        [[events_emitted]]
        key = "com.example.x.foo"
        [entry]
        type = "process"
        command = "x"
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("declared twice"), "got: {err}");
}

#[test]
fn events_emitted_accepts_experimental_stability() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        event_publish = ["com.example.x.*"]
        [[events_emitted]]
        key = "com.example.x.alpha"
        stability = "experimental"
        payload_schema = "schemas/alpha.json"
        [entry]
        type = "process"
        command = "x"
    "#;
    let m = parse(s).expect("should parse");
    let decl = &m.events_emitted[0];
    assert_eq!(decl.stability, EventStability::Experimental);
    assert_eq!(decl.payload_schema.as_deref(), Some("schemas/alpha.json"));
}

// ── extends 블록 검증 ────────────────────────────────────────────────

fn extends_skeleton(extra: &str) -> String {
    format!(
        r#"
            manifest_version = 1
            id = "com.example.ext"
            name = "Ext"
            version = "0.1.0"
            api_version = "1"
            permissions = ["ext:com.tasty.clipboard"]
            [entry]
            type = "process"
            command = "x"
            [extends]
            plugin_id = "com.tasty.clipboard"
            version_req = ">=0.2.0, <0.3.0"
            api_version = "1"
            {extra}
        "#
    )
}

#[test]
fn extends_accepts_valid_hooks() {
    let s = extends_skeleton(
        r#"
            [[extends.pre_ipc]]
            method = "clipboard.add"
            modifies = ["entry"]
            mode = "transform"
            timeout_ms = 100

            [[extends.post_event]]
            event = "clipboard.entry_added"
            mode = "observe"
            timeout_ms = 50
        "#,
    );
    let m = parse(&s).expect("should parse");
    let decl = m.extends.as_ref().expect("extends present");
    assert_eq!(decl.plugin_id, "com.tasty.clipboard");
    assert_eq!(decl.pre_ipc.len(), 1);
    assert_eq!(decl.post_event.len(), 1);
    assert_eq!(decl.pre_ipc[0].mode, HookMode::Transform);
    assert_eq!(decl.post_event[0].mode, HookMode::Observe);
}

#[test]
fn extends_rejects_zero_hooks() {
    let s = extends_skeleton("");
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("at least one hook"), "got: {err}");
}

#[test]
fn extends_rejects_self_target() {
    let s = r#"
        manifest_version = 1
        id = "com.example.ext"
        name = "Ext"
        version = "0.1.0"
        api_version = "1"
        permissions = ["ext:com.example.ext"]
        [entry]
        type = "process"
        command = "x"
        [extends]
        plugin_id = "com.example.ext"
        version_req = ">=0.1.0"
        api_version = "1"
        [[extends.pre_ipc]]
        method = "x.run"
        modifies = ["entry"]
        mode = "transform"
        timeout_ms = 100
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("differ from this plugin"), "got: {err}");
}

#[test]
fn extends_rejects_invalid_version_req() {
    let s = extends_skeleton(
        r#"
            [[extends.pre_ipc]]
            method = "clipboard.add"
            modifies = ["entry"]
            mode = "transform"
            timeout_ms = 100
        "#,
    )
    .replace(">=0.2.0, <0.3.0", "not-a-semver-req~~");
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("version_req"), "got: {err}");
}

#[test]
fn extends_rejects_mismatched_api_version() {
    let s = r#"
        manifest_version = 1
        id = "com.example.ext"
        name = "Ext"
        version = "0.1.0"
        api_version = "1"
        permissions = ["ext:com.tasty.clipboard"]
        [entry]
        type = "process"
        command = "x"
        [extends]
        plugin_id = "com.tasty.clipboard"
        version_req = ">=0.2.0, <0.3.0"
        api_version = "999"
        [[extends.pre_ipc]]
        method = "clipboard.add"
        modifies = ["entry"]
        mode = "transform"
        timeout_ms = 100
    "#;
    let err = parse(s).unwrap_err().to_string();
    assert!(err.contains("api_version"), "got: {err}");
}

#[test]
fn extends_rejects_timeout_over_max() {
    let s = extends_skeleton(
        r#"
            [[extends.pre_ipc]]
            method = "clipboard.add"
            modifies = ["entry"]
            mode = "transform"
            timeout_ms = 5000
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("exceeds maximum"), "got: {err}");
}

#[test]
fn extends_rejects_zero_timeout() {
    let s = extends_skeleton(
        r#"
            [[extends.pre_ipc]]
            method = "clipboard.add"
            modifies = ["entry"]
            mode = "transform"
            timeout_ms = 0
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("timeout_ms"), "got: {err}");
}

#[test]
fn extends_rejects_event_wildcard() {
    let s = extends_skeleton(
        r#"
            [[extends.pre_event]]
            event = "clipboard.*"
            modifies = ["payload"]
            mode = "transform"
            timeout_ms = 100
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("concrete event key"), "got: {err}");
}

#[test]
fn extends_rejects_ipc_wildcard() {
    let s = extends_skeleton(
        r#"
            [[extends.pre_ipc]]
            method = "clipboard.*"
            modifies = ["entry"]
            mode = "transform"
            timeout_ms = 100
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("concrete method"), "got: {err}");
}

#[test]
fn extends_rejects_transform_without_modifies() {
    let s = extends_skeleton(
        r#"
            [[extends.pre_ipc]]
            method = "clipboard.add"
            modifies = []
            mode = "transform"
            timeout_ms = 100
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("non-empty 'modifies'"), "got: {err}");
}

#[test]
fn extends_requires_ext_permission_in_manifest() {
    // skeleton에는 ext:com.tasty.clipboard 권한이 들어 있다. 그걸 빼면 거부되어야.
    let s = extends_skeleton(
        r#"
            [[extends.pre_ipc]]
            method = "clipboard.add"
            modifies = ["entry"]
            mode = "transform"
            timeout_ms = 100
        "#,
    )
    .replace(
        "permissions = [\"ext:com.tasty.clipboard\"]",
        "permissions = []",
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("ext:com.tasty.clipboard"), "got: {err}");
}

#[test]
fn ext_permission_token_parses() {
    assert_eq!(
        Permission::from_token("ext:com.tasty.clipboard"),
        Some(Permission::Extension("com.tasty.clipboard".into()))
    );
    // 잘못된 plugin id는 거부.
    assert_eq!(Permission::from_token("ext:Bad-Id"), None);
    assert_eq!(Permission::from_token("ext:"), None);
    // round trip.
    assert_eq!(
        Permission::Extension("com.x.y".into()).as_token(),
        "ext:com.x.y"
    );
}

fn tool_skeleton(extra: &str) -> String {
    format!(
        r#"
        manifest_version = 1
        id = "com.example.tooly"
        name = "Tooly"
        version = "0.1.0"
        api_version = "1"
        permissions = ["ui.tool_item"]
        [entry]
        type = "process"
        command = "tooly"
        [[surface_kinds]]
        kind = "tooly_panel"
        display_name_i18n_key = "tooly.surface"
        [contributes]
        {extra}
        "#,
    )
}

#[test]
fn tool_event_action_parses() {
    let s = tool_skeleton(
        r#"
            [[contributes.tool]]
            id = "open-search"
            label_i18n_key = "tooly.tool.open_search"
            action = { kind = "event", event_key = "tooly.search_requested" }
        "#,
    );
    let m = parse(&s).expect("event tool should parse");
    assert_eq!(m.contributes.tool.len(), 1);
    match &m.contributes.tool[0].action {
        ToolAction::Event { event_key } => assert_eq!(event_key, "tooly.search_requested"),
        other => panic!("expected event action, got {other:?}"),
    }
    // 기본 order_hint = 100
    assert_eq!(m.contributes.tool[0].order_hint, 100);
}

#[test]
fn tool_open_surface_must_reference_declared_kind() {
    let s = tool_skeleton(
        r#"
            [[contributes.tool]]
            id = "open-panel"
            label_i18n_key = "tooly.tool.open_panel"
            action = { kind = "open_surface", surface_kind = "not_declared" }
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(
        err.contains("not declared in this plugin's [[surface_kinds]]"),
        "got: {err}"
    );
}

#[test]
fn tool_requires_ui_tool_item_permission() {
    let s = tool_skeleton(
        r#"
            [[contributes.tool]]
            id = "open-search"
            label_i18n_key = "tooly.tool.open_search"
            action = { kind = "event", event_key = "tooly.search_requested" }
        "#,
    )
    .replace("permissions = [\"ui.tool_item\"]", "permissions = []");
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("ui.tool_item"), "got: {err}");
}

#[test]
fn tool_id_must_be_unique() {
    let s = tool_skeleton(
        r#"
            [[contributes.tool]]
            id = "dup"
            label_i18n_key = "a"
            action = { kind = "event", event_key = "tooly.a" }
            [[contributes.tool]]
            id = "dup"
            label_i18n_key = "b"
            action = { kind = "event", event_key = "tooly.b" }
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("declared twice"), "got: {err}");
}

#[test]
fn ui_tool_item_permission_token_parses() {
    assert_eq!(
        Permission::from_token("ui.tool_item"),
        Some(Permission::UiToolItem)
    );
    assert_eq!(Permission::UiToolItem.as_token(), "ui.tool_item");
}

#[test]
fn ui_popup_permission_token_parses() {
    assert_eq!(
        Permission::from_token("ui.popup"),
        Some(Permission::UiPopup)
    );
    assert_eq!(Permission::UiPopup.as_token(), "ui.popup");
}

#[test]
fn agent_permission_token_parses() {
    assert_eq!(
        Permission::from_token("agent"),
        Some(Permission::AgentManage)
    );
    assert_eq!(Permission::AgentManage.as_token(), "agent");
}

#[test]
fn memory_permission_tokens_parse() {
    assert_eq!(
        Permission::from_token("memory.read"),
        Some(Permission::MemoryRead)
    );
    assert_eq!(
        Permission::from_token("memory.write"),
        Some(Permission::MemoryWrite)
    );
    assert_eq!(Permission::MemoryRead.as_token(), "memory.read");
    assert_eq!(Permission::MemoryWrite.as_token(), "memory.write");
}

fn popup_skeleton(extra: &str) -> String {
    format!(
        r#"
        manifest_version = 1
        id = "com.example.popper"
        name = "Popper"
        version = "0.1.0"
        api_version = "1"
        permissions = ["ui.popup"]
        event_publish = ["com.example.popper.search_requested"]
        [entry]
        type = "process"
        command = "popper"
        [contributes]
        {extra}
        "#,
    )
}

#[test]
fn popup_event_trigger_parses_with_defaults() {
    let s = popup_skeleton(
        r#"
            [[contributes.popup]]
            id = "search"
            trigger = { kind = "event", event_key = "com.example.popper.search_requested" }
        "#,
    );
    let m = parse(&s).expect("popup with event trigger should parse");
    assert_eq!(m.contributes.popup.len(), 1);
    let p = &m.contributes.popup[0];
    assert_eq!(p.id, "search");
    match &p.trigger {
        PopupTrigger::Event { event_key } => {
            assert_eq!(event_key, "com.example.popper.search_requested")
        }
        other => panic!("expected event trigger, got {other:?}"),
    }
    assert_eq!(p.anchor, PopupAnchor::ScreenCenter);
    assert!(p.dismiss_on_outside_click);
    assert!(p.size_hint.is_none());
}

#[test]
fn popup_ipc_trigger_with_size_and_anchor() {
    let s = popup_skeleton(
        r#"
            [[contributes.popup]]
            id = "panel"
            trigger = { kind = "ipc" }
            size_hint = { width = 480, height = 360 }
            anchor = "cursor"
            dismiss_on_outside_click = false
        "#,
    );
    let m = parse(&s).expect("popup with ipc trigger should parse");
    let p = &m.contributes.popup[0];
    assert!(matches!(p.trigger, PopupTrigger::Ipc));
    assert_eq!(p.anchor, PopupAnchor::Cursor);
    assert!(!p.dismiss_on_outside_click);
    let sz = p.size_hint.expect("size_hint set");
    assert_eq!(sz.width, 480);
    assert_eq!(sz.height, 360);
}

#[test]
fn popup_requires_ui_popup_permission() {
    let s = popup_skeleton(
        r#"
            [[contributes.popup]]
            id = "search"
            trigger = { kind = "ipc" }
        "#,
    )
    .replace("permissions = [\"ui.popup\"]", "permissions = []");
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("ui.popup"), "got: {err}");
}

#[test]
fn popup_id_must_be_unique() {
    let s = popup_skeleton(
        r#"
            [[contributes.popup]]
            id = "dup"
            trigger = { kind = "ipc" }
            [[contributes.popup]]
            id = "dup"
            trigger = { kind = "ipc" }
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("declared twice"), "got: {err}");
}

#[test]
fn popup_size_hint_zero_rejected() {
    let s = popup_skeleton(
        r#"
            [[contributes.popup]]
            id = "search"
            trigger = { kind = "ipc" }
            size_hint = { width = 0, height = 360 }
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("size_hint"), "got: {err}");
}

#[test]
fn extends_filter_mode_allows_empty_modifies() {
    let s = extends_skeleton(
        r#"
            [[extends.pre_ipc]]
            method = "clipboard.add"
            modifies = []
            mode = "filter"
            timeout_ms = 100
        "#,
    );
    let m = parse(&s).expect("filter without modifies should parse");
    assert_eq!(m.extends.unwrap().pre_ipc[0].mode, HookMode::Filter);
}

#[test]
fn file_handler_define_token_parses() {
    assert_eq!(
        Permission::from_token("file_handler.define"),
        Some(Permission::FileHandlerDefine)
    );
    assert_eq!(
        Permission::FileHandlerDefine.as_token(),
        "file_handler.define"
    );
}

#[test]
fn file_handler_extend_handle_tokens_parse() {
    let p = Permission::from_token("file_handler.extend:markdown").unwrap();
    assert_eq!(p, Permission::FileHandlerExtend("markdown".into()));
    assert_eq!(p.as_token(), "file_handler.extend:markdown");

    let p = Permission::from_token("file_handler.handle:pdf").unwrap();
    assert_eq!(p, Permission::FileHandlerHandle("pdf".into()));
    assert_eq!(p.as_token(), "file_handler.handle:pdf");

    // $directory 는 허용 (실제 등록된 reserved id)
    assert!(Permission::from_token("file_handler.handle:$directory").is_some());
}

#[test]
fn file_handler_unknown_sentinel_token_rejected() {
    assert!(Permission::from_token("file_handler.handle:$unknown").is_none());
    assert!(Permission::from_token("file_handler.extend:$unknown").is_none());
}

fn detector_skeleton(perms: &str, extra: &str) -> String {
    format!(
        r#"
        manifest_version = 1
        id = "com.example.pdf"
        name = "PDF"
        version = "0.1"
        api_version = "1"
        permissions = [{perms}]
        [entry]
        type = "process"
        command = "x"
        {extra}
        "#
    )
}

#[test]
fn detector_define_token_required_for_new_id() {
    let s = detector_skeleton(
        r#""file_handler.define""#,
        r#"
            [[contributes.detector]]
            id = "pdf"
            [[contributes.detector.rule]]
            kind = "extension"
            values = ["pdf"]
        "#,
    );
    parse(&s).expect("with file_handler.define should accept new detector");
}

#[test]
fn detector_without_define_or_extend_rejected() {
    let s = detector_skeleton(
        r#""#,
        r#"
            [[contributes.detector]]
            id = "pdf"
            [[contributes.detector.rule]]
            kind = "extension"
            values = ["pdf"]
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("file_handler.define") || err.contains("file_handler.extend"));
}

#[test]
fn detector_reserved_id_rejected_for_plugin() {
    let s = detector_skeleton(
        r#""file_handler.define""#,
        r#"
            [[contributes.detector]]
            id = "$mything"
            [[contributes.detector.rule]]
            kind = "extension"
            values = ["x"]
        "#,
    );
    assert!(parse(&s).is_err());
}

#[test]
fn handler_requires_handle_permission() {
    let s = detector_skeleton(
        r#""file_handler.define", "surface.write""#,
        r#"
            [[surface_kinds]]
            kind = "pdf_view"
            display_name_i18n_key = "x"

            [[contributes.detector]]
            id = "pdf"
            [[contributes.detector.rule]]
            kind = "extension"
            values = ["pdf"]

            [[contributes.handler]]
            id = "viewer"
            detector = "pdf"
            priority = 100
            [contributes.handler.action]
            kind = "open_surface"
            surface_kind = "pdf_view"
        "#,
    );
    // permissions 에 file_handler.handle:pdf 가 없으므로 reject
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("file_handler.handle:pdf"), "got: {err}");
}

#[test]
#[ignore = "production validation 미구현 — handler.action.surface_kind 가 plugin 의 surface_kinds 와 cross-ref 되지 않음. validator 추가 후 ignore 해제."]
fn handler_open_surface_cross_ref() {
    // surface_kind 가 plugin 자체에 없으면 reject
    let s = detector_skeleton(
        r#""file_handler.define", "file_handler.handle:pdf", "surface.write""#,
        r#"
            [[contributes.detector]]
            id = "pdf"
            [[contributes.detector.rule]]
            kind = "extension"
            values = ["pdf"]

            [[contributes.handler]]
            id = "viewer"
            detector = "pdf"
            priority = 100
            [contributes.handler.action]
            kind = "open_surface"
            surface_kind = "missing"
        "#,
    );
    assert!(parse(&s).is_err());
}

#[test]
#[ignore = "production validation 미구현 — PluginHandlerActionDecl 가 'system' kind 를 silently accept. 별 PluginHandlerAction enum (System 제외) 분리 후 ignore 해제."]
fn handler_system_kind_rejected_in_plugin() {
    let s = detector_skeleton(
        r#""file_handler.define", "file_handler.handle:pdf""#,
        r#"
            [[contributes.detector]]
            id = "pdf"
            [[contributes.detector.rule]]
            kind = "extension"
            values = ["pdf"]

            [[contributes.handler]]
            id = "viewer"
            detector = "pdf"
            priority = 100
            [contributes.handler.action]
            kind = "system"
        "#,
    );
    // serde 가 PluginHandlerActionDecl 의 unknown variant 로 reject
    assert!(parse(&s).is_err());
}

// ─────────────────────────────────────────────────────────────────
//  F.H — Plugin manifest 확장
// ─────────────────────────────────────────────────────────────────

#[test]
fn window_spawn_permission_token_roundtrip() {
    assert_eq!(
        Permission::from_token("window.spawn"),
        Some(Permission::WindowSpawn)
    );
    assert_eq!(Permission::WindowSpawn.as_token(), "window.spawn");
}

#[test]
fn surface_kind_default_colors_parses() {
    let s = r##"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[surface_kinds]]
        kind = "foo"
        display_name_i18n_key = "k"
        [surface_kinds.default_colors]
        focused_bg = "#000000"
        focused_fg = "#cdd6f4"
        unfocused_bg = "#181825"
        unfocused_fg = "#a6adc8"
    "##;
    let m = parse(s).expect("surface_kinds.default_colors should parse");
    let kind = &m.surface_kinds[0];
    let dc = kind.default_colors.as_ref().expect("default_colors set");
    assert!(dc.focused_bg.is_some());
    assert!(dc.focused_fg.is_some());
}

#[test]
fn surface_kind_default_colors_invalid_hex_rejected() {
    let s = r#"
        manifest_version = 1
        id = "com.example.x"
        name = "X"
        version = "0.1"
        api_version = "1"
        [entry]
        type = "process"
        command = "x"
        [[surface_kinds]]
        kind = "foo"
        display_name_i18n_key = "k"
        [surface_kinds.default_colors]
        focused_bg = "not-a-color"
    "#;
    // HexColor 의 Deserialize 가 reject (parse 단계).
    let m: Result<Manifest, _> = toml::from_str(s);
    assert!(m.is_err());
}

fn window_skeleton(perms: &str, extra: &str) -> String {
    format!(
        r#"
        manifest_version = 1
        id = "com.example.winapp"
        name = "WinApp"
        version = "0.1.0"
        api_version = "1"
        permissions = [{perms}]
        [entry]
        type = "process"
        command = "winapp"
        [contributes]
        {extra}
        "#,
    )
}

#[test]
fn window_contribute_parses_with_defaults() {
    let s = window_skeleton(
        r#""window.spawn""#,
        r#"
            [[contributes.window]]
            id = "editor"
            display_name_i18n_key = "winapp.editor"
        "#,
    );
    let m = parse(&s).expect("window contribute should parse");
    assert_eq!(m.contributes.window.len(), 1);
    let w = &m.contributes.window[0];
    assert_eq!(w.id, "editor");
    assert!(!w.multi_instance);
    assert!(w.default_size.is_none());
}

#[test]
fn window_contribute_with_size_and_multi_instance() {
    let s = window_skeleton(
        r#""window.spawn""#,
        r#"
            [[contributes.window]]
            id = "editor"
            display_name_i18n_key = "winapp.editor"
            multi_instance = true
            default_size = { width = 1024, height = 768 }
        "#,
    );
    let m = parse(&s).expect("window contribute with size should parse");
    let w = &m.contributes.window[0];
    assert!(w.multi_instance);
    let sz = w.default_size.expect("default_size set");
    assert_eq!(sz.width, 1024);
    assert_eq!(sz.height, 768);
}

#[test]
fn window_contribute_requires_permission() {
    let s = window_skeleton(
        "",
        r#"
            [[contributes.window]]
            id = "editor"
            display_name_i18n_key = "winapp.editor"
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("window.spawn"), "got: {err}");
}

#[test]
fn window_contribute_id_must_be_unique() {
    let s = window_skeleton(
        r#""window.spawn""#,
        r#"
            [[contributes.window]]
            id = "dup"
            display_name_i18n_key = "winapp.dup"
            [[contributes.window]]
            id = "dup"
            display_name_i18n_key = "winapp.dup"
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("declared twice"), "got: {err}");
}

#[test]
fn window_contribute_id_format_validated() {
    let s = window_skeleton(
        r#""window.spawn""#,
        r#"
            [[contributes.window]]
            id = "Bad-Id"
            display_name_i18n_key = "winapp.bad"
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("invalid contributes.window id"), "got: {err}");
}

#[test]
fn example_markdown_plugin_manifest_parses() {
    // crates/tasty-plugin-markdown/tasty-plugin.toml — schema 확장 데모 +
    // 템플릿 plugin. BUILTINS 미등록이라 런타임 로드는 안 되지만, 컴파일타임
    // include + 런타임 parse 로 schema 호환성을 잠근다.
    let text = include_str!("../../tasty-plugin-markdown/tasty-plugin.toml");
    let m = parse(text).expect("example markdown manifest should parse and validate");
    assert_eq!(m.id, "com.tasty.markdown");
    assert_eq!(m.surface_kinds.len(), 1);
    assert!(m.surface_kinds[0].default_colors.is_some());
}

#[test]
fn window_contribute_zero_default_size_rejected() {
    let s = window_skeleton(
        r#""window.spawn""#,
        r#"
            [[contributes.window]]
            id = "editor"
            display_name_i18n_key = "winapp.editor"
            default_size = { width = 0, height = 768 }
        "#,
    );
    let err = parse(&s).unwrap_err().to_string();
    assert!(err.contains("default_size"), "got: {err}");
}
