//! `FileFormatRegistry` 단위 테스트 — manifest install/uninstall, extension priority,
//! user TOML round-trip, identify_by_*, plugin disable/enable 등.

use super::*;
use crate::config::DetectorRuleDecl;
use crate::types::{DetectDepth, FileTarget};
use std::path::PathBuf;

fn target(p: &str) -> FileTarget {
    FileTarget::new(PathBuf::from(p))
}

fn install_host_with_markdown(reg: &FileFormatRegistry) {
    reg.install_host_defaults(crate::HOST_DEFAULTS_TOML);
    let decls = vec![DetectorDecl {
        id: "markdown".into(),
        display_name_i18n_key: Some("file_handler.format.markdown".into()),
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::Extension {
            values: vec!["md".into(), "markdown".into()],
        }],
    }];
    reg.install_plugin_detectors("com.tasty.markdown", &decls);
}

#[test]
fn host_default_loads_and_identifies_markdown() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let id = reg.identify(&target("a/b.md"), DetectDepth::Cheap);
    assert_eq!(id, Some(DetectorId("markdown".into())));
    let id = reg.identify(&target("a/b.MARKDOWN"), DetectDepth::Cheap);
    assert_eq!(id, Some(DetectorId("markdown".into())));
    let id = reg.identify(&target("a/b.html"), DetectDepth::Cheap);
    assert_eq!(id, Some(DetectorId("html".into())));
    let id = reg.identify(&target("a/b.unknownext"), DetectDepth::Cheap);
    assert_eq!(id, None);
}

#[test]
fn plugin_extends_existing_detector() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let decls = vec![DetectorDecl {
        id: "markdown".into(),
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::Extension {
            values: vec!["mdx".into()],
        }],
    }];
    reg.install_plugin_detectors("com.example.mdx", &decls);
    let id = reg.identify(&target("a/b.mdx"), DetectDepth::Cheap);
    assert_eq!(id, Some(DetectorId("markdown".into())));
    let id = reg.identify(&target("a/b.md"), DetectDepth::Cheap);
    assert_eq!(id, Some(DetectorId("markdown".into())));
}

#[test]
fn plugin_lua_rule_dropped_with_warn() {
    let reg = FileFormatRegistry::new();
    let decls = vec![DetectorDecl {
        id: "weird-fmt".into(),
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: vec![
            DetectorRuleDecl::Lua {
                script: "return true".into(),
            },
            DetectorRuleDecl::Extension {
                values: vec!["wf".into()],
            },
        ],
    }];
    reg.install_plugin_detectors("com.example.weird", &decls);
    let id = reg.identify(&target("x.wf"), DetectDepth::Deep);
    assert_eq!(id, Some(DetectorId("weird-fmt".into())));
}

#[test]
fn plugin_lua_only_detector_skipped() {
    let reg = FileFormatRegistry::new();
    let decls = vec![DetectorDecl {
        id: "lua-only".into(),
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::Lua {
            script: "return true".into(),
        }],
    }];
    reg.install_plugin_detectors("com.example.lua-only", &decls);
    assert_eq!(reg.identify(&target("anything"), DetectDepth::Deep), None);
}

#[test]
fn uninstall_plugin_removes_only_its_rules() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let decls = vec![DetectorDecl {
        id: "markdown".into(),
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::Extension {
            values: vec!["mdx".into()],
        }],
    }];
    reg.install_plugin_detectors("com.example.mdx", &decls);
    assert_eq!(
        reg.identify(&target("a/b.mdx"), DetectDepth::Cheap),
        Some(DetectorId("markdown".into()))
    );
    reg.uninstall_plugin("com.example.mdx");
    assert_eq!(
        reg.identify(&target("a/b.md"), DetectDepth::Cheap),
        Some(DetectorId("markdown".into()))
    );
    assert_eq!(reg.identify(&target("a/b.mdx"), DetectDepth::Cheap), None);
}

#[test]
fn directory_prefilter() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let dir = std::env::temp_dir();
    let t = FileTarget::new(dir);
    assert_eq!(
        reg.identify(&t, DetectDepth::Cheap),
        Some(DetectorId("$directory".into()))
    );
    let t = target("/nonexistent/file.no-such-ext");
    assert_eq!(reg.identify(&t, DetectDepth::Cheap), None);
}

#[test]
fn identify_deep_matches_magic_when_cheap_misses() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "png"
            [[detector.rule]]
            kind = "extension"
            values = ["png"]
            [[detector.rule]]
            kind = "magic"
            offset = 0
            bytes_hex = "89504E470D0A1A0A"
        "#;
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("file-handlers.toml");
    std::fs::write(&cfg, user_toml).unwrap();
    reg.install_user_config(&cfg);

    let png_sig = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let img_path = dir.path().join("masquerade.dat");
    std::fs::write(&img_path, png_sig).unwrap();
    let t = FileTarget::new(img_path);

    assert_eq!(reg.identify(&t, DetectDepth::Cheap), None);
    assert_eq!(
        reg.identify(&t, DetectDepth::Deep),
        Some(DetectorId("png".into()))
    );
}

#[test]
fn reload_user_config_replaces_user_entries_keeps_host() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    assert_eq!(
        reg.identify(&target("a/b.pdf"), DetectDepth::Cheap),
        Some(DetectorId("pdf".into()))
    );

    std::fs::write(
        &p,
        r#"
                [[detector]]
                id = "csv"
                [[detector.rule]]
                kind = "extension"
                values = ["csv"]
            "#,
    )
    .unwrap();
    reg.reload_user_config(&p);

    assert_eq!(reg.identify(&target("a/b.pdf"), DetectDepth::Cheap), None);
    assert_eq!(
        reg.identify(&target("a/b.csv"), DetectDepth::Cheap),
        Some(DetectorId("csv".into()))
    );
    assert_eq!(
        reg.identify(&target("a/b.md"), DetectDepth::Cheap),
        Some(DetectorId("markdown".into()))
    );
}

#[test]
fn reload_user_config_missing_file_clears_user_entries() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    assert!(reg.detector(&DetectorId("pdf".into())).is_some());

    std::fs::remove_file(&p).unwrap();
    reg.reload_user_config(&p);
    assert!(reg.detector(&DetectorId("pdf".into())).is_none());
    assert!(reg.detector(&DetectorId("markdown".into())).is_some());
}

#[test]
fn reload_user_config_parse_error_keeps_previous_state() {
    let reg = FileFormatRegistry::new();
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(
        &p,
        r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    assert!(reg.detector(&DetectorId("pdf".into())).is_some());

    std::fs::write(&p, "[[detector\n id = broken").unwrap();
    reg.reload_user_config(&p);
    assert!(reg.detector(&DetectorId("pdf".into())).is_some());
}

#[test]
fn user_disabled_overrides_host() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let user_toml = r#"
            [[detector]]
            id = "markdown"
            disabled = true
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);
    assert_eq!(reg.identify(&target("a/b.md"), DetectDepth::Cheap), None);
}

#[test]
fn export_emits_user_only_origin() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let user_toml = r#"
            [[detector]]
            id = "pdf"
            [[detector.rule]]
            kind = "extension"
            values = ["pdf"]

            [[detector]]
            id = "markdown"
            disabled = true
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let exported = reg.export_user_config();
    assert!(exported.contains("pdf"), "exported = {exported}");
    assert!(exported.contains("markdown"));
    assert!(exported.contains("disabled = true"));
    assert!(exported.contains("\"pdf\""));
}

#[test]
fn export_round_trip_preserves_user_state() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let user_toml = r#"
            [[detector]]
            id = "pdf"
            display_name_i18n_key = "file_format.pdf"
            icon = "file-pdf"
            [[detector.rule]]
            kind = "extension"
            values = ["pdf"]
            [[detector.rule]]
            kind = "magic"
            offset = 0
            bytes_hex = "255044462D"

            [[detector]]
            id = "markdown"
            disabled = true
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let exported = reg.export_user_config();

    let reg2 = FileFormatRegistry::new();
    install_host_with_markdown(&reg2);
    let p2 = dir.path().join("export.toml");
    std::fs::write(&p2, &exported).unwrap();
    reg2.install_user_config(&p2);

    assert_eq!(
        reg.identify(&target("a/b.pdf"), DetectDepth::Cheap),
        reg2.identify(&target("a/b.pdf"), DetectDepth::Cheap),
    );
    assert_eq!(
        reg.identify(&target("a/b.md"), DetectDepth::Cheap),
        reg2.identify(&target("a/b.md"), DetectDepth::Cheap),
    );

    let pdf = reg2.detector(&DetectorId("pdf".into())).unwrap();
    assert_eq!(
        pdf.display_name_i18n_key.as_deref(),
        Some("file_format.pdf")
    );
    assert_eq!(pdf.icon.as_deref(), Some("file-pdf"));
}

#[test]
fn export_preserves_unknown_rule_payload() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "futureproof"
            [[detector.rule]]
            kind = "ai_classify"
            model = "v2"
            confidence = 0.8
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-handlers.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let exported = reg.export_user_config();
    assert!(exported.contains("ai_classify"));
    assert!(exported.contains("model"));
    assert!(exported.contains("\"v2\""));
    assert!(exported.contains("confidence"));
}

#[test]
fn save_user_config_atomic_write() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "pdf"
            [[detector.rule]]
            kind = "extension"
            values = ["pdf"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("src.toml");
    std::fs::write(&src, user_toml).unwrap();
    reg.install_user_config(&src);

    let dst = dir.path().join("subdir").join("dst.toml");
    reg.save_user_config(&dst).unwrap();
    assert!(dst.exists());
    let written = std::fs::read_to_string(&dst).unwrap();
    assert!(written.contains("pdf"));
    assert!(written.contains("\"pdf\""));
}

#[test]
fn export_empty_when_no_user_contributions() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    assert_eq!(reg.export_user_config(), "");
}

#[test]
fn advertised_extensions_returns_only_extension_rule_values() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "png"
            [[detector.rule]]
            kind = "extension"
            values = ["png", "PNG"]
            [[detector.rule]]
            kind = "magic"
            offset = 0
            bytes_hex = "89504E47"
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let exts = reg.advertised_extensions(&DetectorId("png".into()));
    assert_eq!(exts, vec!["png".to_string()]);

    assert!(
        reg.advertised_extensions(&DetectorId("nope".into()))
            .is_empty()
    );
}

#[test]
fn detectors_for_extension_orders_by_install_order() {
    let reg = FileFormatRegistry::new();
    let user_toml_a = r#"
            [[detector]]
            id = "zzz"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p1 = dir.path().join("a.toml");
    std::fs::write(&p1, user_toml_a).unwrap();
    reg.install_user_config(&p1);

    let decls = vec![DetectorDecl {
        id: "aaa".into(),
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::Extension {
            values: vec!["md".into()],
        }],
    }];
    reg.install_plugin_detectors("com.example.aaa", &decls);

    let hits = reg.detectors_for_extension("md");
    assert_eq!(
        hits,
        vec![DetectorId("zzz".into()), DetectorId("aaa".into())]
    );
}

#[test]
fn detectors_for_extension_skips_disabled() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "x"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[detector]]
            id = "y"
            disabled = true
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let hits = reg.detectors_for_extension("md");
    assert_eq!(hits, vec![DetectorId("x".into())]);
}

#[test]
fn detectors_for_extension_accepts_leading_dot_and_uppercase() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "x"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    assert_eq!(
        reg.detectors_for_extension(".md"),
        vec![DetectorId("x".into())]
    );
    assert_eq!(
        reg.detectors_for_extension("MD"),
        vec![DetectorId("x".into())]
    );
    assert!(reg.detectors_for_extension("").is_empty());
    assert!(reg.detectors_for_extension(".").is_empty());
}

#[test]
fn all_advertised_extensions_dedupes_and_sorts() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "a"
            [[detector.rule]]
            kind = "extension"
            values = ["md", "markdown"]

            [[detector]]
            id = "b"
            [[detector.rule]]
            kind = "extension"
            values = ["mdx", "md"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let exts = reg.all_advertised_extensions();
    assert_eq!(
        exts,
        vec!["markdown".to_string(), "md".to_string(), "mdx".to_string()],
    );
}

#[test]
fn is_enabled_reflects_disabled_field() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    assert!(reg.is_enabled(&DetectorId("markdown".into())));
    assert!(!reg.is_enabled(&DetectorId("nope".into())));

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(
        &p,
        r#"
                [[detector]]
                id = "markdown"
                disabled = true
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    assert!(!reg.is_enabled(&DetectorId("markdown".into())));
}

#[test]
fn extension_priority_user_config_parsed_and_queryable() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "x"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[detector]]
            id = "y"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["y", "x"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let order = reg.extension_priority_order("md").expect("present");
    assert_eq!(order, vec![DetectorId("y".into()), DetectorId("x".into())]);
    assert_eq!(reg.extension_priority_order(".MD"), Some(order));
    assert!(reg.extension_priority_order("zzz").is_none());
}

#[test]
fn extension_priority_user_overrides_host() {
    let reg = FileFormatRegistry::new();
    let host_toml = r#"
            [[detector]]
            id = "host-md"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["host-md"]
        "#;
    reg.install_host_defaults(host_toml);
    assert_eq!(
        reg.extension_priority_order("md"),
        Some(vec![DetectorId("host-md".into())])
    );

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(
        &p,
        r#"
                [[extension_priority]]
                extension = "md"
                order = ["user-md"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    assert_eq!(
        reg.extension_priority_order("md"),
        Some(vec![DetectorId("user-md".into())])
    );
}

#[test]
fn extension_priority_empty_order_removes_entry() {
    let reg = FileFormatRegistry::new();
    reg.install_host_defaults(
        r#"
                [[extension_priority]]
                extension = "md"
                order = ["host-md"]
            "#,
    );
    assert!(reg.extension_priority_order("md").is_some());

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(
        &p,
        r#"
                [[extension_priority]]
                extension = "md"
                order = []
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    assert!(reg.extension_priority_order("md").is_none());
}

#[test]
fn extension_priority_exported_only_user_origin() {
    let reg = FileFormatRegistry::new();
    reg.install_host_defaults(
        r#"
                [[extension_priority]]
                extension = "md"
                order = ["host-md"]
            "#,
    );
    assert_eq!(reg.export_user_config(), "");

    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(
        &p,
        r#"
                [[extension_priority]]
                extension = "json"
                order = ["json-strict", "json"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);

    let exported = reg.export_user_config();
    assert!(exported.contains("extension_priority"), "got: {exported}");
    assert!(exported.contains("\"json\""));
    assert!(exported.contains("json-strict"));
    assert!(!exported.contains("host-md"), "got: {exported}");
}

#[test]
fn extension_priority_round_trip_through_export() {
    let reg = FileFormatRegistry::new();
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(
        &p,
        r#"
                [[extension_priority]]
                extension = "md"
                order = ["mdx-strict", "markdown"]

                [[extension_priority]]
                extension = "json"
                order = ["jsonc"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    let exported = reg.export_user_config();

    let reg2 = FileFormatRegistry::new();
    let p2 = dir.path().join("export.toml");
    std::fs::write(&p2, &exported).unwrap();
    reg2.install_user_config(&p2);

    assert_eq!(
        reg2.extension_priority_order("md"),
        Some(vec![
            DetectorId("mdx-strict".into()),
            DetectorId("markdown".into())
        ])
    );
    assert_eq!(
        reg2.extension_priority_order("json"),
        Some(vec![DetectorId("jsonc".into())])
    );
}

#[test]
fn extension_priority_reload_clears_old_user_entries() {
    let reg = FileFormatRegistry::new();
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");

    std::fs::write(
        &p,
        r#"
                [[extension_priority]]
                extension = "md"
                order = ["mdx"]

                [[extension_priority]]
                extension = "json"
                order = ["json"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    assert!(reg.extension_priority_order("md").is_some());
    assert!(reg.extension_priority_order("json").is_some());

    std::fs::write(
        &p,
        r#"
                [[extension_priority]]
                extension = "md"
                order = ["mdx"]
            "#,
    )
    .unwrap();
    reg.reload_user_config(&p);
    assert!(reg.extension_priority_order("md").is_some());
    assert!(
        reg.extension_priority_order("json").is_none(),
        "user reload should drop previous json entry",
    );
}

#[test]
fn extension_priority_dedupes_duplicate_ids_in_order() {
    let reg = FileFormatRegistry::new();
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(
        &p,
        r#"
                [[extension_priority]]
                extension = "md"
                order = ["a", "b", "a", "b", "c"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);

    let order = reg.extension_priority_order("md").unwrap();
    assert_eq!(
        order,
        vec![
            DetectorId("a".into()),
            DetectorId("b".into()),
            DetectorId("c".into())
        ]
    );
}

#[test]
fn identify_uses_extension_priority_table() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "a"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[detector]]
            id = "b"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["b", "a"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let got = reg.identify(&target("hello.md"), DetectDepth::Cheap);
    assert_eq!(got, Some(DetectorId("b".into())));
}

#[test]
fn identify_falls_back_to_install_order_without_priority_table() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "z"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let decls = vec![DetectorDecl {
        id: "a".into(),
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::Extension {
            values: vec!["md".into()],
        }],
    }];
    reg.install_plugin_detectors("com.example.a", &decls);

    let got = reg.identify(&target("hello.md"), DetectDepth::Cheap);
    assert_eq!(got, Some(DetectorId("z".into())));
}

#[test]
fn identify_priority_entry_with_unknown_id_skips_to_next() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "real"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["ghost", "real"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let got = reg.identify(&target("a.md"), DetectDepth::Cheap);
    assert_eq!(got, Some(DetectorId("real".into())));
}

#[test]
fn identify_fast_path_skips_disabled_detectors() {
    let reg = FileFormatRegistry::new();
    let user_toml = r#"
            [[detector]]
            id = "off"
            disabled = true
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[detector]]
            id = "on"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["off", "on"]
        "#;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(&p, user_toml).unwrap();
    reg.install_user_config(&p);

    let got = reg.identify(&target("a.md"), DetectDepth::Cheap);
    assert_eq!(got, Some(DetectorId("on".into())));
}

#[test]
fn identify_fast_path_does_not_apply_to_directory_target() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(
        &p,
        r#"
                [[detector]]
                id = "junk"
                [[detector.rule]]
                kind = "extension"
                values = ["tmp"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);

    let tmp_dir = dir.path().join("scratch.tmp");
    std::fs::create_dir_all(&tmp_dir).unwrap();
    let got = reg.identify(&FileTarget::new(tmp_dir), DetectDepth::Cheap);
    assert_eq!(got, Some(DetectorId("$directory".into())));
}

#[test]
fn identify_existing_tests_still_pass_after_cutover() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    assert_eq!(
        reg.identify(&target("a/b.md"), DetectDepth::Cheap),
        Some(DetectorId("markdown".into()))
    );
}

#[test]
fn install_order_persists_across_patch_from_other_origin() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let initial = reg
        .detector(&DetectorId("markdown".into()))
        .unwrap()
        .install_order;
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("u.toml");
    std::fs::write(
        &p,
        r#"
                [[detector]]
                id = "markdown"
                [[detector.rule]]
                kind = "extension"
                values = ["mdx"]
            "#,
    )
    .unwrap();
    reg.install_user_config(&p);
    let after = reg
        .detector(&DetectorId("markdown".into()))
        .unwrap()
        .install_order;
    assert_eq!(initial, after);
}

#[test]
fn a_poisoned_registry_still_installs_and_identifies() {
    let reg = std::sync::Arc::new(FileFormatRegistry::new());

    let held = std::sync::Arc::clone(&reg);
    let joined = std::thread::spawn(move || {
        let _guard = held.inner.write().expect("fresh rwlock");
        panic!("a thread dies while holding the registry");
    })
    .join();
    assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");
    assert!(reg.inner.read().is_err(), "poison 됐어야 한다");

    install_host_with_markdown(&reg);
    assert!(
        reg.identify(&target("readme.md"), DetectDepth::Cheap)
            .is_some(),
        "poison 이후에도 설치가 반영되고 식별이 된다"
    );
}

#[test]
fn identify_does_not_extension_match_a_url_target() {
    let reg = FileFormatRegistry::new();
    install_host_with_markdown(&reg);
    let url = target("https://example.com/a.md");
    assert_eq!(reg.identify(&url, DetectDepth::Cheap), None);
    assert_eq!(reg.identify(&url, DetectDepth::Deep), None);
    assert_eq!(
        reg.identify(&target("/tmp/a.md"), DetectDepth::Cheap),
        Some(DetectorId("markdown".into())),
    );
}

#[test]
fn identify_does_not_path_glob_match_a_url_target() {
    let reg = FileFormatRegistry::new();
    let decls = vec![DetectorDecl {
        id: "dockerfile".into(),
        display_name_i18n_key: None,
        icon: None,
        disabled: None,
        rule: vec![DetectorRuleDecl::PathGlob {
            pattern: "Dockerfile".into(),
        }],
    }];
    reg.install_plugin_detectors("com.example.docker", &decls);
    assert_eq!(
        reg.identify(&target("/repo/Dockerfile"), DetectDepth::Cheap),
        Some(DetectorId("dockerfile".into())),
    );
    assert_eq!(
        reg.identify(
            &target("https://example.com/repo/Dockerfile"),
            DetectDepth::Cheap
        ),
        None,
    );
}

fn plugin_markdown_decl(icon: &str, disabled: Option<bool>) -> DetectorDecl {
    DetectorDecl {
        id: "markdown".into(),
        display_name_i18n_key: Some("plugin.markdown".into()),
        icon: Some(icon.into()),
        disabled,
        rule: vec![DetectorRuleDecl::Extension {
            values: vec!["md".into()],
        }],
    }
}

fn user_config(body: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("file-formats.toml");
    std::fs::write(&p, body).unwrap();
    (dir, p)
}

fn markdown(reg: &FileFormatRegistry) -> FileFormatDetector {
    reg.detector(&DetectorId("markdown".into()))
        .expect("markdown detector")
}

#[test]
fn a_user_patch_wins_over_a_plugin_installed_after_the_boot_load() {
    let reg = FileFormatRegistry::new();
    reg.install_host_defaults(crate::HOST_DEFAULTS_TOML);
    let (_dir, p) = user_config(
        r#"
            [[detector]]
            id = "markdown"
            icon = "user-icon"
            display_name_i18n_key = "user.markdown"
        "#,
    );
    reg.install_user_config(&p);
    reg.install_plugin_detectors("com.tasty.markdown", &[plugin_markdown_decl("p", None)]);

    let det = markdown(&reg);
    assert_eq!(det.icon.as_deref(), Some("user-icon"));
    assert_eq!(det.display_name_i18n_key.as_deref(), Some("user.markdown"));
}

#[test]
fn a_user_patch_wins_over_a_plugin_that_contributes_again_without_a_reload() {
    let reg = FileFormatRegistry::new();
    let (_dir, p) = user_config(
        r#"
            [[detector]]
            id = "markdown"
            icon = "user-icon"
        "#,
    );
    reg.install_plugin_detectors("com.tasty.markdown", &[plugin_markdown_decl("p", None)]);
    reg.install_user_config(&p);
    assert_eq!(markdown(&reg).icon.as_deref(), Some("user-icon"));

    reg.uninstall_plugin("com.tasty.markdown");
    reg.install_plugin_detectors("com.tasty.markdown", &[plugin_markdown_decl("p", None)]);
    assert_eq!(markdown(&reg).icon.as_deref(), Some("user-icon"));
}

#[test]
fn a_user_enable_beats_a_plugin_disable_across_boot_and_plugin_restart() {
    let reg = FileFormatRegistry::new();
    let (_dir, p) = user_config(
        r#"
            [[detector]]
            id = "markdown"
            disabled = false
        "#,
    );
    reg.install_user_config(&p);
    reg.install_plugin_detectors(
        "com.tasty.markdown",
        &[plugin_markdown_decl("p", Some(true))],
    );
    assert!(!markdown(&reg).disabled, "부팅 뒤 user 의 켜기가 졌다");

    reg.uninstall_plugin("com.tasty.markdown");
    reg.install_plugin_detectors(
        "com.tasty.markdown",
        &[plugin_markdown_decl("p", Some(true))],
    );
    assert!(
        !markdown(&reg).disabled,
        "plugin 재기동 뒤 user 의 켜기가 졌다"
    );

    assert!(reg.export_user_config().contains("disabled = false"));
}

#[test]
fn a_plugin_disabled_false_does_not_enable_what_the_user_disabled() {
    let reg = FileFormatRegistry::new();
    let (_dir, p) = user_config(
        r#"
            [[detector]]
            id = "markdown"
            disabled = true
        "#,
    );
    reg.install_user_config(&p);
    reg.install_plugin_detectors(
        "com.tasty.markdown",
        &[plugin_markdown_decl("p", Some(false))],
    );
    assert!(markdown(&reg).disabled);
}

#[test]
fn a_plugin_overrides_a_host_default_whatever_the_install_order() {
    let host = r#"
        [[detector]]
        id = "markdown"
        icon = "host-icon"
        [[detector.rule]]
        kind = "extension"
        values = ["md"]
    "#;
    let reg = FileFormatRegistry::new();
    reg.install_plugin_detectors("com.tasty.markdown", &[plugin_markdown_decl("p", None)]);
    reg.install_host_defaults(host);
    assert_eq!(markdown(&reg).icon.as_deref(), Some("p"));
}

#[test]
fn the_user_entry_is_seen_the_same_after_boot_and_after_a_reload() {
    let reg = FileFormatRegistry::new();
    reg.install_host_defaults(crate::HOST_DEFAULTS_TOML);
    let (_dir, p) = user_config(
        r#"
            [[detector]]
            id = "markdown"
            icon = "user-icon"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#,
    );
    reg.install_user_config(&p);
    reg.install_plugin_detectors("com.tasty.markdown", &[plugin_markdown_decl("p", None)]);
    let id = DetectorId("markdown".into());
    let plugin = RuleOrigin::Plugin("com.tasty.markdown".into());

    assert!(
        markdown(&reg)
            .rules
            .iter()
            .all(|r| !matches!(r.origin, RuleOrigin::User))
    );
    let seen = |reg: &FileFormatRegistry| (reg.has_user_contribution(&id), reg.rule_origins(&id));
    let after_boot = seen(&reg);
    assert_eq!(after_boot, (true, vec![plugin.clone(), RuleOrigin::User]));

    reg.reload_user_config(&p);
    assert_eq!(seen(&reg), after_boot);

    reg.uninstall_plugin("com.tasty.markdown");
    reg.install_plugin_detectors("com.tasty.markdown", &[plugin_markdown_decl("p", None)]);
    assert_eq!(seen(&reg), after_boot);

    reg.remove_user_detector(&id);
    assert_eq!(seen(&reg), (false, vec![plugin.clone()]));
    reg.set_user_detector_disabled(&id, true);
    assert_eq!(seen(&reg), (true, vec![plugin]));
}
