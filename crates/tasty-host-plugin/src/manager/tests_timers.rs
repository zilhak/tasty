//! plugin 주기 작업(`PluginTick`) 스케줄 단위 테스트.
//!
//! `pump(now)` 가 기준시각을 인자로 받으므로 시간 주입만으로 결정론적으로 검증할 수
//! 있다 — 실제 sleep 없이 "15초 뒤" 를 재현한다. 발화 관측은 허브 스냅샷의
//! `last_fired` 로 한다(프로세스가 없는 매니저라 ping 자체는 부수효과가 없다).

use std::sync::Arc;
use std::time::{Duration, Instant};

use tasty_terminal::waker_factory::NoopWakerFactory;

use super::{
    AUTO_RELOAD_POLL_INTERVAL, PING_INTERVAL, PluginManager, PluginTick, RSS_SAMPLE_INTERVAL,
};

fn mgr() -> PluginManager {
    PluginManager::new(Arc::new(NoopWakerFactory))
}

/// 허브에서 이 키가 마지막으로 발화한 시각.
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

    // 같은 주기 안에서 여러 번 pump 해도 다시 발화하지 않는다.
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
    assert_eq!(last_fired(&m, PluginTick::Rss), None, "30초 전에는 안 뜬다");

    m.pump(t0 + RSS_SAMPLE_INTERVAL);
    assert_eq!(
        last_fired(&m, PluginTick::Rss),
        Some(t0 + RSS_SAMPLE_INTERVAL)
    );
}

/// RSS 는 Lax 라 **스스로 호스트를 깨우지 않는다** — 30초 시점이 데드라인이지만
/// hard deadline 은 ping 주기만큼 뒤다. 그래서 `next_deadline()` 은 항상 ping 이
/// 결정한다(관측용 샘플링이 idle wakeup 을 늘리지 않는다).
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
