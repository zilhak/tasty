//! Namespace admission rejects before allocating a listener or starting a process.
use std::sync::Arc;

use super::PluginManager;
use crate::test_support::HomeEnvGuard;
use tasty_plugin_manifest::{Manifest, PluginPackage};
use tasty_terminal::waker_factory::NoopWakerFactory;

fn owner_manager() -> PluginManager {
    let manifest: Manifest = toml::from_str(
        r#"manifest_version=1
id="com.example.owner"
name="Owner"
version="0.1.0"
api_version="1"
[entry]
type="process"
command="must-not-start"
[[contributes.ipc_namespace]]
prefix="owner"
"#,
    )
    .unwrap();
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    mgr.set_packages_for_tests(vec![PluginPackage {
        dir: std::path::PathBuf::from("/nonexistent/owner"),
        manifest,
    }]);
    mgr
}

#[test]
fn unknown_or_denied_namespace_does_not_start_a_listener() {
    let _home = HomeEnvGuard::tasty_home();
    let mut mgr = owner_manager();
    assert_eq!(
        mgr.validate_namespace_call("unknown.echo", None)
            .unwrap_err()
            .0,
        -32601
    );
    for caller in ["com.example.owner", "com.example.unprivileged"] {
        assert_eq!(
            mgr.validate_namespace_call("owner.echo", Some(caller))
                .unwrap_err()
                .0,
            -32001
        );
    }
    assert!(mgr.listener.is_none());
    assert!(mgr.processes.is_empty());
}

#[test]
fn disabled_and_auto_disabled_owners_remain_stopped() {
    let home = HomeEnvGuard::tasty_home();
    let mut mgr = owner_manager();
    mgr.disable("com.example.owner").unwrap();
    let path = home.path().join("plugins.toml");
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(
        mgr.validate_namespace_call("owner.echo", None)
            .unwrap_err()
            .0,
        -32002
    );
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    assert!(mgr.listener.is_none());
    assert!(mgr.processes.is_empty());
    let mut mgr = owner_manager();
    // Separate the automatic health stop from the user-disabled configuration.
    mgr.config.enable("com.example.owner");
    mgr.auto_disabled.insert("com.example.owner".into());
    assert_eq!(
        mgr.validate_namespace_call("owner.echo", None)
            .unwrap_err()
            .0,
        -32002
    );
    assert!(mgr.listener.is_none());
    assert!(mgr.processes.is_empty());
}
