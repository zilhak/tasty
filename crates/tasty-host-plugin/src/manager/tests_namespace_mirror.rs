//! G.D.c — `IpcNamespaceRegistry` ↔ `tasty_ipc::method_meta` runtime registry
//! mirror 동작 통합 테스트.
//!
//! `start_plugin_internal` / `disable` / `pump` 의 unresponsive restart 가
//! 호출하는 mirror loop 의 *진짜 lifecycle path* 는 process spawn 까지 가야
//! 동작하므로 본 테스트에서는 *manifest 모양 + mirror API 의 동조성* 만 검증.

use std::sync::Mutex;

use tasty_plugin_manifest::Manifest;

/// 본 테스트는 process-global PLUGIN_PREFIXES 를 건드리므로 직렬화.
static TEST_LOCK: Mutex<()> = Mutex::new(());

fn test_lock() -> std::sync::MutexGuard<'static, ()> {
    TEST_LOCK
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

const FAKE_MANIFEST: &str = r#"
manifest_version = 1
id = "com.example.namespace_mirror_test"
name = "Namespace Mirror Test"
version = "0.0.1"
api_version = "1.0"

[entry]
type = "process"
command = "echo"
args = []

[[contributes.ipc_namespace]]
prefix = "nstest"
"#;

fn parse_fake_manifest() -> Manifest {
    toml::from_str(FAKE_MANIFEST).expect("fake manifest should parse")
}

#[test]
fn lookup_returns_none_before_any_registration() {
    let _g = test_lock();
    tasty_ipc::method_meta::clear_plugin_prefixes_for_tests();
    assert!(
        tasty_ipc::method_meta::method_meta("nstest.invoke").is_none(),
        "no plugin registered → runtime registry empty"
    );
}

#[test]
fn manifest_driven_register_makes_method_resolvable() {
    let _g = test_lock();
    tasty_ipc::method_meta::clear_plugin_prefixes_for_tests();

    let manifest = parse_fake_manifest();
    // lifecycle.rs:207 의 mirror loop 와 동일한 형태로 등록.
    for ns in &manifest.contributes.ipc_namespace {
        tasty_ipc::method_meta::register_plugin_prefix(&ns.prefix);
    }

    let m = tasty_ipc::method_meta::method_meta("nstest.invoke")
        .expect("registered via runtime mirror");
    assert!(m.plugin_callable);
    assert!(m.required.is_empty());

    tasty_ipc::method_meta::clear_plugin_prefixes_for_tests();
}

#[test]
fn disable_path_unregisters_prefix() {
    let _g = test_lock();
    tasty_ipc::method_meta::clear_plugin_prefixes_for_tests();

    let manifest = parse_fake_manifest();
    for ns in &manifest.contributes.ipc_namespace {
        tasty_ipc::method_meta::register_plugin_prefix(&ns.prefix);
    }
    assert!(tasty_ipc::method_meta::method_meta("nstest.invoke").is_some());

    // disable 경로의 mirror unregister (lifecycle.rs:disable + pump.rs unresponsive).
    for ns in &manifest.contributes.ipc_namespace {
        tasty_ipc::method_meta::unregister_plugin_prefix(&ns.prefix);
    }
    assert!(
        tasty_ipc::method_meta::method_meta("nstest.invoke").is_none(),
        "unregister mirror loop should clear runtime registry"
    );
}

#[test]
fn enable_disable_reenable_cycle_consistent() {
    let _g = test_lock();
    tasty_ipc::method_meta::clear_plugin_prefixes_for_tests();

    let manifest = parse_fake_manifest();

    // enable
    for ns in &manifest.contributes.ipc_namespace {
        tasty_ipc::method_meta::register_plugin_prefix(&ns.prefix);
    }
    assert!(tasty_ipc::method_meta::method_meta("nstest.invoke").is_some());

    // disable
    for ns in &manifest.contributes.ipc_namespace {
        tasty_ipc::method_meta::unregister_plugin_prefix(&ns.prefix);
    }
    assert!(tasty_ipc::method_meta::method_meta("nstest.invoke").is_none());

    // re-enable
    for ns in &manifest.contributes.ipc_namespace {
        tasty_ipc::method_meta::register_plugin_prefix(&ns.prefix);
    }
    assert!(tasty_ipc::method_meta::method_meta("nstest.invoke").is_some());

    tasty_ipc::method_meta::clear_plugin_prefixes_for_tests();
}
