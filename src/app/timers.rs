//! 호스트 타이머의 키와 등록 정책. GUI·헤드리스가 drain_due 결과를 각 실행부에서 처리한다.
//! 등록·중지 규칙: docs/dev-guide/timer-hub.md.

use std::time::Duration;
use std::time::Instant;

use tasty_timer::Precision;
use tasty_timer::TimerHub;

/// DAG 뷰가 정한 읽기 주기를 지난 데드라인 보정에도 사용한다.
#[cfg(feature = "gui")]
use crate::adapters::ui::surface::dag_graph::view::POLL_INTERVAL as DAG_POLL_INTERVAL;

/// 키별 작업이 필요한 engine을 순회하므로 engine마다 반복 타이머를 두지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tick {
    /// busy 상태·원격 activity·전역/idle 훅을 확인한다. 헤드리스는 플러그인 pump도 실행한다.
    Busy,
    /// 서버 readonly 화면 갱신과 클라이언트 출력 큐 적용. 스트림 알림에 따른 즉시 적용과 별개다.
    #[cfg(feature = "gui")]
    AttachView,
    #[cfg(feature = "gui")]
    NativeMenu,
    /// Linux native webview의 별도 이벤트 루프가 받은 키를 호스트에서 처리하도록 깨운다.
    /// 실제 처리는 pump_webview_key_events가 하며 macOS·Windows에는 예약하지 않는다.
    #[cfg(feature = "gui")]
    WebviewKeyPoll,
    /// 변경이 있을 때만 예약해 레이아웃을 저장한다. 반복 타이머로 두면 idle에서도 계속 깨어난다.
    #[cfg(feature = "gui")]
    LayoutFlush,
    /// 보이는 DAG surface의 조회 예약. 닫히거나 배경으로 가면 취소해야 한다.
    #[cfg(feature = "gui")]
    DagGraph(u32),
    /// surface에 속하지 않는 DAG 목록 popup은 별도 키로 예약한다.
    #[cfg(feature = "gui")]
    DagListPopup,
    /// anchor 재연결 backoff 시각에 깨운다. workspace로 돌아온 순간의 재시도 판정은 별도다.
    #[cfg(feature = "gui")]
    Reconnect(u32),
    /// 접근 시 정리를 보완해 요청이 없어도 TTL이 지난 headless PTY를 회수한다.
    PtySweep,
    /// 다음 청크가 오지 않아도 만료한 업로드를 정리한다.
    CaptureSweep,
    /// 로그 추가가 없어도 보존 정책을 확인한다. 실제 실행 간격은 log_retention이 따로 제한한다.
    LogPrune,
}

/// busy 상태 확인 주기. 이벤트 루프 지연까지 포함한 반응 시간 상한은 아니다.
pub(crate) const BUSY_TICK_INTERVAL: Duration = Duration::from_secs(1);

/// 서버 readonly 화면과 클라이언트 출력 큐를 주기적으로 확인하는 간격.
#[cfg(feature = "gui")]
pub(crate) const ATTACH_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// 마지막 변경이 아니라 첫 dirty 시각을 기준으로 저장을 예약한다.
#[cfg(feature = "gui")]
pub(crate) const LAYOUT_FLUSH_DEBOUNCE: Duration = Duration::from_millis(500);

/// 다른 깨움에 함께 처리할 수 있는 여유 시간. Lax도 next_due + slack으로 즉시 대기 계산에 참여한다.
#[cfg(feature = "gui")]
pub(crate) const LAYOUT_FLUSH_SLACK: Duration = Duration::from_millis(500);

/// backoff 최소 간격. 지난 재시도 시각 보정에 쓰며 실제 backoff 상태는 auto_attach가 관리한다.
#[cfg(feature = "gui")]
pub(crate) const RECONNECT_MIN_BACKOFF: Duration = Duration::from_millis(500);

/// 네이티브 메뉴가 열려 있을 때만 예약하는 폴링 간격.
#[cfg(feature = "gui")]
pub(crate) const PENDING_MENU_POLL_INTERVAL: Duration = Duration::from_millis(8);

/// Linux에서 필요한 webview 키 폴링 간격. 필요가 없어지면 예약을 취소한다.
#[cfg(feature = "gui")]
pub(crate) const WEBVIEW_KEY_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// TTL 정리 확인 주기. 주기와 slack은 예약 간격이며 OS·이벤트 루프 지연 상한은 아니다.
pub(crate) const SWEEP_TICK_INTERVAL: Duration = Duration::from_secs(30);

/// 정리는 다른 깨움에 함께 처리할 수 있도록 이만큼 늦은 깨우기 목표를 사용한다.
pub(crate) const SWEEP_TICK_SLACK: Duration = Duration::from_secs(60);

/// 실제 로그 정리는 별도 시간 조건으로 제한하므로 확인 타이머도 성기게 둔다.
pub(crate) const LOG_PRUNE_TICK_INTERVAL: Duration = Duration::from_secs(600);

pub(crate) const LOG_PRUNE_TICK_SLACK: Duration = Duration::from_secs(600);

pub(crate) fn register_steady_state(hub: &mut TimerHub<Tick>, now: Instant) {
    hub.every(Tick::Busy, BUSY_TICK_INTERVAL, Precision::Strict, now);
    #[cfg(feature = "gui")]
    hub.every(
        Tick::AttachView,
        ATTACH_POLL_INTERVAL,
        Precision::Strict,
        now,
    );
    // every는 drain 뒤 다음 시각을 미래로 옮긴다. 외부 상태로 매번 재등록하는 once_at과 다르다.
    hub.every(
        Tick::PtySweep,
        SWEEP_TICK_INTERVAL,
        Precision::Lax {
            slack: SWEEP_TICK_SLACK,
        },
        now,
    );
    hub.every(
        Tick::CaptureSweep,
        SWEEP_TICK_INTERVAL,
        Precision::Lax {
            slack: SWEEP_TICK_SLACK,
        },
        now,
    );
    hub.every(
        Tick::LogPrune,
        LOG_PRUNE_TICK_INTERVAL,
        Precision::Lax {
            slack: LOG_PRUNE_TICK_SLACK,
        },
        now,
    );
}

/// 두 허브의 가장 이른 깨우기 목표를 합친다. 다른 이벤트의 깨움은 판단하지 않는다.
pub(crate) fn min_deadline(a: Option<Instant>, b: Option<Instant>) -> Option<Instant> {
    match (a, b) {
        (Some(x), Some(y)) => Some(x.min(y)),
        (x, None) => x,
        (None, y) => y,
    }
}

/// 현재 또는 지난 데드라인만 now + period로 옮겨 즉시 재깨움을 막는다. 미래 시각은 유지한다.
#[cfg(feature = "gui")]
fn not_before_next_period(at: Instant, now: Instant, period: Duration) -> Instant {
    if at > now { at } else { now + period }
}

/// 외부 상태에서 만든 절대시각은 이 함수를 통해 등록한다.
/// 상태 갱신이 멈춰도 지난 시각을 반복 등록하지 않게 한다. timer_deadline_hygiene가 호출 경로를 검사한다.
#[cfg(feature = "gui")]
fn arm_derived(
    hub: &mut TimerHub<Tick>,
    key: Tick,
    at: Instant,
    now: Instant,
    period: Duration,
    precision: Precision,
) {
    hub.once_at(key, not_before_next_period(at, now, period), precision);
}

/// 저장 가능한 engine들의 가장 이른 첫 dirty 시각을 사용한다.
/// None은 dirty 자체가 없다는 뜻이 아니라 지금 저장할 대상이 없다는 뜻이다.
#[cfg(feature = "gui")]
pub(crate) fn sync_layout_flush_timer(
    hub: &mut TimerHub<Tick>,
    earliest_dirty_since: Option<Instant>,
    now: Instant,
) {
    match earliest_dirty_since {
        Some(since) => arm_derived(
            hub,
            Tick::LayoutFlush,
            since + LAYOUT_FLUSH_DEBOUNCE,
            now,
            LAYOUT_FLUSH_DEBOUNCE,
            Precision::Lax {
                slack: LAYOUT_FLUSH_SLACK,
            },
        ),
        None => hub.cancel(Tick::LayoutFlush),
    }
}

/// 이번 프레임에 보이지 않은 DAG surface의 예약을 취소해 불필요한 깨움을 막는다.
#[cfg(feature = "gui")]
pub(crate) fn sync_dag_graph_timers(
    hub: &mut TimerHub<Tick>,
    active: &[(u32, Instant)],
    now: Instant,
) {
    hub.cancel_if(|key| match key {
        Tick::DagGraph(sid) => !active.iter().any(|(s, _)| *s == sid),
        _ => false,
    });
    for (sid, at) in active {
        arm_derived(
            hub,
            Tick::DagGraph(*sid),
            *at,
            now,
            DAG_POLL_INTERVAL,
            Precision::Strict,
        );
    }
}

#[cfg(feature = "gui")]
pub(crate) fn sync_dag_list_popup_timer(
    hub: &mut TimerHub<Tick>,
    next_poll: Option<Instant>,
    now: Instant,
) {
    match next_poll {
        Some(at) => arm_derived(
            hub,
            Tick::DagListPopup,
            at,
            now,
            DAG_POLL_INTERVAL,
            Precision::Strict,
        ),
        None => hub.cancel(Tick::DagListPopup),
    }
}

/// 목록에서 빠진 anchor의 타이머만 취소한다. give-up 상태를 보존해야 하므로 재연결 슬롯은 지우지 않는다.
#[cfg(feature = "gui")]
pub(crate) fn sync_reconnect_timers(
    hub: &mut TimerHub<Tick>,
    wakeups: &[(u32, Instant)],
    now: Instant,
) {
    hub.cancel_if(|key| match key {
        Tick::Reconnect(anchor) => !wakeups.iter().any(|(a, _)| *a == anchor),
        _ => false,
    });
    for (anchor, at) in wakeups {
        arm_derived(
            hub,
            Tick::Reconnect(*anchor),
            *at,
            now,
            RECONNECT_MIN_BACKOFF,
            Precision::Strict,
        );
    }
}

/// 메뉴가 열려 있으면 추가 입력이 없어도 조회할 수 있도록 다음 폴링을 예약한다.
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

/// Linux에서 호출자가 폴링이 필요하다고 판단한 동안만 예약한다. 그렇지 않으면 취소한다.
#[cfg(feature = "gui")]
pub(crate) fn reschedule_webview_key_poll(
    hub: &mut TimerHub<Tick>,
    needs_poll: bool,
    now: Instant,
) {
    let arm = needs_poll && cfg!(target_os = "linux");
    if arm {
        hub.once_after(
            Tick::WebviewKeyPoll,
            WEBVIEW_KEY_POLL_INTERVAL,
            Precision::Strict,
            now,
        );
    } else {
        hub.cancel(Tick::WebviewKeyPoll);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[cfg(feature = "gui")]
    #[test]
    fn pending_menu_reschedules_a_poll_frame() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        reschedule_pending_menu_poll(&mut hub, true, t0);
        assert_eq!(hub.next_deadline(), Some(t0 + PENDING_MENU_POLL_INTERVAL));
        assert!(
            PENDING_MENU_POLL_INTERVAL <= Duration::from_millis(16),
            "메뉴 폴링 예약 간격은 60fps 프레임 간격을 넘지 않아야 한다"
        );
    }

    #[test]
    fn sweep_ticks_are_registered_at_boot() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        register_steady_state(&mut hub, t0);

        for (key, interval, slack) in [
            (Tick::PtySweep, SWEEP_TICK_INTERVAL, SWEEP_TICK_SLACK),
            (Tick::CaptureSweep, SWEEP_TICK_INTERVAL, SWEEP_TICK_SLACK),
            (
                Tick::LogPrune,
                LOG_PRUNE_TICK_INTERVAL,
                LOG_PRUNE_TICK_SLACK,
            ),
        ] {
            let e = hub
                .snapshot()
                .into_iter()
                .find(|e| e.key == key)
                .unwrap_or_else(|| panic!("{key:?} 미등록"));
            assert_eq!(e.interval, Some(interval), "{key:?} 주기");
            assert_eq!(
                e.precision,
                Precision::Lax { slack },
                "{key:?}: 정리 타이머는 Lax로 등록해야 한다"
            );
            assert_eq!(e.next_due, t0 + interval, "{key:?} 첫 next_due");
        }
    }

    /// 초기 등록에서는 정리 타이머의 깨우기 목표가 Busy보다 늦다.
    #[test]
    fn sweep_ticks_do_not_advance_the_wakeup_deadline() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        register_steady_state(&mut hub, t0);
        assert_eq!(
            hub.next_deadline(),
            Some(t0 + BUSY_TICK_INTERVAL),
            "가장 이른 hard deadline 은 여전히 1Hz busy tick"
        );

        let mut only_sweeps = TimerHub::new();
        only_sweeps.every(
            Tick::PtySweep,
            SWEEP_TICK_INTERVAL,
            Precision::Lax {
                slack: SWEEP_TICK_SLACK,
            },
            t0,
        );
        assert_eq!(
            only_sweeps.next_deadline(),
            Some(t0 + SWEEP_TICK_INTERVAL + SWEEP_TICK_SLACK)
        );
    }

    #[test]
    fn repeating_sweep_ticks_advance_on_their_own() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        register_steady_state(&mut hub, t0);

        let late = t0 + SWEEP_TICK_INTERVAL * 10;
        assert!(hub.drain_due(late).contains(&Tick::PtySweep));
        let next = hub
            .snapshot()
            .into_iter()
            .find(|e| e.key == Tick::PtySweep)
            .expect("still registered")
            .next_due;
        assert!(
            next > late,
            "{next:?} <= {late:?} 이면 즉시 wake 가 반복된다"
        );
    }

    /// 뒤이은 변경이 첫 dirty 시각과 예약 시각을 미루지 않는지 확인한다. 실제 저장 시점 검사는 아니다.
    #[cfg(feature = "gui")]
    #[test]
    fn layout_flush_deadline_is_anchored_to_the_first_change() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        sync_layout_flush_timer(&mut hub, Some(t0), t0);
        assert!(hub.is_registered(Tick::LayoutFlush));

        sync_layout_flush_timer(&mut hub, Some(t0), t0);
        assert_eq!(hub.snapshot().len(), 1);
        assert!(hub.drain_due(t0 + Duration::from_millis(400)).is_empty());
        assert_eq!(
            hub.drain_due(t0 + LAYOUT_FLUSH_DEBOUNCE),
            vec![Tick::LayoutFlush]
        );
        assert!(!hub.is_registered(Tick::LayoutFlush));
    }

    /// Lax의 깨우기 목표는 처음부터 next_due + slack이다.
    #[cfg(feature = "gui")]
    #[test]
    fn layout_flush_does_not_create_its_own_wakeup_before_slack() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        sync_layout_flush_timer(&mut hub, Some(t0), t0);
        assert_eq!(
            hub.next_deadline(),
            Some(t0 + LAYOUT_FLUSH_DEBOUNCE + LAYOUT_FLUSH_SLACK)
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn a_stale_layout_dirty_since_never_schedules_a_wakeup_in_the_past() {
        let now = Instant::now();
        let stale = now - Duration::from_secs(30);
        let mut hub = TimerHub::new();

        sync_layout_flush_timer(&mut hub, Some(stale), now);
        let entry = hub.snapshot().into_iter().next().expect("registered");
        assert!(
            entry.next_due > now,
            "지난 데드라인이 미래로 보정되지 않았다: {:?} <= {now:?}",
            entry.next_due
        );
        assert_eq!(
            entry.next_due,
            now + LAYOUT_FLUSH_DEBOUNCE,
            "지난 데드라인은 now + period로 예약해야 한다"
        );

        let later = now + Duration::from_millis(1);
        sync_layout_flush_timer(&mut hub, Some(stale), later);
        let entry = hub.snapshot().into_iter().next().expect("registered");
        assert_eq!(entry.next_due, later + LAYOUT_FLUSH_DEBOUNCE);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn a_stale_reconnect_attempt_never_schedules_a_wakeup_in_the_past() {
        let now = Instant::now();
        let mut hub = TimerHub::new();
        sync_reconnect_timers(&mut hub, &[(3, now - Duration::from_secs(5))], now);
        assert_eq!(hub.next_deadline(), Some(now + RECONNECT_MIN_BACKOFF));
    }

    /// 미래 데드라인은 주기보다 가까워도 그대로 유지한다.
    #[cfg(feature = "gui")]
    #[test]
    fn fresh_derived_deadlines_are_scheduled_as_is() {
        let now = Instant::now();
        let mut hub = TimerHub::new();
        let soon = now + Duration::from_millis(120);

        sync_layout_flush_timer(&mut hub, Some(now), now);
        sync_dag_graph_timers(&mut hub, &[(7, soon)], now);
        sync_dag_list_popup_timer(&mut hub, Some(soon), now);
        sync_reconnect_timers(&mut hub, &[(3, soon)], now);

        let due: std::collections::HashMap<_, _> = hub
            .snapshot()
            .into_iter()
            .map(|e| (format!("{:?}", e.key), e.next_due))
            .collect();
        assert_eq!(due["LayoutFlush"], now + LAYOUT_FLUSH_DEBOUNCE);
        assert_eq!(due["DagGraph(7)"], soon);
        assert_eq!(due["DagListPopup"], soon);
        assert_eq!(due["Reconnect(3)"], soon);
    }

    #[cfg(feature = "gui")]
    #[test]
    fn layout_flush_timer_is_cancelled_when_nothing_is_dirty() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        sync_layout_flush_timer(&mut hub, Some(t0), t0);
        sync_layout_flush_timer(&mut hub, None, t0);
        assert!(!hub.is_registered(Tick::LayoutFlush));
        assert!(hub.next_deadline().is_none());
    }

    #[cfg(feature = "gui")]
    #[test]
    fn dag_graph_timers_track_the_visible_set() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        sync_dag_graph_timers(&mut hub, &[(7, t0 + Duration::from_millis(500))], t0);
        assert!(hub.is_registered(Tick::DagGraph(7)));
        assert_eq!(hub.next_deadline(), Some(t0 + Duration::from_millis(500)));
        assert_eq!(
            hub.drain_due(t0 + Duration::from_millis(500)),
            vec![Tick::DagGraph(7)]
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn closing_a_dag_graph_view_cancels_its_timer() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        sync_dag_graph_timers(&mut hub, &[(7, t0 + Duration::from_millis(500))], t0);
        assert!(hub.is_registered(Tick::DagGraph(7)));

        sync_dag_graph_timers(&mut hub, &[], t0);
        assert!(!hub.is_registered(Tick::DagGraph(7)));
        assert!(hub.next_deadline().is_none());
    }

    #[cfg(feature = "gui")]
    #[test]
    fn dag_graph_timers_are_independent_per_surface() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        let at = t0 + Duration::from_millis(500);
        sync_dag_graph_timers(&mut hub, &[(7, at), (9, at)], t0);
        assert!(hub.is_registered(Tick::DagGraph(7)));
        assert!(hub.is_registered(Tick::DagGraph(9)));

        sync_dag_graph_timers(&mut hub, &[(9, at)], t0);
        assert!(!hub.is_registered(Tick::DagGraph(7)));
        assert!(hub.is_registered(Tick::DagGraph(9)));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn dag_graph_sync_leaves_other_ticks_alone() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        register_steady_state(&mut hub, t0);
        sync_dag_graph_timers(&mut hub, &[(7, t0 + Duration::from_millis(500))], t0);
        sync_dag_graph_timers(&mut hub, &[], t0);
        assert!(hub.is_registered(Tick::Busy));
        assert!(hub.is_registered(Tick::AttachView));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn a_stale_dag_deadline_never_schedules_a_wakeup_in_the_past() {
        let now = Instant::now();
        let stale = now - Duration::from_secs(3);
        let mut hub = TimerHub::new();

        sync_dag_graph_timers(&mut hub, &[(7, stale)], now);
        let deadline = hub.next_deadline().expect("registered");
        assert!(
            deadline > now,
            "지난 데드라인이 미래로 보정되지 않았다: {deadline:?} <= {now:?}"
        );
        assert_eq!(
            deadline,
            now + DAG_POLL_INTERVAL,
            "지난 데드라인은 now + period로 예약해야 한다"
        );

        let later = now + Duration::from_millis(1);
        sync_dag_graph_timers(&mut hub, &[(7, stale)], later);
        assert_eq!(hub.next_deadline(), Some(later + DAG_POLL_INTERVAL));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn a_fresh_dag_deadline_is_scheduled_as_is() {
        let now = Instant::now();
        let at = now + Duration::from_millis(120);
        let mut hub = TimerHub::new();
        sync_dag_graph_timers(&mut hub, &[(7, at)], now);
        assert_eq!(hub.next_deadline(), Some(at));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn a_stale_dag_list_popup_deadline_is_floored_too() {
        let now = Instant::now();
        let mut hub = TimerHub::new();
        sync_dag_list_popup_timer(&mut hub, Some(now - Duration::from_secs(1)), now);
        assert_eq!(hub.next_deadline(), Some(now + DAG_POLL_INTERVAL));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn dag_list_popup_timer_follows_the_popup() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        sync_dag_list_popup_timer(&mut hub, Some(t0 + Duration::from_millis(500)), t0);
        assert!(hub.is_registered(Tick::DagListPopup));
        sync_dag_list_popup_timer(&mut hub, None, t0);
        assert!(!hub.is_registered(Tick::DagListPopup));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn reconnect_timers_track_the_scheduled_anchors() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        let at = t0 + Duration::from_secs(5);
        sync_reconnect_timers(&mut hub, &[(3, at), (4, at + Duration::from_secs(1))], t0);
        assert!(hub.is_registered(Tick::Reconnect(3)));
        assert!(hub.is_registered(Tick::Reconnect(4)));
        assert_eq!(hub.next_deadline(), Some(at));

        sync_reconnect_timers(&mut hub, &[(4, at + Duration::from_secs(1))], t0);
        assert!(!hub.is_registered(Tick::Reconnect(3)));
        assert!(hub.is_registered(Tick::Reconnect(4)));
    }

    #[cfg(feature = "gui")]
    #[test]
    fn reconnect_sync_leaves_other_ticks_alone() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        register_steady_state(&mut hub, t0);
        sync_dag_graph_timers(&mut hub, &[(7, t0 + Duration::from_millis(500))], t0);
        sync_reconnect_timers(&mut hub, &[(3, t0 + Duration::from_secs(5))], t0);
        sync_reconnect_timers(&mut hub, &[], t0);
        assert!(hub.is_registered(Tick::Busy));
        assert!(hub.is_registered(Tick::AttachView));
        assert!(hub.is_registered(Tick::DagGraph(7)));
    }

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
