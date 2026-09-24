//! pump에 시각을 전달해 주기 작업의 실행 시점을 검사한다.
//! 프로세스는 실행하지 않으며 TimerHub의 last_fired를 확인한다.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tasty_terminal::waker_factory::NoopWakerFactory;

use super::{
    AUTO_RELOAD_POLL_INTERVAL, PING_INTERVAL, PluginManager, PluginTick, RSS_SAMPLE_INTERVAL,
};

fn mgr() -> PluginManager {
    PluginManager::new(Arc::new(NoopWakerFactory))
}

/// 이 타이머를 마지막으로 실행한 시각.
fn last_fired(m: &PluginManager, key: PluginTick) -> Option<Instant> {
    m.timers
        .snapshot()
        .into_iter()
        .find(|s| s.key == key)
        .and_then(|s| s.last_fired)
}

#[test]
fn ping_fires_once_per_interval() {
    let mut m = mgr();
    let t0 = Instant::now();

    m.pump(t0); // 등록 직후 — 아직 도래 전
    assert_eq!(last_fired(&m, PluginTick::Ping), None);

    m.pump(t0 + PING_INTERVAL - Duration::from_secs(1)); // 미도래
    assert_eq!(last_fired(&m, PluginTick::Ping), None);

    m.pump(t0 + PING_INTERVAL); // 도래
    assert_eq!(last_fired(&m, PluginTick::Ping), Some(t0 + PING_INTERVAL));

    // 같은 주기에는 여러 번 호출해도 다시 실행하지 않는다.
    m.pump(t0 + PING_INTERVAL + Duration::from_secs(1));
    assert_eq!(last_fired(&m, PluginTick::Ping), Some(t0 + PING_INTERVAL));

    // 다음 주기에 한 번 더.
    m.pump(t0 + PING_INTERVAL * 2);
    assert_eq!(
        last_fired(&m, PluginTick::Ping),
        Some(t0 + PING_INTERVAL * 2)
    );
}

#[test]
fn rss_sampling_keeps_its_own_cadence() {
    let mut m = mgr();
    let t0 = Instant::now();

    m.pump(t0 + PING_INTERVAL);
    assert_eq!(
        last_fired(&m, PluginTick::Rss),
        None,
        "30초 전에는 RSS 타이머를 실행하지 않는다"
    );

    m.pump(t0 + RSS_SAMPLE_INTERVAL);
    assert_eq!(
        last_fired(&m, PluginTick::Rss),
        Some(t0 + RSS_SAMPLE_INTERVAL)
    );
}

/// 처음 등록한 상태에서는 RSS의 slack을 포함한 시각보다 ping 시각이 빠르다.
#[test]
fn rss_never_advances_the_wakeup_deadline() {
    let m = mgr();
    let deadline = m.next_deadline().expect("ping is always registered");
    let ping_due = m
        .timers
        .snapshot()
        .into_iter()
        .find(|s| s.key == PluginTick::Ping)
        .expect("ping entry")
        .next_due;
    assert_eq!(deadline, ping_due);
}

#[test]
fn auto_reload_timer_is_absent_while_flag_is_off() {
    let m = mgr();
    assert!(!m.auto_reload_enabled, "default off");
    assert!(
        !m.timers.is_registered(PluginTick::AutoReload),
        "꺼진 기능이 데드라인에 기여하면 안 된다"
    );
}

#[test]
fn enabling_auto_reload_registers_its_timer_and_disabling_cancels_it() {
    let mut m = mgr();
    let t0 = Instant::now();

    m.set_auto_reload_enabled(true, t0);
    assert!(m.timers.is_registered(PluginTick::AutoReload));
    // 2초 주기라 ping(15초)보다 먼저 깨워야 한다.
    assert_eq!(m.next_deadline(), Some(t0 + AUTO_RELOAD_POLL_INTERVAL));

    m.pump(t0 + AUTO_RELOAD_POLL_INTERVAL);
    assert_eq!(
        last_fired(&m, PluginTick::AutoReload),
        Some(t0 + AUTO_RELOAD_POLL_INTERVAL)
    );

    m.set_auto_reload_enabled(false, t0 + AUTO_RELOAD_POLL_INTERVAL);
    assert!(!m.timers.is_registered(PluginTick::AutoReload));
    assert!(
        m.next_deadline()
            .is_some_and(|d| d > t0 + AUTO_RELOAD_POLL_INTERVAL),
        "auto-reload 를 끄면 2초 데드라인이 사라지고 ping 만 남는다"
    );
}
