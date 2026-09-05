//! 이벤트 루프 stall 워치독 — winit 콜백이 임계 시간 안에 반환하지 않는 상황을
//! 독립 스레드에서 관측해 로그로 남긴다. **복구는 하지 않는다(관측 전용).**
//!
//! ## 왜 필요한가
//!
//! winit `ApplicationHandler` 콜백은 전부 이벤트 루프 스레드에서 동기 실행되고,
//! `WindowEvent::RedrawRequested` 처리 안에서 GPU 렌더가 같은 스레드로 돈다. 그래서
//! GPU 호출 하나가 반환하지 않으면 이벤트 펌프 자체가 멎어 키·마우스·IPC 가 전부
//! 무응답이 된다. 이때 panic 은 일어나지 않으므로 panic hook 도, crash report 도
//! 남지 않는다 — 프로세스를 강제 종료하고 나면 **아무 증거도 남지 않는다**.
//!
//! 이 워치독은 그 상황에서 유일하게 살아 있는 관측자로서 "어느 콜백의 어느 단계에서
//! 몇 초째 멎었는지" 를 남긴다. 기록처는 두 곳이다:
//!
//! - `tracing::error!(target: "tasty::stall")` — stderr + `~/.tasty/debug.log`(release 는 warn 이상).
//! - `~/.tasty/crash-reports/hang-<ts>.log` — stall 당 1 개
//!   ([`crate::crash_report::write_hang_report`]).
//!
//! 메인 스레드를 **의도적으로** 막는 구간(native 모달 등)은 [`without_stall_watch`] 로
//! 감싸 보고 대상에서 뺀다 — 감싸지 않으면 정상 조작이 행으로 오탐된다.
//!
//! 파일 리포트를 따로 쓰는 이유는 두 가지다. (1) 사용자가 "멎었다" 를 겪은 뒤 실제로
//! 확인하는 곳이 `crash-reports/` 다. (2) 공유 로그는 프로세스 시작마다 truncate 되므로
//! 행 상태에서 `tasty` CLI 가 한 번이라도 실행되면 지워진다 — 리포트 파일은 살아남는다.
//!
//! 결정 근거·대안·재검토 조건: [`docs/adr/0091-render-stall-watchdog-observation-only.md`].

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// 콜백이 이 시간을 넘겨도 반환하지 않으면 처음 보고한다.
///
/// 정상 프레임은 수 ms 이고 `gpu.rs` 의 slow-render 경고선조차 30ms 다. 5 초는
/// "느린 프레임" 으로 설명되지 않는 영역이라 오탐이 사실상 없고, 그러면서도 사용자가
/// "멈췄다" 고 느끼기 시작하는 시점 근처다.
const REPORT_AFTER: Duration = Duration::from_secs(5);

/// 같은 stall 이 계속될 때 재보고 간격 (로그 폭주 방지).
const REPEAT_EVERY: Duration = Duration::from_secs(30);

/// 워치독 스레드 폴링 주기.
const POLL_INTERVAL: Duration = Duration::from_secs(1);

/// 진입 시각을 `u64` 나노초로 눕히기 위한 기준점.
static ORIGIN: LazyLock<Instant> = LazyLock::new(Instant::now);

/// 콜백 진입/이탈 시퀀스. **홀수 = 콜백 안, 짝수 = 바깥.**
static SEQ: AtomicU64 = AtomicU64::new(0);
/// 현재 콜백 진입 시각 (`ORIGIN` 기준 나노초).
static ENTERED_NS: AtomicU64 = AtomicU64::new(0);
/// 현재 콜백 종류 ([`Site`] 의 discriminant).
static SITE: AtomicU8 = AtomicU8::new(0);
/// 렌더 세부 단계 ([`Phase`] 의 discriminant).
static PHASE: AtomicU8 = AtomicU8::new(0);
/// 의도적 블로킹 구간의 중첩 깊이 ([`without_stall_watch`]). 0 보다 크면 보고하지 않는다.
static PAUSED: AtomicU32 = AtomicU32::new(0);

/// 관측 대상 winit 콜백.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Site {
    Resumed = 1,
    /// `RedrawRequested` 를 제외한 `window_event`.
    WindowEvent = 2,
    /// `WindowEvent::RedrawRequested` — GPU 렌더가 도는 경로.
    Redraw = 3,
    UserEvent = 4,
    AboutToWait = 5,
}

fn site_label(raw: u8) -> &'static str {
    match raw {
        1 => "resumed",
        2 => "window_event",
        3 => "redraw",
        4 => "user_event",
        5 => "about_to_wait",
        _ => "unknown",
    }
}

/// 렌더 안의 세부 단계. 어느 GPU 호출이 안 돌아왔는지 구분하는 것이 목적이다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Phase {
    /// GPU 호출 구간 밖 (또는 phase 를 계측하지 않는 렌더 경로).
    None = 0,
    /// `surface.get_current_texture()`.
    Acquire = 1,
    /// 렌더 패스 기록 + `queue.submit(...)`.
    Submit = 2,
    /// `SurfaceTexture::present()`.
    Present = 3,
}

fn phase_label(raw: u8) -> &'static str {
    match raw {
        0 => "none",
        1 => "acquire",
        2 => "submit",
        3 => "present",
        _ => "unknown",
    }
}

/// 콜백 구간을 표시하는 RAII 가드. 콜백 최상단에서 만들고, 조기 return 을 포함한
/// 모든 이탈 경로를 `Drop` 이 덮는다.
pub struct Guard;

impl Guard {
    pub fn enter(site: Site) -> Self {
        SITE.store(site as u8, Ordering::Relaxed);
        PHASE.store(Phase::None as u8, Ordering::Relaxed);
        ENTERED_NS.store(ORIGIN.elapsed().as_nanos() as u64, Ordering::Relaxed);
        // payload 를 다 쓴 뒤 마지막에 홀수로 올린다 — 워치독이 "안에 있음" 으로
        // 읽는 순간엔 site/phase/시각이 이미 이 콜백의 값이다.
        SEQ.fetch_add(1, Ordering::Release);
        Guard
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        SEQ.fetch_add(1, Ordering::Release);
        PHASE.store(Phase::None as u8, Ordering::Relaxed);
    }
}

/// 렌더 세부 단계 표시. 가드 안에서만 의미가 있고, 가드 `Drop` 이 자동으로 되돌린다.
#[inline]
pub fn set_phase(phase: Phase) {
    PHASE.store(phase as u8, Ordering::Relaxed);
}

/// 메인 스레드를 **의도적으로** 오래 막는 구간을 실행한다 — 그 사이 워치독은 보고하지
/// 않고, 구간이 끝나면 진입 시각을 다시 잡아 거기 쓴 시간이 stall 로 누적되지 않게 한다.
///
/// 대상은 native 모달처럼 "돌아오지 않는 것이 정상" 인 동기 호출이다. tasty 의 파일·폴더
/// 선택(`rfd::FileDialog`)은 macOS 요구사항 때문에 메인 스레드에서 동기로 열리므로
/// (`src/view/plugins/ui/add.rs` 참조 — 이 호출은 **사용자 조작 경로에만** 있다.
/// IPC 쪽 대응물이던 `fs.pick_file` 은 "돌아오지 않는 모달은 에이전트 표면이 아니다"
/// 로 제거됐다), 감싸지 않으면 사용자가 선택에 5 초만 써도
/// `crash-reports/` 에 "GPU driver hang suspected" 리포트가 남는다. 그러면 "그 디렉토리에
/// 파일이 있다 = 행이 있었다" 라는 이 워치독의 전제가 무너진다.
///
/// **메인 스레드를 막는 동기 호출을 새로 추가하면 이 래퍼를 통과시킨다.**
pub fn without_stall_watch<T>(f: impl FnOnce() -> T) -> T {
    let _pause = PauseGuard::enter();
    f()
}

/// [`without_stall_watch`] 의 RAII 본체. `f` 가 panic 해도 `Drop` 이 구간을 닫는다.
struct PauseGuard;

impl PauseGuard {
    fn enter() -> Self {
        PAUSED.fetch_add(1, Ordering::Release);
        PauseGuard
    }
}

impl Drop for PauseGuard {
    fn drop(&mut self) {
        // 블로킹에 쓴 시간은 stall 이 아니다 — 남은 콜백을 지금부터 다시 잰다.
        ENTERED_NS.store(ORIGIN.elapsed().as_nanos() as u64, Ordering::Relaxed);
        // seq 를 2 만큼 올린다: 홀짝(= 콜백 안)은 유지하면서 ① 진행 중인 표본을 무효화하고
        // ② 재보고 추적이 이 이후를 별개 구간으로 보게 한다.
        SEQ.fetch_add(2, Ordering::Release);
        // 시각을 갱신한 뒤에 푼다 — 반대 순서면 낡은 진입 시각이 잠깐 노출된다.
        PAUSED.fetch_sub(1, Ordering::Release);
    }
}

/// 관측 스냅샷 — 보고 판정을 순수 함수로 떼어내기 위한 값 묶음.
#[derive(Clone, Copy, Debug)]
struct Snapshot {
    seq: u64,
    stuck: Duration,
    /// 표본 시점에 의도적 블로킹 구간([`without_stall_watch`]) 안이었나.
    paused: bool,
}

/// 직전 보고 상태.
#[derive(Clone, Copy, Debug, Default)]
struct Reported {
    seq: u64,
    at: Duration,
}

/// 이번 폴링에서 보고할지 판정한다. `None` 이면 보고하지 않는다.
/// 반환하는 `bool` 은 "이 stall 의 첫 보고인가".
fn should_report(snap: Snapshot, reported: Option<Reported>) -> Option<bool> {
    if snap.seq.is_multiple_of(2) {
        return None; // 콜백 바깥 — 정상
    }
    if snap.paused {
        return None; // 의도적 블로킹(native 모달 등) — 행이 아니다
    }
    if snap.stuck < REPORT_AFTER {
        return None;
    }
    match reported {
        Some(prev) if prev.seq == snap.seq => {
            // 같은 stall — 재보고 간격을 채웠을 때만.
            (snap.stuck.saturating_sub(prev.at) >= REPEAT_EVERY).then_some(false)
        }
        _ => Some(true),
    }
}

/// 워치독 스레드를 띄운다. 이벤트 루프 진입 전에 한 번 호출한다.
pub fn spawn() {
    LazyLock::force(&ORIGIN);
    if let Err(e) = std::thread::Builder::new()
        .name("tasty-stall-watchdog".to_string())
        .spawn(watch_loop)
    {
        // 워치독이 없어도 앱 동작은 동일하다 — 진단이 빠질 뿐이라 warn 으로 남긴다.
        tracing::warn!("stall watchdog thread spawn failed: {e}");
    }
}

/// 이벤트 루프 상태를 한 번 읽는다. 읽는 도중 콜백이 교체되면 `None`(그 표본은 버린다).
fn sample() -> Option<Snapshot> {
    let seq = SEQ.load(Ordering::Acquire);
    let paused = PAUSED.load(Ordering::Acquire) > 0;
    let entered = Duration::from_nanos(ENTERED_NS.load(Ordering::Relaxed));
    let stuck = ORIGIN.elapsed().checked_sub(entered)?;
    // 읽는 사이에 콜백이 끝났거나 블로킹 구간이 닫혔으면(seq += 2) 위 값들은 과거 것이다.
    (SEQ.load(Ordering::Acquire) == seq).then_some(Snapshot { seq, stuck, paused })
}

/// stall 1 건 보고. `first` 면 파일 리포트까지 남긴다(재보고는 로그만 — 파일 폭주 방지).
fn report(stuck: Duration, first: bool) {
    let site = site_label(SITE.load(Ordering::Relaxed));
    let phase = phase_label(PHASE.load(Ordering::Relaxed));
    let stuck_ms = stuck.as_millis() as u64;

    tracing::error!(
        target: "tasty::stall",
        site,
        phase,
        stuck_ms,
        first,
        "event loop stalled — winit callback has not returned; \
         input/IPC are blocked until it does (GPU driver hang suspected)"
    );

    if first {
        write_report_file(site, phase, stuck_ms);
    }
}

/// stall 리포트 파일 1 개를 남기고 결과를 로그로 확인한다.
fn write_report_file(site: &str, phase: &str, stuck_ms: u64) {
    // 리포트를 못 써도 호출자의 error 로그는 이미 남았다 — 진단이 얕아질 뿐이다.
    let written = crate::crash_report::write_hang_report(site, phase, stuck_ms);
    tracing::error!(
        target: "tasty::stall",
        path = ?written,
        "hang report written (path = None means the write failed)"
    );
}

fn watch_loop() {
    let mut reported: Option<Reported> = None;
    loop {
        std::thread::sleep(POLL_INTERVAL);

        let Some(snap) = sample() else { continue };
        let Some(first) = should_report(snap, reported) else {
            continue;
        };
        reported = Some(Reported {
            seq: snap.seq,
            at: snap.stuck,
        });
        report(snap.stuck, first);
    }
}

// =============================================================================
// Debug-only: 결함 주입 (워치독 자체를 검증하기 위한 인위적 블로킹)
// =============================================================================

#[cfg(debug_assertions)]
static DEBUG_STALL_MS: AtomicU64 = AtomicU64::new(0);

/// 다음 프레임의 `present` 직전을 한 번 블로킹하도록 예약한다 (debug 전용).
///
/// 실제 드라이버 행을 결정적으로 재현할 수는 없으므로, "GPU 호출이 반환하지 않으면
/// 이벤트 펌프가 멎는다" 는 구조를 재현하는 최소 수단으로 둔다.
#[cfg(debug_assertions)]
pub fn arm_debug_stall(ms: u64) {
    DEBUG_STALL_MS.store(ms, Ordering::Relaxed);
}

/// 예약된 결함 주입이 있으면 소비해 그 시간만큼 블로킹한다. release 는 no-op.
#[cfg(debug_assertions)]
pub fn take_debug_stall() {
    let ms = DEBUG_STALL_MS.swap(0, Ordering::Relaxed);
    if ms > 0 {
        tracing::warn!(target: "tasty::stall", ms, "debug stall injected before present");
        std::thread::sleep(Duration::from_millis(ms));
    }
}

/// release 빌드에는 결함 주입이 없다.
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn take_debug_stall() {}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    /// `SEQ` / `PAUSED` 는 프로세스 전역이라, 그것을 실제로 건드리는 테스트끼리는
    /// 직렬화해야 한다(테스트는 기본적으로 병렬 실행된다).
    static GLOBALS: Mutex<()> = Mutex::new(());

    fn snap(seq: u64, secs: u64) -> Snapshot {
        Snapshot {
            seq,
            stuck: Duration::from_secs(secs),
            paused: false,
        }
    }

    fn paused_snap(seq: u64, secs: u64) -> Snapshot {
        Snapshot {
            paused: true,
            ..snap(seq, secs)
        }
    }

    #[test]
    fn even_seq_means_outside_callback_never_reports() {
        assert_eq!(should_report(snap(4, 600), None), None);
    }

    #[test]
    fn short_callback_does_not_report() {
        assert_eq!(should_report(snap(3, 1), None), None);
    }

    #[test]
    fn first_long_stall_reports_as_first() {
        assert_eq!(should_report(snap(3, 5), None), Some(true));
    }

    #[test]
    fn same_stall_waits_for_repeat_interval() {
        let prev = Reported {
            seq: 3,
            at: Duration::from_secs(5),
        };
        // 5s → 20s: 재보고 간격(30s) 미달.
        assert_eq!(should_report(snap(3, 20), Some(prev)), None);
        // 5s → 35s: 간격 충족, 첫 보고는 아니다.
        assert_eq!(should_report(snap(3, 35), Some(prev)), Some(false));
    }

    #[test]
    fn new_stall_after_previous_report_is_first_again() {
        let prev = Reported {
            seq: 3,
            at: Duration::from_secs(40),
        };
        assert_eq!(should_report(snap(7, 6), Some(prev)), Some(true));
    }

    #[test]
    fn intentional_block_is_not_reported() {
        // 콜백 안(홀수) + 임계 초과지만 의도적 블로킹 구간이면 보고하지 않는다.
        assert_eq!(should_report(paused_snap(3, 600), None), None);
    }

    #[test]
    fn pause_keeps_parity_and_invalidates_the_sample() {
        let _lock = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let _g = Guard::enter(Site::UserEvent);
        let inside = SEQ.load(Ordering::Acquire);
        assert!(!inside.is_multiple_of(2), "가드 안 = 홀수");

        without_stall_watch(|| {
            assert!(PAUSED.load(Ordering::Acquire) > 0, "구간 안 = paused");
        });

        assert_eq!(PAUSED.load(Ordering::Acquire), 0, "구간을 벗어나면 풀린다");
        let after = SEQ.load(Ordering::Acquire);
        assert_eq!(
            after,
            inside + 2,
            "seq 는 2 만큼 올라 홀짝(콜백 안)을 유지한다"
        );
        assert!(
            sample().is_none_or(|s| s.stuck < REPORT_AFTER),
            "구간이 끝나면 진입 시각이 다시 잡힌다"
        );
    }

    #[test]
    fn guard_toggles_seq_parity() {
        let _lock = GLOBALS.lock().unwrap_or_else(|e| e.into_inner());
        let before = SEQ.load(Ordering::Acquire);
        {
            let _g = Guard::enter(Site::Redraw);
            assert_eq!(
                SEQ.load(Ordering::Acquire),
                before + 1,
                "가드 안 = 홀수 전이"
            );
            set_phase(Phase::Present);
            assert_eq!(PHASE.load(Ordering::Relaxed), Phase::Present as u8);
        }
        assert_eq!(
            SEQ.load(Ordering::Acquire),
            before + 2,
            "가드 밖 = 짝수 전이"
        );
        assert_eq!(PHASE.load(Ordering::Relaxed), Phase::None as u8);
    }

    #[test]
    fn labels_cover_every_variant() {
        assert_eq!(site_label(Site::Resumed as u8), "resumed");
        assert_eq!(site_label(Site::WindowEvent as u8), "window_event");
        assert_eq!(site_label(Site::Redraw as u8), "redraw");
        assert_eq!(site_label(Site::UserEvent as u8), "user_event");
        assert_eq!(site_label(Site::AboutToWait as u8), "about_to_wait");
        assert_eq!(site_label(0), "unknown");

        assert_eq!(phase_label(Phase::None as u8), "none");
        assert_eq!(phase_label(Phase::Acquire as u8), "acquire");
        assert_eq!(phase_label(Phase::Submit as u8), "submit");
        assert_eq!(phase_label(Phase::Present as u8), "present");
        assert_eq!(phase_label(9), "unknown");
    }
}
