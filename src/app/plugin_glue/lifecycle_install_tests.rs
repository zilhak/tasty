//! 신뢰 판정에서 거부된 설치는 흔적을 남기지 않고, 명시적으로 고른 설치는 성공한다.
use std::sync::Arc;

use super::install_into;
use crate::app::command::CoreEvent;
use crate::app::plugin_glue::install_trust::{InstallChoice, UnverifiedRefused};
use crate::plugin::PluginManager;
use tasty_host_plugin::known_plugins::KnownPlugins;
use tasty_host_plugin::trust_verdict::UnverifiedCause;
use tasty_terminal::waker_factory::NoopWakerFactory;

const ID: &str = "com.example.refused-install";

struct TempHome {
    dir: tempfile::TempDir,
}
impl TempHome {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        tasty_utils::path::push_home_override(dir.path().to_path_buf());
        Self { dir }
    }
}
impl Drop for TempHome {
    fn drop(&mut self) {
        tasty_utils::path::pop_home_override();
    }
}

fn unsigned_source() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        dir.path().join("tasty-plugin.toml"),
        format!(
            r#"
manifest_version = 1
id = "{ID}"
name = "Refused"
version = "0.1.0"
api_version = "1"
[entry]
type = "process"
command = "refused-bin"
"#
        ),
    )
    .expect("manifest");
    dir
}

/// 같은 ID의 비활성 표시·grant와 다른 플러그인의 기록이 있는 홈.
fn seeded_home() -> TempHome {
    let home = TempHome::new();
    std::fs::write(
        home.dir.path().join("plugins.toml"),
        format!(
            "[disabled]\nids = [\"{ID}\"]\n\n[grants.\"{ID}\"]\ngranted = [\"fs.read\"]\n\n[grants.\"com.example.other\"]\ngranted = [\"network\"]\n"
        ),
    )
    .expect("plugins.toml");
    std::fs::write(
        home.dir.path().join("known-plugins.toml"),
        "[plugins.\"com.example.other\"]\npubkey = \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\"\ntrusted_at = \"2026-10-01T00:00:00Z\"\n",
    )
    .expect("known-plugins.toml");
    home
}

fn manager() -> PluginManager {
    PluginManager::with_registries(
        Arc::new(NoopWakerFactory),
        Arc::new(crate::file::format::FileFormatRegistry::new()),
        Arc::new(crate::file::handler::FileHandlerRegistry::new()),
    )
}

fn read(home: &TempHome, name: &str) -> Vec<u8> {
    std::fs::read(home.dir.path().join(name)).expect("read")
}

#[test]
fn a_refused_install_leaves_files_and_records_as_they_were() {
    let home = seeded_home();
    let src = unsigned_source();
    let mut mgr = manager();
    let plugins_before = read(&home, "plugins.toml");
    let known_before = read(&home, "known-plugins.toml");

    let err = install_into(&mut mgr, src.path(), &InstallChoice::VerifiedOnly)
        .expect_err("an unsigned plugin is refused without a choice");
    let refused = err
        .downcast_ref::<UnverifiedRefused>()
        .expect("the error names the trust cause, not a missing install");
    assert_eq!(refused.cause, UnverifiedCause::Unsigned);
    assert!(!err.to_string().contains("not installed'"), "{err}");

    assert!(!home.dir.path().join("plugins").join(ID).exists());
    assert_eq!(read(&home, "plugins.toml"), plugins_before);
    assert_eq!(read(&home, "known-plugins.toml"), known_before);
    assert!(mgr.config.is_disabled(ID));
    assert!(!mgr.packages().iter().any(|p| p.manifest.id == ID));
}

#[test]
fn an_explicit_choice_installs_after_a_refusal() {
    let home = seeded_home();
    let src = unsigned_source();
    let mut mgr = manager();
    install_into(&mut mgr, src.path(), &InstallChoice::VerifiedOnly).expect_err("refused");

    // 거부가 폴더를 남기지 않으므로 "already installed" 로 막히지 않는다.
    let events = install_into(&mut mgr, src.path(), &InstallChoice::AllowUnverified)
        .expect("explicitly chosen install succeeds");
    assert!(events.iter().any(|e| matches!(
        e,
        CoreEvent::PluginRegistryChanged { plugin_id, .. } if plugin_id == ID
    )));
    // 비활성 표시가 있으면 실행하지 않지만 등록은 실제로 됐다.
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, CoreEvent::PluginEnableToggled { .. }))
    );
    assert!(mgr.packages().iter().any(|p| p.manifest.id == ID));
    assert!(home.dir.path().join("plugins").join(ID).exists());
    assert!(mgr.config.is_disabled(ID));
    let known = KnownPlugins::load().expect("load");
    assert_eq!(
        known.unverified_choice(ID).map(|c| c.cause.as_str()),
        Some("unsigned")
    );
    assert!(known.lookup("com.example.other").is_some());
}
