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

/// 무응답 재시작을 걸어 회수 중으로 만든 manager 와, 번들(src)·설치본(dest) 경로.
fn restarting(home: &HomeEnvGuard) -> (PluginManager, std::path::PathBuf, std::path::PathBuf) {
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
    (mgr, src, dest)
}

fn upgrade(mgr: &mut PluginManager, src: &Path, dest: &Path, installed: u64, bundle: u64) {
    let spec = BuiltinSpec {
        id: ID,
        crate_dir: "",
        bin_name: "",
    };
    apply_builtin_upgrade_decision(
        mgr,
        &spec,
        src,
        dest,
        Some(semver::Version::new(1, installed, 0)),
        Some(semver::Version::new(1, bundle, 0)),
        false,
        false,
    );
}

fn pump_until_running(mgr: &mut PluginManager) {
    let give_up = Instant::now() + Duration::from_secs(10);
    while !mgr.is_running(ID) {
        assert!(
            Instant::now() < give_up,
            "upgrade 뒤 재시작 중이던 plugin 이 다시 뜨지 않았다"
        );
        mgr.pump(Instant::now());
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn shut_down(mgr: &mut PluginManager) {
    mgr.begin_shutdown_all();
    let give_up = Instant::now() + Duration::from_secs(10);
    while !mgr.poll_shutdown_all() {
        assert!(Instant::now() < give_up, "종료가 10 초 안에 안 끝났다");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// 재시작으로 회수 중인 plugin 에 **쓰는** upgrade 가 오면, 회수를 기다려 쓰고 그 뒤에 결국
/// 다시 뜬다 — 기다리며 가져온 재기동 예약을 버리지 않는다.
#[test]
fn an_upgrade_during_a_restart_keeps_the_restart() {
    let home = HomeEnvGuard::tasty_home();
    let (mut mgr, src, dest) = restarting(&home);
    std::fs::write(src.join("new-asset"), "x").expect("bundle change");

    upgrade(&mut mgr, &src, &dest, 0, 0);
    assert_eq!(
        mgr.retiring_count(),
        0,
        "쓰는 갈래는 회수를 끝까지 기다린다"
    );
    assert!(
        dest.join("new-asset").exists(),
        "같은 버전의 바뀐 내용이 안 쓰였다"
    );
    pump_until_running(&mut mgr);
    shut_down(&mut mgr);
}

/// 쓸 것이 없는 upgrade(같은 버전·같은 내용 / 설치본이 더 높음)는 회수를 기다리지 않는다.
/// 회수는 뒤에서 이어지고, 재시작 예약도 그대로 남아 결국 다시 뜬다.
#[test]
fn an_upgrade_that_writes_nothing_does_not_wait_for_a_retirement() {
    for (installed, bundle) in [(0, 0), (1, 0)] {
        let home = HomeEnvGuard::tasty_home();
        let (mut mgr, src, dest) = restarting(&home);

        let t = Instant::now();
        upgrade(&mut mgr, &src, &dest, installed, bundle);
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "쓸 것이 없는 upgrade(v1.{installed} ← v1.{bundle})가 회수를 기다렸다: {:?}",
            t.elapsed()
        );
        assert_eq!(
            mgr.retiring_respawn(ID),
            Some(true),
            "재시작 예약이 사라졌다"
        );
        pump_until_running(&mut mgr);
        shut_down(&mut mgr);
    }
}
