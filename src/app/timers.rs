//! 메인 루프 시간축의 키 정의와 등록 — `tasty_timer::TimerHub` 의 호스트측 어휘.
//!
//! 주기 작업마다 ticker 스레드를 만드는 대신 여기 키를 등록하고, gui/headless 양쪽
//! 실행부가 `drain_due` 로 받은 키를 `match` 로 실행한다. 등록 규칙·Strict/Lax 판단
//! 기준·가드 중 정지 계약은 `docs/dev-guide/timer-hub.md`.

use std::time::Duration;
use std::time::Instant;

use tasty_timer::Precision;
use tasty_timer::TimerHub;

/// 메인 루프가 시간축으로 굴리는 주기 작업의 키.
///
/// 키 하나 = "전 엔진 순회 스텝 하나". busy 갱신이 이미 전 window + parked 엔진을
/// 순회하는 형태라 엔진별 주기는 필요 없다(필요해지면 `(engine_id, kind)` 로 확장).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tick {
    /// 1Hz. 모든 surface 의 busy(foreground 프로세스) 상태 재평가 + 원격 attach
    /// client 로의 activity forward + 글로벌 훅 / idle-timeout 훅 평가.
    /// headless 는 여기에 plugin pump 안전망도 얹는다.
    Busy,
    /// 3초. 서버측 readonly 뷰의 display mirror 를 live grid 로 갱신하고, client
    /// mirror 의 누적 출력 버퍼를 적용한다(실시간 stream 이 아닌 polling cadence).
    #[cfg(feature = "gui")]
    AttachView,
    /// 일회성. 네이티브 컨텍스트 메뉴가 떠 있는 동안 다음 폴링 프레임을 예약한다.
    #[cfg(feature = "gui")]
    NativeMenu,
}

/// busy 재평가 주기. 사용자가 체감하는 indicator 반응 상한이라 1초를 넘기지 않는다.
pub(crate) const BUSY_TICK_INTERVAL: Duration = Duration::from_secs(1);

/// attach 뷰 갱신 주기. 사용자 확정 UX = 3초/회 — 원격 readonly·mirror 뷰는 실시간
/// stream 이 아니라 이 cadence 로만 렌더를 갱신한다.
#[cfg(feature = "gui")]
pub(crate) const ATTACH_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// 네이티브 컨텍스트 메뉴가 떠 있는 동안의 폴링 주기 상한. 메뉴 트래킹(하이라이트
/// 이동 등)이 사람 눈에 끊겨 보이지 않을 만큼 짧고, idle 상태에서 이벤트 루프를
/// 계속 깨우지는 않을 만큼만 짧다(메뉴가 닫히면 등록 자체가 사라진다).
#[cfg(feature = "gui")]
pub(crate) const PENDING_MENU_POLL_INTERVAL: Duration = Duration::from_millis(8);

/// steady-state 주기 작업 등록 — 부팅 시 1회.
pub(crate) fn register_steady_state(hub: &mut TimerHub<Tick>, now: Instant) {
    hub.every(Tick::Busy, BUSY_TICK_INTERVAL, Precision::Strict, now);
    // headless 는 렌더가 없어 readonly display mirror·client mirror 가 무의미하다.
    #[cfg(feature = "gui")]
    hub.every(
        Tick::AttachView,
        ATTACH_POLL_INTERVAL,
        Precision::Strict,
        now,
    );
}

/// 두 허브 데드라인의 합성 — 둘 다 `None` 이면 깨울 이유가 없다.
///
/// plugin manager 처럼 본체 타입을 모르는 크레이트는 자기 `TimerHub` 를 따로
/// 소유한다. 허브가 여러 개여도 **대기 계산은 하나**여야 하므로, 프레임 말미에
/// 이 함수로 접어 넣는다(`docs/dev-guide/timer-hub.md` "계층을 넘는 허브 합성").
pub(crate) fn min_deadline(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// pending native menu 유무 → 다음 폴링 wakeup 예약/취소.
///
/// 순수 함수로 뽑아 헤드리스 회귀 테스트가 가능하게 했다 — 이 재예약을 빠뜨리면
/// 메뉴가 열린 채 아무 이벤트도 안 오는 순간 폴링이 멈춰 메뉴가 화면에서 얼어붙는다.
#[cfg(feature = "gui")]
pub(crate) fn reschedule_pending_menu_poll(
    hub: &mut TimerHub<Tick>,
    has_pending: bool,
    now: Instant,
) {
    if has_pending {
        hub.once_after(
            Tick::NativeMenu,
            PENDING_MENU_POLL_INTERVAL,
            Precision::Strict,
            now,
        );
    } else {
        hub.cancel(Tick::NativeMenu);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// steady-state 등록이 빠지면 busy indicator·attach mirror 가 통째로 멈춘다.
    #[test]
    fn steady_state_registers_the_busy_cadence() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        register_steady_state(&mut hub, t0);
        assert!(hub.is_registered(Tick::Busy));
        assert_eq!(hub.next_deadline(), Some(t0 + BUSY_TICK_INTERVAL));
        assert_eq!(hub.drain_due(t0 + BUSY_TICK_INTERVAL), vec![Tick::Busy]);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn steady_state_registers_the_attach_cadence() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        register_steady_state(&mut hub, t0);
        assert!(hub.is_registered(Tick::AttachView));
        assert_eq!(
            hub.drain_due(t0 + ATTACH_POLL_INTERVAL),
            vec![Tick::Busy, Tick::AttachView]
        );
    }

    /// 허브가 여럿이어도 대기 계산은 하나 — 가장 이른 데드라인이 이긴다.
    #[test]
    fn min_deadline_folds_two_hubs() {
        let t0 = Instant::now();
        assert_eq!(min_deadline(None, None), None);
        assert_eq!(min_deadline(Some(t0), None), Some(t0));
        assert_eq!(min_deadline(None, Some(t0)), Some(t0));
        assert_eq!(
            min_deadline(Some(t0 + Duration::from_secs(3)), Some(t0)),
            Some(t0)
        );
        assert_eq!(
            min_deadline(Some(t0), Some(t0 + Duration::from_secs(3))),
            Some(t0)
        );
    }

    /// 메뉴가 떠 있으면 짧은 데드라인으로 다음 폴링 프레임을 반드시 예약한다.
    #[cfg(feature = "gui")]
    #[test]
    fn pending_menu_reschedules_a_poll_frame() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        reschedule_pending_menu_poll(&mut hub, true, t0);
        assert_eq!(hub.next_deadline(), Some(t0 + PENDING_MENU_POLL_INTERVAL));
        assert!(
            PENDING_MENU_POLL_INTERVAL <= Duration::from_millis(16),
            "폴링 주기가 한 프레임(60fps)보다 길면 메뉴 트래킹이 끊겨 보인다"
        );
    }

    /// 메뉴가 닫히면 등록을 걷어낸다 — 남겨두면 idle 에서도 8ms 마다 깨운다.
    #[cfg(feature = "gui")]
    #[test]
    fn no_pending_menu_cancels_the_poll() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        reschedule_pending_menu_poll(&mut hub, true, t0);
        reschedule_pending_menu_poll(&mut hub, false, t0);
        assert!(!hub.is_registered(Tick::NativeMenu));
        assert!(hub.next_deadline().is_none());
    }
}
