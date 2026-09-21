//! `upgrade-builtins` 가 회수 중인 plugin 의 재기동 예약을 잇는다는 것.
//!
//! 무응답 재시작은 옛 프로세스를 뒤에서 회수하고 회수가 끝나면 다시 띄운다
//! (`manager::retire`). 그 사이 `upgrade-builtins` 가 그 디렉토리에 쓰려고 회수를 기다리면
//! 예약은 회수 기록과 함께 사라진다 — 그것을 이어받지 않으면 enabled 인 plugin 이 꺼진 채
//! 남는다.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::{BuiltinSpec, apply_builtin_upgrade_decision};
use crate::manager::PluginManager;
use crate::process::PluginProcess;
use crate::test_support::HomeEnvGuard;
use tasty_terminal::waker_factory::NoopWakerFactory;

const ID: &str = "com.example.test";

const MANIFEST: &str = r#"
manifest_version = 1
id = "com.example.test"
name = "Test"
version = "1.0.0"
api_version = "1"
[entry]
type = "process"
command = "fake.sh"
"#;

/// 인증 한 줄을 보내고 소켓이 닫힐 때까지 읽기만 하는 plugin — 기동이 성공하는 데 필요한
/// 만큼만 한다.
const FAKE_PLUGIN: &str = r#"#!/bin/bash
exec 3<>/dev/tcp/127.0.0.1/"$TASTY_HOST_IPC_PORT"
printf '{"plugin_id":"%s","token":"%s"}\n' "$TASTY_PLUGIN_ID" "$TASTY_PLUGIN_TOKEN" >&3
exec cat <&3 >/dev/null
"#;

fn write_plugin(dir: &Path) {
    std::fs::create_dir_all(dir).expect("plugin dir");
    std::fs::write(dir.join("tasty-plugin.toml"), MANIFEST).expect("manifest");
    let entry = dir.join("fake.sh");
    std::fs::write(&entry, FAKE_PLUGIN).expect("entry");
    std::fs::set_permissions(&entry, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

/// 재시작으로 회수 중인 plugin 에 `upgrade-builtins` 가 오면, 쓰기를 마친 뒤 결국 다시 뜬다.
#[test]
fn an_upgrade_during_a_restart_keeps_the_restart() {
    let home = HomeEnvGuard::tasty_home();
    let src = home.path().join("bundle");
    let dest = home.path().join("installed");
    write_plugin(&src);
    write_plugin(&dest);

    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    mgr.set_packages_for_tests(vec![tasty_plugin_manifest::PluginPackage {
        dir: dest.clone(),
        manifest: toml::from_str(MANIFEST).expect("fixture manifest"),
    }]);
    let stalled = Command::new("sleep").arg("30").spawn().expect("child");
    let proc = PluginProcess::stub_with_child(ID, stalled);
    proc.backdate_pong_for_test(Duration::from_secs(120));
    mgr.processes.insert(ID.into(), proc);
    mgr.restart_unresponsive_for_test();
    assert_eq!(
        mgr.retiring_respawn(ID),
        Some(true),
        "재시작이 예약돼야 한다"
    );

    let v = semver::Version::new(1, 0, 0);
    let spec = BuiltinSpec {
        id: ID,
        crate_dir: "",
        bin_name: "",
    };
    apply_builtin_upgrade_decision(
        &mut mgr,
        &spec,
        &src,
        &dest,
        Some(v.clone()),
        Some(v),
        false,
        false,
    );

    let give_up = Instant::now() + Duration::from_secs(10);
    while !mgr.is_running(ID) {
        assert!(
            Instant::now() < give_up,
            "upgrade 뒤 재시작 중이던 plugin 이 다시 뜨지 않았다"
        );
        mgr.pump(Instant::now());
        std::thread::sleep(Duration::from_millis(20));
    }

    mgr.begin_shutdown_all();
    let give_up = Instant::now() + Duration::from_secs(10);
    while !mgr.poll_shutdown_all() {
        assert!(Instant::now() < give_up, "종료가 10 초 안에 안 끝났다");
        std::thread::sleep(Duration::from_millis(20));
    }
}
