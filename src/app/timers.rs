//! 메인 루프 시간축의 키 정의와 등록 — `tasty_timer::TimerHub` 의 호스트측 어휘.
//!
//! 주기 작업마다 ticker 스레드를 만드는 대신 여기 키를 등록하고, gui/headless 양쪽
//! 실행부가 `drain_due` 로 받은 키를 `match` 로 실행한다. 등록 규칙·Strict/Lax 판단
//! 기준·가드 중 정지 계약은 `docs/dev-guide/timer-hub.md`.

use std::time::Duration;
use std::time::Instant;

use tasty_timer::Precision;
use tasty_timer::TimerHub;

/// DAG 뷰/popup 이 memory store 를 다시 읽는 주기. 데이터 소유자는 뷰 쪽이라
/// 상수도 거기 있고, 여기서는 stale 데드라인의 바닥값으로만 쓴다.
#[cfg(feature = "gui")]
use crate::adapters::ui::surface::dag_graph::view::POLL_INTERVAL as DAG_POLL_INTERVAL;

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
    /// 일회성. 살아있는 native webview 가 있는 동안 다음 폴링 프레임을 예약한다.
    ///
    /// webview 는 winit 창과 별개의 OS 자식 창이라 그 자식으로 들어간 키/클릭이
    /// winit 이벤트 루프를 깨우지 못한다(Linux 는 GDK 가 자기 X 연결로 받는다).
    /// 이 tick 이 없으면 webview 에 포커스가 있는 동안 host 가 영영 깨지 않아
    /// 포워딩된 키가 큐에 쌓인 채 멈춘다. 깨우는 것 자체가 역할이고 실행부는 없다
    /// (`about_to_wait` 의 `pump_webview_key_events` 가 파이프라인에서 처리한다).
    ///
    /// **실제로 걸리는 것은 Linux 뿐이다** — macOS/Windows 는 native 키 콜백이 winit 과
    /// 같은 OS 이벤트 루프에서 발화해 폴링이 필요 없고, 거기까지 상시 60Hz wakeup 을
    /// 둘 이유가 없다(`reschedule_webview_key_poll` 이 그 판정을 갖는다).
    #[cfg(feature = "gui")]
    WebviewKeyPoll,
    /// 일회성 디바운스. 레이아웃이 처음 dirty 가 된 시각 + `LAYOUT_FLUSH_DEBOUNCE`
    /// 에 슬롯 파일로 flush 한다. **`every` 가 아니다** — 주기로 두면 변경이 없어도
    /// 500ms 마다 깨어나는 회귀가 된다.
    #[cfg(feature = "gui")]
    LayoutFlush,
    /// DAG graph surface 하나의 다음 폴링 wakeup. runner 는 별도 스레드라 진행이
    /// 렌더 루프를 깨우지 않으므로 보이는 동안만 이 키가 걸린다.
    ///
    /// **뷰가 닫히거나 배경으로 밀리면 반드시 취소돼야 한다** — 등록만 하고 잊으면
    /// 사라진 뷰 때문에 영원히 깨어나는 누수가 된다. 그래서 매 프레임
    /// `sync_dag_graph_timers` 가 "이번에 보인 surface" 집합으로 전체를 맞춘다.
    #[cfg(feature = "gui")]
    DagGraph(u32),
    /// DAG 목록 popup 의 다음 폴링 wakeup. popup 은 surface 에 매이지 않아
    /// `DagGraph(u32)` 로 표현할 수 없다 — 같은 뷰를 쓰지만 키가 따로다.
    #[cfg(feature = "gui")]
    DagListPopup,
    /// 원격 attach 가 끊긴 anchor 워크스페이스의 backoff 재연결 시각.
    ///
    /// 담당은 **`due`(시각) 쪽뿐**이다 — 사용자가 그 워크스페이스로 돌아오는 edge
    /// 트리거는 시각과 무관하므로 기존대로 프레임 검사로 남는다. 실행부도 no-op 이다:
    /// 깨어나기만 하면 파이프라인의 `poll_auto_attach` 가 판정·발화를 한다.
    #[cfg(feature = "gui")]
    Reconnect(u32),
    /// 30초. idle TTL 을 넘긴 headless PTY 를 회수한다
    /// (`docs/adr/0050-headless-pty-primitive.md` "좀비 회수 시점").
    ///
    /// 접근 시점 lazy sweep 을 **대체하지 않고 보완한다** — lazy 는 `pty.spawn` 직전에
    /// 돌아 동시 개수 상한 판정을 정확하게 유지하는 별개 역할이 있다. 이 tick 은
    /// "에이전트가 조용해져 아무도 `pty.*` 를 부르지 않는" 사각만 메운다.
    PtySweep,
    /// 30초. TTL 을 넘긴 캡처 업로드 partial 을 회수한다. `append` 내부 lazy 경로와
    /// 같은 이유의 보완 — 다음 청크가 와야 이전 stale 이 정리되는 구조라, 업로드가
    /// 멈춘 순간(= 정리가 가장 필요한 순간) 정리도 멈춘다.
    CaptureSweep,
    /// 10분. IPC 관측 로그 3종의 보존 정책을 집행한다(`ADR-0085`). 실제 집행 주기는
    /// `log_retention::PRUNE_INTERVAL_MS`(1시간) 게이트가 정하므로 이 tick 이 자주
    /// 와도 무해하다 — tick 은 "append 가 전혀 없는 인스턴스에서도 게이트를 볼
    /// 기회를 만든다" 는 역할만 한다.
    LogPrune,
}

/// busy 재평가 주기. 사용자가 체감하는 indicator 반응 상한이라 1초를 넘기지 않는다.
pub(crate) const BUSY_TICK_INTERVAL: Duration = Duration::from_secs(1);

/// attach 뷰 갱신 주기. 사용자 확정 UX = 3초/회 — 원격 readonly·mirror 뷰는 실시간
/// stream 이 아니라 이 cadence 로만 렌더를 갱신한다.
#[cfg(feature = "gui")]
pub(crate) const ATTACH_POLL_INTERVAL: Duration = Duration::from_secs(3);

/// 레이아웃이 처음 dirty 가 된 뒤 슬롯 파일에 flush 하기까지의 디바운스.
/// 분할·이동이 연달아 일어나는 동안 디스크 쓰기를 한 번으로 접는다.
#[cfg(feature = "gui")]
pub(crate) const LAYOUT_FLUSH_DEBOUNCE: Duration = Duration::from_millis(500);

/// flush 데드라인의 Lax slack. flush 는 사용자가 즉시 체감하는 작업이 아니라
/// 자기 힘으로 호스트를 깨울 이유가 없다 — 다른 wakeup(1Hz busy tick 등)에 편승하고,
/// slack 을 넘기면 그때는 반드시 저장한다(변경이 영영 디스크에 못 닿는 것을 막는다).
#[cfg(feature = "gui")]
pub(crate) const LAYOUT_FLUSH_SLACK: Duration = Duration::from_millis(500);

/// 재연결 backoff 의 최소 간격(`tasty_ssh::Backoff::new` 의 min 과 같은 값).
/// 여기서는 stale `next_attempt` 의 바닥값으로만 쓴다 — backoff 자체는
/// `app/auto_attach.rs` 가 굴린다.
#[cfg(feature = "gui")]
pub(crate) const RECONNECT_MIN_BACKOFF: Duration = Duration::from_millis(500);

/// 네이티브 컨텍스트 메뉴가 떠 있는 동안의 폴링 주기 상한. 메뉴 트래킹(하이라이트
/// 이동 등)이 사람 눈에 끊겨 보이지 않을 만큼 짧고, idle 상태에서 이벤트 루프를
/// 계속 깨우지는 않을 만큼만 짧다(메뉴가 닫히면 등록 자체가 사라진다).
#[cfg(feature = "gui")]
pub(crate) const PENDING_MENU_POLL_INTERVAL: Duration = Duration::from_millis(8);

/// 살아있는 webview 가 있는 동안의 키 포워딩 폴링 주기. 단축키 반응 지연이 사람에게
/// 안 느껴질 만큼 짧고(한 프레임 수준), webview 가 없으면 등록 자체가 사라진다.
#[cfg(feature = "gui")]
pub(crate) const WEBVIEW_KEY_POLL_INTERVAL: Duration = Duration::from_millis(16);

/// TTL 정리 tick 주기. TTL(5분) 대비 충분히 촘촘해 회수 지연 상한이
/// `TTL + interval + slack` = 최대 6.5분이다. TTL 자체는 바꾸지 않는다.
pub(crate) const SWEEP_TICK_INTERVAL: Duration = Duration::from_secs(30);

/// 정리 tick 의 Lax slack. 정리는 사용자가 즉시 체감하는 작업이 아니라 자기 힘으로
/// 호스트를 깨울 이유가 없다 — 다른 wakeup 에 편승하고, slack 을 넘겨서야 반드시
/// 깨운다(완전 idle 인 인스턴스에서도 회수가 영영 멈추지는 않게).
pub(crate) const SWEEP_TICK_SLACK: Duration = Duration::from_secs(60);

/// 로그 prune tick 주기. 집행 자체가 1시간 게이트라 tick 은 성기게 둔다 —
/// 자주 깨울 이유가 없다.
pub(crate) const LOG_PRUNE_TICK_INTERVAL: Duration = Duration::from_secs(600);

/// 로그 prune tick 의 Lax slack.
pub(crate) const LOG_PRUNE_TICK_SLACK: Duration = Duration::from_secs(600);

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
    // TTL 정리 3종은 전부 `Lax` — 회수가 몇십 초 늦는 것은 무해하지만 그것 때문에
    // idle 인스턴스를 깨우는 것은 낭비다. 사용자가 뭐라도 하는 프레임에 공짜로
    // 실행되고, 완전 idle 이면 slack 을 넘길 때 한 번만 깨운다.
    //
    // 이 셋은 `once_at`(외부 상태에서 파생한 절대 시각)이 아니라 `every`(고정 주기)
    // 라서 `arm_derived` 바닥치기가 필요 없다 — 등록이 부팅 1회뿐이라 매 프레임
    // 재등록되지 않고, 재발화 시각은 `TimerHub` 가 직전 데드라인에 주기를 더해
    // 스스로 전진시킨다(과거에 고정될 수 없다). 0 주기는 `TimerHub` 의
    // `normalize()` 가 이미 막는다.
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

/// 이미 지난 파생 데드라인을 한 주기 뒤로 끌어올린다.
///
/// 파생 데드라인은 전부 "외부 상태의 어떤 시각 + 주기" 꼴이다(마지막으로 읽은
/// 시각, 처음 dirty 가 된 시각, 다음 재시도 시각 …). 그 외부 상태가 어떤 이유로든
/// 갱신을 멈추면 값이 **영원히 과거**에 머문다. 그대로 등록하면 `next_deadline()`
/// 이 과거가 되어 `WaitUntil(과거)` = 즉시 wake 가 무한 반복되고, 코어 하나가
/// 100% 로 스핀한다.
///
/// 등록을 잊는 누수는 "쓸데없이 한 번 더 깨어남" 이지만 stale 데드라인은 "아예
/// 쉬지 못함" 이라 실패 비용의 차원이 다르다. 그래서 바닥치기는 개별 키의 선택이
/// 아니라 **이 모듈의 규칙**이다 — 파생 데드라인은 [`arm_derived`] 를 통해서만
/// 등록한다.
#[cfg(feature = "gui")]
fn not_before_next_period(at: Instant, now: Instant, period: Duration) -> Instant {
    if at > now { at } else { now + period }
}

/// 외부 상태에서 파생한 절대 데드라인을 등록하는 **유일한 통로**.
///
/// `hub.once_at` 을 이 모듈에서 직접 부르지 않는다 — 새 `Tick` 이 추가될 때
/// 바닥치기를 빠뜨리는 것이 이 버그 클래스의 재발 경로이기 때문이다.
/// `tests/timer_deadline_hygiene.rs` 가 이 규칙을 소스 수준에서 강제한다.
///
/// `period` 는 그 키의 고유 cadence 다 — 상류가 멈춰도 최악이 "주기당 1회" 로
/// 묶이도록 그 키가 정상 동작할 때의 간격을 넘겨준다.
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

/// 레이아웃 flush 데드라인 동기화 — 가장 이른 `dirty_since + debounce` 하나로 예약한다.
///
/// engine(창)마다 자기 dirty 상태를 갖지만 flush 는 전 engine 을 한 번에 도는
/// 함수라 타이머는 하나면 된다. `dirty_since` 는 **처음 dirty 가 된 시각**이라
/// (뒤 변경이 리셋하지 않는다) 매 프레임 같은 절대 시각으로 재등록해도 위상이 밀리지
/// 않는다 — 절대 시각으로 등록하는 이유다.
///
/// `None` 은 "**이 프레임에 저장할 것이 없다**" 이지 "dirty 가 없다" 가 아니다 —
/// 저장이 꺼져 있거나(`restore_layout=false`) 슬롯이 없는 engine 은 dirty 여도
/// 영원히 flush 되지 않으므로 애초에 예약 대상이 아니다
/// (`App::earliest_layout_dirty_since`).
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
        // 저장할 변경이 없다 — 등록을 남기면 idle 에서도 깨운다.
        None => hub.cancel(Tick::LayoutFlush),
    }
}

/// DAG graph surface 폴링 타이머를 **이번 프레임에 보인 집합**에 맞춘다.
///
/// `active` 에 없는 `DagGraph` 키는 전부 취소한다 — 닫힌 뷰(`drop_view`)든 배경
/// 탭으로 밀린 뷰든 예약이 남지 않는 것이 이 함수의 존재 이유다. egui
/// `request_repaint_after` 는 뷰가 그려질 때만 갱신돼 자동 소멸했지만 허브 등록은
/// 그렇지 않다.
///
/// 데드라인은 [`not_before_next_period`] 를 통과시킨다 — `active` 가 어떤 경로로든
/// 낡은 채로 들어와도 스핀이 아니라 주기당 1회 wakeup 으로 끝난다.
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

/// DAG 목록 popup 폴링 타이머. `None` = popup 이 닫혀 있다(예약 없음).
/// graph 뷰와 같은 이유로 데드라인을 [`not_before_next_period`] 로 바닥친다.
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

/// backoff 재연결 wakeup 을 `wakeups` 집합에 맞춘다. 목록에 없는 anchor 의
/// `Reconnect` 키는 취소한다.
///
/// **취소는 "재연결 스케줄 삭제" 가 아니다.** give-up 한 anchor 는 여기서 타이머만
/// 사라지고 `ReconnectSlot` 은 그대로 남는다 — 슬롯을 지우면 즉시 재시도가 재개되는
/// 회귀가 있었다(`app/auto_attach.rs` 의 `reconnect_wakeup_at` 참조). 이 함수는
/// 허브만 만지고 슬롯 맵에는 접근하지 않는다.
///
/// `next_attempt` 도 파생 데드라인이라 [`arm_derived`] 를 통과시킨다 — 재시도
/// 트리거가 슬롯을 갱신하지 않고 빠지는 경로(매핑이 사라진 anchor 등)에서
/// `next_attempt` 가 과거에 고정될 수 있다.
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

/// "드러난 webview + 활성 창" 여부 → 다음 키 폴링 wakeup 예약/취소(Linux 전용).
///
/// `reschedule_pending_menu_poll` 과 같은 이유로 순수 함수다 — 이 재예약을 빠뜨리면
/// webview 에 키보드 포커스가 있는 동안 host 가 깨지 않아 단축키가 통째로 멈춘다.
/// 반대로 조건이 풀리면 반드시 취소해야 배경 인스턴스가 16ms 마다 깨지 않는다.
#[cfg(feature = "gui")]
pub(crate) fn reschedule_webview_key_poll(
    hub: &mut TimerHub<Tick>,
    needs_poll: bool,
    now: Instant,
) {
    // tick 을 세우는 것은 **Linux 뿐**이다. macOS/Windows 는 native 키 콜백이 winit 과
    // 같은 OS 이벤트 루프에서 발화해 폴링이 필요 없어, 조건과 무관하게 세우지 않는다.
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

    /// TTL 정리 3종은 부팅 시 `Lax` 로 등록된다 — 등록 자체가 빠지면 정리가 접근
    /// 시점 lazy 로만 도는 옛 상태로 되돌아간다.
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
                "{key:?} 는 Lax 여야 한다 — Strict 면 정리 때문에 idle 인스턴스를 깨운다"
            );
            assert_eq!(e.next_due, t0 + interval, "{key:?} 첫 발화");
        }
    }

    /// 정리 tick 은 **idle wakeup 을 늘리지 않는다**. `Lax` 라 hard deadline 이
    /// `next_due + slack` 까지 밀려 있어, 1Hz busy tick 이 이미 잡아둔 데드라인을
    /// 앞당기지 않는다.
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

        // 정리 tick 만 등록된 허브에서도 slack 경계 전에는 깨우지 않는다.
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

    /// 주기 tick 은 `every` 라 **`arm_derived` 바닥치기 대상이 아니다** — 발화할
    /// 때마다 허브가 직전 데드라인에 주기를 더해 스스로 전진시키므로 데드라인이
    /// 과거에 고정될 수 없다. 파생 절대시각을 매 프레임 재등록하는 `once_at` 계열과
    /// 다른 점이고, 그래서 스핀 클래스에 해당하지 않는다.
    #[test]
    fn repeating_sweep_ticks_advance_on_their_own() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        register_steady_state(&mut hub, t0);

        // 한참(=여러 주기) 뒤에 처음 drain 해도 다음 데드라인은 미래다.
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

    /// 디바운스는 **첫 변경 시각 기준**이다 — 뒤이은 변경이 데드라인을 밀지 않으므로
    /// 연속 변경 중에도 첫 변경으로부터 debounce 안에 반드시 한 번 저장된다.
    #[cfg(feature = "gui")]
    #[test]
    fn layout_flush_deadline_is_anchored_to_the_first_change() {
        let t0 = Instant::now();
        let mut hub = TimerHub::new();
        sync_layout_flush_timer(&mut hub, Some(t0), t0);
        assert!(hub.is_registered(Tick::LayoutFlush));

        // t=400ms 에 또 변경이 일어나도 dirty_since 는 t0 그대로 → 데드라인 불변.
        sync_layout_flush_timer(&mut hub, Some(t0), t0);
        assert_eq!(hub.snapshot().len(), 1);
        assert!(hub.drain_due(t0 + Duration::from_millis(400)).is_empty());
        assert_eq!(
            hub.drain_due(t0 + LAYOUT_FLUSH_DEBOUNCE),
            vec![Tick::LayoutFlush]
        );
        // 일회성이라 발화 후 자동 해제 — 다음 dirty 전이에서 다시 등록된다.
        assert!(!hub.is_registered(Tick::LayoutFlush));
    }

    /// flush 는 자기 힘으로 호스트를 깨우지 않는다(Lax) — 다른 wakeup 에 편승하고
    /// slack 을 넘겨서야 hard deadline 이 된다.
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

    /// **회귀 방지(gate4-22 2차)** — `restore_layout=false` 처럼 flush 가 영원히
    /// 일어나지 않는 상태에서는 `dirty_since` 가 계속 남는다. 그 시각에서 파생한
    /// 데드라인은 매 프레임 **과거**가 되고, 그대로 등록하면 `WaitUntil(과거)` 가
    /// 즉시 wake 를 무한 반복해 코어 하나가 100% 로 스핀한다(실측 624 ticks/5s vs
    /// baseline 2). 데드라인은 언제나 `now` 보다 뒤여야 한다.
    #[cfg(feature = "gui")]
    #[test]
    fn a_stale_layout_dirty_since_never_schedules_a_wakeup_in_the_past() {
        let now = Instant::now();
        let stale = now - Duration::from_secs(30); // 30초 전에 dirty 가 됐고 그대로
        let mut hub = TimerHub::new();

        sync_layout_flush_timer(&mut hub, Some(stale), now);
        let entry = hub.snapshot().into_iter().next().expect("registered");
        assert!(
            entry.next_due > now,
            "과거 데드라인을 그대로 등록하면 루프가 스핀한다: {:?} <= {now:?}",
            entry.next_due
        );
        assert_eq!(
            entry.next_due,
            now + LAYOUT_FLUSH_DEBOUNCE,
            "최악이라도 주기당 1회"
        );

        // 다음 프레임에도 같은(여전히 낡은) 값이 들어오지만 결과는 동일하다.
        let later = now + Duration::from_millis(1);
        sync_layout_flush_timer(&mut hub, Some(stale), later);
        let entry = hub.snapshot().into_iter().next().expect("registered");
        assert_eq!(entry.next_due, later + LAYOUT_FLUSH_DEBOUNCE);
    }

    /// 낡은 재연결 `next_attempt` 도 마찬가지 — 재시도 트리거가 슬롯을 갱신하지
    /// 않고 빠지는 경로(매핑이 사라진 anchor 등)에서 과거에 고정될 수 있다.
    #[cfg(feature = "gui")]
    #[test]
    fn a_stale_reconnect_attempt_never_schedules_a_wakeup_in_the_past() {
        let now = Instant::now();
        let mut hub = TimerHub::new();
        sync_reconnect_timers(&mut hub, &[(3, now - Duration::from_secs(5))], now);
        assert_eq!(hub.next_deadline(), Some(now + RECONNECT_MIN_BACKOFF));
    }

    /// 정상(미래) 데드라인은 바닥치기가 건드리지 않는다 — 위 방어가 위상을
    /// 밀어버리면 그것대로 회귀다. 파생 데드라인을 쓰는 네 키를 한 번에 본다.
    #[cfg(feature = "gui")]
    #[test]
    fn fresh_derived_deadlines_are_scheduled_as_is() {
        let now = Instant::now();
        let mut hub = TimerHub::new();
        let soon = now + Duration::from_millis(120);

        sync_layout_flush_timer(&mut hub, Some(now), now); // dirty_since=now → +500ms
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

    /// 저장할 변경이 사라지면 등록을 걷어낸다 — 남겨두면 idle 에서도 깨운다.
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

    /// 보이는 DAG 뷰마다 폴링 데드라인이 걸린다.
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

    /// 뷰를 닫으면(= 보인 집합에서 빠지면) 타이머가 사라진다. 이게 없으면 닫힌
    /// 뷰 때문에 500ms 마다 영원히 깨어나는 누수가 된다.
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

    /// 여러 뷰가 동시에 열려 있어도 각자의 키를 갖고, 사라진 것만 걷힌다.
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

    /// DAG 뷰 정리는 다른 키를 건드리지 않는다(`cancel_if` 술어 범위 확인).
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

    /// **회귀 방지(gate4-22)** — 낡은(이미 지난) 데드라인이 들어와도 이벤트 루프가
    /// 쉴 수 있어야 한다.
    ///
    /// 상류(`pending_poll_deadlines`)는 "마지막으로 읽은 시각 + 500ms" 를 준다.
    /// 실제 읽기가 멈춘 채 이 값만 계속 재계산되면 값이 영원히 과거에 머무는데,
    /// 그대로 등록하면 `WaitUntil(과거)` 가 즉시 wake 를 무한 반복해 코어 하나가
    /// 100% 로 스핀한다(실측 410~449 ticks/5s vs baseline 2). 데드라인은 항상
    /// `now` 보다 뒤여야 한다.
    #[cfg(feature = "gui")]
    #[test]
    fn a_stale_dag_deadline_never_schedules_a_wakeup_in_the_past() {
        let now = Instant::now();
        let stale = now - Duration::from_secs(3); // 3초 전에 읽고 그대로 멈춘 뷰
        let mut hub = TimerHub::new();

        sync_dag_graph_timers(&mut hub, &[(7, stale)], now);
        let deadline = hub.next_deadline().expect("registered");
        assert!(
            deadline > now,
            "과거 데드라인을 그대로 등록하면 루프가 스핀한다: {deadline:?} <= {now:?}"
        );
        assert_eq!(deadline, now + DAG_POLL_INTERVAL, "최악이라도 주기당 1회");

        // 같은 상황이 다음 프레임에 반복돼도(= 여전히 낡은 값) 결과는 동일하다.
        let later = now + Duration::from_millis(1);
        sync_dag_graph_timers(&mut hub, &[(7, stale)], later);
        assert_eq!(hub.next_deadline(), Some(later + DAG_POLL_INTERVAL));
    }

    /// 정상(미래) 데드라인은 바닥치기가 건드리지 않는다 — 위 방어가 폴링 위상을
    /// 밀어버리면 그것대로 회귀다.
    #[cfg(feature = "gui")]
    #[test]
    fn a_fresh_dag_deadline_is_scheduled_as_is() {
        let now = Instant::now();
        let at = now + Duration::from_millis(120); // 380ms 전에 읽은 뷰
        let mut hub = TimerHub::new();
        sync_dag_graph_timers(&mut hub, &[(7, at)], now);
        assert_eq!(hub.next_deadline(), Some(at));
    }

    /// popup 데드라인도 같은 바닥을 갖는다(같은 파생식 → 같은 실패 양상).
    #[cfg(feature = "gui")]
    #[test]
    fn a_stale_dag_list_popup_deadline_is_floored_too() {
        let now = Instant::now();
        let mut hub = TimerHub::new();
        sync_dag_list_popup_timer(&mut hub, Some(now - Duration::from_secs(1)), now);
        assert_eq!(hub.next_deadline(), Some(now + DAG_POLL_INTERVAL));
    }

    /// popup 이 닫히면 예약도 사라진다.
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

    /// anchor 마다 독립적인 backoff wakeup 을 갖고, 목록에서 빠지면 걷힌다.
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

        // give-up 하거나 재연결이 끝난 anchor 는 목록에서 빠지고 예약도 사라진다.
        sync_reconnect_timers(&mut hub, &[(4, at + Duration::from_secs(1))], t0);
        assert!(!hub.is_registered(Tick::Reconnect(3)));
        assert!(hub.is_registered(Tick::Reconnect(4)));
    }

    /// 재연결 정리는 다른 키를 건드리지 않는다.
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
