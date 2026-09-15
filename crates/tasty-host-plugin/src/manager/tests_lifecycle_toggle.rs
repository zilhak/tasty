//! 미설치 plugin 토글은 메모리 상태와 설정 파일을 바꾸지 않는다.
use std::sync::Arc;

use super::PluginManager;
use crate::test_support::HomeEnvGuard;
use tasty_terminal::waker_factory::NoopWakerFactory;

const MISSING: &str = "com.example.never.installed";

#[test]
fn unknown_toggles_do_not_create_a_config_file() {
    let home = HomeEnvGuard::tasty_home();
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    let before = toml::to_string(&mgr.config).unwrap();
    for enable in [true, false] {
        let result = if enable {
            mgr.enable(MISSING)
        } else {
            mgr.disable(MISSING)
        };
        assert_eq!(
            result.unwrap_err().to_string(),
            format!("plugin '{MISSING}' not installed")
        );
        assert_eq!(toml::to_string(&mgr.config).unwrap(), before);
        assert!(!home.path().join("plugins.toml").exists());
        assert!(!mgr.is_running(MISSING));
    }
}

#[test]
fn unknown_toggles_preserve_existing_config_and_recovery_state() {
    let home = HomeEnvGuard::tasty_home();
    let path = home.path().join("plugins.toml");
    // Preserve even an old dangling disabled mark; rejection is not a cleanup command.
    let bytes = format!(
        "# user formatting stays intact\n[disabled]\nids = [\"{MISSING}\", \"com.example.other\"]\n"
    );
    std::fs::write(&path, &bytes).unwrap();
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    mgr.auto_disabled.insert(MISSING.to_string());
    let before = toml::to_string(&mgr.config).unwrap();
    for enable in [true, false] {
        let result = if enable {
            mgr.enable(MISSING)
        } else {
            mgr.disable(MISSING)
        };
        assert!(result.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), bytes.as_bytes());
        assert_eq!(toml::to_string(&mgr.config).unwrap(), before);
        assert!(mgr.is_auto_disabled(MISSING));
        assert!(!mgr.is_running(MISSING));
    }
}
