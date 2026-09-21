//! 단건 종료(disable · 무응답 재시작)가 메인 스레드를 세우지 않는다는 것, 그리고 옛
//! 프로세스가 빠지기 전에 새 프로세스를 띄우지 않는다는 것(`manager::retire`).
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::PluginManager;
use crate::process::PluginProcess;
use crate::test_support::HomeEnvGuard;
use tasty_terminal::waker_factory::NoopWakerFactory;

const ID: &str = "com.example.test";

/// shutdown 요청을 못 받는 자식 — deadline 까지 살아 있다가 kill 된다.
fn slow_child() -> Child {
    #[cfg(unix)]
    {
        Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("test child spawn")
    }
    #[cfg(windows)]
    {
        Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 > nul"])
            .spawn()
            .expect("test child spawn")
    }
}

fn installed(home: &HomeEnvGuard) -> PluginManager {
    let mut mgr = PluginManager::new(Arc::new(NoopWakerFactory));
    mgr.set_packages_for_tests(vec![tasty_plugin_manifest::PluginPackage {
        dir: home.path().join("plugin"),
        manifest: toml::from_str(
            r#"
manifest_version = 1
id = "com.example.test"
name = "Test"
version = "1.0.0"
api_version = "1"
[entry]
type = "process"
command = "unused"
"#,
        )
        .expect("fixture manifest"),
    }]);
    mgr
}

/// 회수가 끝날 때까지 pump 를 돌리고, 가장 오래 걸린 pump 한 번의 시간을 돌려준다.
fn pump_until_retired(mgr: &mut PluginManager) -> Duration {
    let give_up = Instant::now() + Duration::from_secs(10);
    let mut longest = Duration::ZERO;
    while mgr.retiring_count() > 0 {
        assert!(Instant::now() < give_up, "회수가 10 초 안에 안 끝났다");
        let t = Instant::now();
        mgr.pump(Instant::now());
        longest = longest.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(20));
    }
    longest
}

/// disable 은 자식이 빠질 때까지 기다리지 않는다 — 예전에는 최대 2 초 동안 메인 스레드가
/// 섰다. 회수는 뒤에서 끝나고, 그동안 pump 한 번도 서지 않는다.
#[test]
fn disable_returns_before_a_stalled_child_exits() {
    let home = HomeEnvGuard::tasty_home();
    let mut mgr = installed(&home);
    mgr.processes
        .insert(ID.into(), PluginProcess::stub_with_child(ID, slow_child()));

    let t = Instant::now();
    mgr.disable(ID).expect("disable");
    let took = t.elapsed();
    assert!(
        took < Duration::from_millis(500),
        "disable 이 회수를 기다렸다: {took:?}"
    );
    assert_eq!(mgr.retiring_count(), 1, "회수가 뒤에 남아 있어야 한다");
    assert_eq!(mgr.retiring_respawn(ID), Some(false));

    let longest = pump_until_retired(&mut mgr);
    assert!(
        longest < Duration::from_millis(500),
        "회수를 거두는 pump 가 섰다: {longest:?}"
    );
    assert!(
        t.elapsed() >= Duration::from_millis(1500),
        "shutdown 을 못 받는 자식은 deadline(2 초)까지 기회를 받아야 한다: {:?}",
        t.elapsed()
    );
    assert!(!mgr.is_running(ID));
}

/// 회수 중에 온 enable 은 곧바로 띄우지 않고 회수 뒤로 미룬다. 그 뒤 다시 온 disable 은
/// 그 예약을 거둔다.
#[test]
fn enable_during_retirement_is_deferred_and_a_later_disable_cancels_it() {
    let home = HomeEnvGuard::tasty_home();
    let mut mgr = installed(&home);
    mgr.processes
        .insert(ID.into(), PluginProcess::stub_with_child(ID, slow_child()));
    mgr.disable(ID).expect("disable");

    mgr.enable(ID).expect("enable");
    assert!(
        !mgr.is_running(ID),
        "옛 프로세스가 빠지기 전에 새 프로세스가 떴다"
    );
    assert_eq!(mgr.retiring_respawn(ID), Some(true));

    mgr.disable(ID).expect("disable again");
    assert_eq!(mgr.retiring_respawn(ID), Some(false));
    pump_until_retired(&mut mgr);
    assert!(!mgr.is_running(ID), "disable 로 거둔 예약이 기동됐다");
}

/// 무응답 재시작은 옛 프로세스를 뒤에서 회수하고, 새 프로세스를 **그 자리에서** 띄우지
/// 않는다 — 회수가 끝나는 tick 에 띄운다.
#[test]
fn an_unresponsive_restart_waits_for_the_old_process_before_starting() {
    let home = HomeEnvGuard::tasty_home();
    let mut mgr = installed(&home);
    let proc = PluginProcess::stub_with_child(ID, slow_child());
    proc.backdate_pong_for_test(super::HEALTHCHECK_TIMEOUT + Duration::from_secs(1));
    mgr.processes.insert(ID.into(), proc);

    let t = Instant::now();
    mgr.restart_unresponsive_plugins();
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "재시작이 회수를 기다렸다: {:?}",
        t.elapsed()
    );
    assert!(
        !mgr.is_running(ID),
        "옛 프로세스가 빠지기 전에 새 프로세스가 떴다"
    );
    assert_eq!(mgr.retiring_respawn(ID), Some(true));
    // 회수가 끝난 뒤의 기동 자체는 이 시험의 몫이 아니다(실제 실행 파일이 없다).
    pump_until_retired(&mut mgr);
}

/// 호스트 종료는 단건 경로로 내려가던 plugin 도 끝날 때까지 기다리고, 다시 띄우지 않는다.
#[test]
fn exit_waits_for_a_retiring_plugin_and_does_not_respawn_it() {
    let home = HomeEnvGuard::tasty_home();
    let mut mgr = installed(&home);
    let proc = PluginProcess::stub_with_child(ID, slow_child());
    proc.backdate_pong_for_test(super::HEALTHCHECK_TIMEOUT + Duration::from_secs(1));
    mgr.processes.insert(ID.into(), proc);
    mgr.restart_unresponsive_plugins();

    mgr.begin_shutdown_all();
    assert!(
        !mgr.poll_shutdown_all(),
        "회수 중인 plugin 을 두고 종료가 끝났다"
    );
    assert_eq!(mgr.retiring_respawn(ID), Some(false));
    let give_up = Instant::now() + Duration::from_secs(10);
    while !mgr.poll_shutdown_all() {
        assert!(Instant::now() < give_up, "종료가 10 초 안에 안 끝났다");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(mgr.retiring_count(), 0);
    assert!(!mgr.is_running(ID));
}

/// 디렉토리를 지우거나 덮어쓰는 호출자는 옛 프로세스가 사라질 때까지 기다린다.
#[test]
fn wait_retired_blocks_until_the_child_is_gone() {
    let home = HomeEnvGuard::tasty_home();
    let mut mgr = installed(&home);
    mgr.processes
        .insert(ID.into(), PluginProcess::stub_with_child(ID, slow_child()));
    mgr.disable(ID).expect("disable");
    mgr.wait_retired(ID);
    assert_eq!(mgr.retiring_count(), 0);
}
