//! 리페인트 요청 상한(coalescing gate) + 유발원 계측.
//!
//! [`View::mark_dirty`](crate::view::ui::View::mark_dirty) 는 dirty 플래그를 세우는 데
//! 그치지 않고 그 자리에서 `Window::request_redraw()` 를 호출한다 — 즉 **`mark_dirty()`
//! 호출 = 프레임 1 회 요청**이다. 유발 경로는 여러 갈래지만 전부 이 한 지점으로
//! 수렴하므로, 상한도 여기 한 곳에 건다.
//!
//! # 왜 상한이 필요한가
//!
//! 프레임 하나의 비용은 환경에 따라 자릿수가 다르다. 원격 데스크톱(xrdp 계열)을
//! 경유하면 GPU 스캔아웃 경로가 없어 present 마다 GPU→CPU readback → X11 `PutImage`
//! → 서버측 재인코딩을 타므로, 프레임당 화면 전체(1920×1080×4B ≈ 8MB)가 소켓으로
//! 흐른다. tasty 는 원격 attach 를 1급 시나리오로 두므로(`docs/identity.md`) 이
//! 경로는 예외 상황이 아니다. 그리고 **디스플레이 주사율을 넘겨 그린 프레임은 화면에
//! 나타나지 못한 채 그 비용만 물고 버려진다** — 상한이 없으면 에이전트 여럿이 동시에
//! 출력을 쏟는 tasty 의 주 워크로드에서 그 초과분이 그대로 쌓인다.
//!
//! # 무엇을 묶고 무엇을 통과시키는가
//!
//! 사용자 조작발 요청([`RepaintSource::Interactive`])은 **항상 즉시** 통과한다. 상한을
//! 걸면 타이핑·클릭 반응성이 그만큼 그대로 나빠지기 때문이다. 반대로 출력·애니메이션발
//! 요청은 사람이 개별 프레임을 구분하지 못하므로 주사율까지 묶는다.
//!
//! # dirty 를 억제하지는 않는다
//!
//! 게이트는 `request_redraw()` 를 **미룰** 뿐 `dirty` 를 지우거나 요청을 버리지 않는다.
//! 미뤄진 요청은 [`RepaintGate::deferred_deadline`] 이 알려주는 시각에
//! `about_to_wait` 이 `ControlFlow::WaitUntil` 로 재예약해 반드시 발화한다.
//! `render_if_dirty`(`src/view/main/redraw.rs`)의 doc 주석이 명시하듯 attach 서버의
//! 원격 mirror 중계는 `dirty` 프레임에 종속돼 있어, 프레임을 **없애면** 원격 사용자
//! 화면이 굶는다. 상한은 cadence 만 주사율에 맞추고 프레임 자체는 계속 흐르게 한다.
//!
//! # 계측
//!
//! 유발원별 통과/지연 횟수와 실제 present 횟수를 1 초 창으로 집계해 한 줄 dump 한다.
//! `src/gfx/perf.rs` 와 같은 관례 — 전용 target 이라 기본 stderr 필터(warn)에서는
//! 나오지 않는다. `TASTY_LOG=tasty::view::repaint=info` 로 stderr 에 켜고, dev 빌드는
//! 파일 필터가 `debug` 라 `~/.tasty-debug/debug-dev.log` 에 그냥 남는다.

use std::time::{Duration, Instant};

/// 리페인트 요청의 유발원. 상한 적용 여부와 계측 분류를 함께 결정한다.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum RepaintSource {
    /// 키·마우스·IME·리사이즈·포커스 등 사용자 조작과 구조 변경. 즉시 통과.
    Interactive,
    /// PTY 출력 반영. 상한 대상.
    TerminalOutput,
    /// egui 내부의 delay 0 즉시 repaint(진행 중 애니메이션 등). 상한 대상.
    EguiAnimation,
    /// attach mirror(원격 화면 중계) 갱신. 상한 대상.
    AttachMirror,
}

impl RepaintSource {
    /// 상한(coalesce) 대상인가.
    fn coalescable(self) -> bool {
        !matches!(self, Self::Interactive)
    }
}

/// 주사율을 얻지 못했을 때의 기본 상한. 오늘날 가장 흔한 주사율이라 여기 맞추면
/// 대부분의 환경에서 눈에 띄는 손해가 없다.
const FALLBACK_REFRESH_HZ: u32 = 60;
/// 주사율 재조회 주기. 창을 다른 모니터로 옮기거나 모드가 바뀌는 것을 따라가되,
/// `current_monitor()` 조회(X11 은 서버 왕복이 있을 수 있다)를 프레임마다 하지는 않는다.
const REFRESH_RATE_TTL: Duration = Duration::from_secs(5);
/// 주사율 clamp 하한 — 비정상적으로 낮은 보고값이 화면을 얼리지 못하게 한다.
const MIN_REFRESH_HZ: u32 = 24;
/// 주사율 clamp 상한 — 비정상적으로 높은 보고값이 상한을 사실상 무력화하지 못하게 한다.
const MAX_REFRESH_HZ: u32 = 480;
/// 계측 집계 창.
const STATS_WINDOW: Duration = Duration::from_secs(1);

/// 모니터가 보고한 주사율(millihertz)에서 최소 프레임 간격을 구한다.
///
/// 고정 상수로 박지 않는 이유는 고주사율 환경에서 그 상수가 **오히려 상한**이 되기
/// 때문이다. 보고값이 없거나(0 포함) 상식 범위를 벗어나면 clamp 해 방어한다.
fn min_interval_from_millihertz(millihertz: Option<u32>) -> Duration {
    let mhz = millihertz
        .filter(|m| *m > 0)
        .unwrap_or(FALLBACK_REFRESH_HZ * 1000)
        .clamp(MIN_REFRESH_HZ * 1000, MAX_REFRESH_HZ * 1000);
    // ns/frame = 1e12 / mHz — Hz 로 내림하지 않아 59.94Hz 같은 값도 그대로 반영된다.
    Duration::from_nanos(1_000_000_000_000u64 / u64::from(mhz))
}

/// 창 하나의 리페인트 요청 상한 상태.
pub struct RepaintGate {
    /// 현재 적용 중인 최소 프레임 간격.
    min_interval: Duration,
    /// `min_interval` 을 마지막으로 모니터에서 다시 구한 시각.
    interval_checked_at: Option<Instant>,
    /// 마지막으로 실제 프레임을 그린 시각. 상한 창의 기준점.
    last_present: Option<Instant>,
    /// 상한에 걸려 미뤄진 요청의 발화 예정 시각.
    deferred_until: Option<Instant>,
    stats: RepaintStats,
}

impl RepaintGate {
    pub fn new() -> Self {
        Self::with_min_interval(min_interval_from_millihertz(None))
    }

    fn with_min_interval(min_interval: Duration) -> Self {
        Self {
            min_interval,
            interval_checked_at: None,
            last_present: None,
            deferred_until: None,
            stats: RepaintStats::default(),
        }
    }

    /// 요청 하나를 받아 **지금 `request_redraw()` 를 할지** 판정한다.
    ///
    /// `false` 를 반환해도 요청은 버려지지 않는다 — [`Self::deferred_deadline`] 으로
    /// 미뤄질 뿐이며, 호출자(`about_to_wait`)가 그 시각에 반드시 발화시킨다.
    pub fn admit(
        &mut self,
        source: RepaintSource,
        now: Instant,
        window: &winit::window::Window,
    ) -> bool {
        self.refresh_min_interval(now, window);
        self.admit_at(source, now)
    }

    /// [`Self::admit`] 에서 모니터 조회만 뺀 판정 본체 — 시각을 주입할 수 있어
    /// 헤드리스 테스트가 가능하다.
    fn admit_at(&mut self, source: RepaintSource, now: Instant) -> bool {
        let admitted = !source.coalescable() || self.window_open(now);
        if admitted {
            // 지금 그리는 프레임이 미뤄뒀던 요청까지 덮는다.
            self.deferred_until = None;
        } else {
            let at = self.next_allowed(now);
            self.deferred_until = Some(self.deferred_until.map_or(at, |cur| cur.min(at)));
        }
        self.stats
            .note_request(source, admitted, now, self.min_interval);
        admitted
    }

    /// 미뤄둔 요청이 만기됐으면 슬롯을 비우고 `true` — 호출자가 `request_redraw()` 한다.
    pub fn take_due(&mut self, now: Instant) -> bool {
        match self.deferred_until {
            Some(at) if at <= now => {
                self.deferred_until = None;
                true
            }
            _ => false,
        }
    }

    /// 아직 만기 전인 미뤄둔 요청의 발화 예정 시각. `about_to_wait` 이 이 값으로
    /// `WaitUntil` 을 재예약한다 — 빠뜨리면 미뤄진 프레임이 영영 오지 않는다.
    pub fn deferred_deadline(&self) -> Option<Instant> {
        self.deferred_until
    }

    /// 실제로 프레임을 그리기로 확정한 지점. 다음 상한 창의 기준점을 갱신한다.
    pub fn note_present(&mut self, now: Instant) {
        self.last_present = Some(now);
        // 이 프레임이 미뤄뒀던 요청을 소진한다.
        self.deferred_until = None;
        self.stats.note_present(now, self.min_interval);
    }

    /// 지금 즉시 그려도 상한을 넘지 않는가.
    fn window_open(&self, now: Instant) -> bool {
        self.last_present
            .is_none_or(|t| now.duration_since(t) >= self.min_interval)
    }

    /// 다음으로 그려도 되는 가장 이른 시각.
    fn next_allowed(&self, now: Instant) -> Instant {
        self.last_present
            .map_or(now, |t| (t + self.min_interval).max(now))
    }

    /// TTL 이 지났으면 현재 모니터의 주사율에서 상한을 다시 구한다.
    fn refresh_min_interval(&mut self, now: Instant, window: &winit::window::Window) {
        if self
            .interval_checked_at
            .is_some_and(|t| now.duration_since(t) < REFRESH_RATE_TTL)
        {
            return;
        }
        self.interval_checked_at = Some(now);
        let millihertz = window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz());
        self.min_interval = min_interval_from_millihertz(millihertz);
    }
}

impl Default for RepaintGate {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Default, Clone, Copy)]
struct SourceCount {
    admitted: u32,
    deferred: u32,
}

/// 1 초 창 집계. 요청 수와 **실제 present 수**를 따로 세야 "요청이 많아도 실제
/// 렌더는 합쳐졌다" 를 구분할 수 있다.
#[derive(Default)]
struct RepaintStats {
    interactive: SourceCount,
    terminal_output: SourceCount,
    egui: SourceCount,
    attach_mirror: SourceCount,
    presents: u32,
    window_start: Option<Instant>,
}

impl RepaintStats {
    fn note_request(
        &mut self,
        source: RepaintSource,
        admitted: bool,
        now: Instant,
        min_interval: Duration,
    ) {
        let slot = match source {
            RepaintSource::Interactive => &mut self.interactive,
            RepaintSource::TerminalOutput => &mut self.terminal_output,
            RepaintSource::EguiAnimation => &mut self.egui,
            RepaintSource::AttachMirror => &mut self.attach_mirror,
        };
        if admitted {
            slot.admitted += 1;
        } else {
            slot.deferred += 1;
        }
        self.maybe_dump(now, min_interval);
    }

    fn note_present(&mut self, now: Instant, min_interval: Duration) {
        self.presents += 1;
        self.maybe_dump(now, min_interval);
    }

    fn maybe_dump(&mut self, now: Instant, min_interval: Duration) {
        let start = *self.window_start.get_or_insert(now);
        let elapsed = now.duration_since(start);
        if elapsed < STATS_WINDOW {
            return;
        }
        tracing::info!(
            target: "tasty::view::repaint",
            "repaint {:.1}s cap={:.1}fps present={} \
             interactive={}+{}d output={}+{}d egui={}+{}d mirror={}+{}d",
            elapsed.as_secs_f64(),
            1.0 / min_interval.as_secs_f64(),
            self.presents,
            self.interactive.admitted,
            self.interactive.deferred,
            self.terminal_output.admitted,
            self.terminal_output.deferred,
            self.egui.admitted,
            self.egui.deferred,
            self.attach_mirror.admitted,
            self.attach_mirror.deferred,
        );
        *self = Self {
            window_start: Some(now),
            ..Self::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 상한값은 모니터 주사율을 따라간다 — 50Hz(xrdp 가상 디스플레이)든 144Hz 든.
    #[test]
    fn min_interval_tracks_reported_refresh_rate() {
        assert_eq!(
            min_interval_from_millihertz(Some(50_000)),
            Duration::from_millis(20)
        );
        assert_eq!(
            min_interval_from_millihertz(Some(144_000)),
            Duration::from_nanos(6_944_444)
        );
    }

    /// 보고값이 없으면 60Hz 기본값. 고정 상수를 박지 않는 이유의 반대면 —
    /// 알 수 없을 때만 쓰는 폴백이다.
    #[test]
    fn min_interval_falls_back_to_60hz() {
        assert_eq!(
            min_interval_from_millihertz(None),
            min_interval_from_millihertz(Some(FALLBACK_REFRESH_HZ * 1000))
        );
        assert_eq!(
            min_interval_from_millihertz(None),
            Duration::from_nanos(16_666_666)
        );
    }

    /// 비정상 보고값은 clamp 한다 — 0 은 폴백으로, 극단값은 범위 안으로.
    #[test]
    fn min_interval_clamps_absurd_rates() {
        assert_eq!(
            min_interval_from_millihertz(Some(0)),
            min_interval_from_millihertz(None)
        );
        assert_eq!(
            min_interval_from_millihertz(Some(1)),
            min_interval_from_millihertz(Some(MIN_REFRESH_HZ * 1000))
        );
        assert_eq!(
            min_interval_from_millihertz(Some(10_000_000)),
            min_interval_from_millihertz(Some(MAX_REFRESH_HZ * 1000))
        );
    }

    /// 사용자 조작발 요청은 상한과 무관하게 언제나 즉시 통과한다.
    #[test]
    fn interactive_requests_always_pass() {
        let mut gate = RepaintGate::with_min_interval(Duration::from_millis(20));
        let t0 = Instant::now();
        gate.note_present(t0);
        for i in 0..10 {
            assert!(gate.admit_at(RepaintSource::Interactive, t0 + Duration::from_millis(i)));
        }
        assert_eq!(gate.deferred_deadline(), None);
    }

    /// 출력발 요청은 상한 창 안에서 묶이고, 만기 시각이 예약된다.
    #[test]
    fn output_requests_coalesce_within_the_window() {
        let mut gate = RepaintGate::with_min_interval(Duration::from_millis(20));
        let t0 = Instant::now();
        gate.note_present(t0);

        // 창이 열리기 전 요청들은 전부 하나의 만기 시각으로 접힌다.
        for i in 1..=5 {
            assert!(!gate.admit_at(RepaintSource::TerminalOutput, t0 + Duration::from_millis(i)));
        }
        assert_eq!(
            gate.deferred_deadline(),
            Some(t0 + Duration::from_millis(20))
        );

        // 만기 전에는 발화하지 않는다.
        assert!(!gate.take_due(t0 + Duration::from_millis(19)));
        // 만기 후 정확히 1 회 발화하고 슬롯이 비워진다 — 유실도 중복도 없다.
        assert!(gate.take_due(t0 + Duration::from_millis(20)));
        assert!(!gate.take_due(t0 + Duration::from_millis(21)));
        assert_eq!(gate.deferred_deadline(), None);
    }

    /// 창이 이미 열려 있으면 출력발 요청도 즉시 통과한다(상시 20ms 지연이 아니다).
    #[test]
    fn output_request_passes_once_the_window_reopens() {
        let mut gate = RepaintGate::with_min_interval(Duration::from_millis(20));
        let t0 = Instant::now();
        gate.note_present(t0);
        assert!(gate.admit_at(
            RepaintSource::TerminalOutput,
            t0 + Duration::from_millis(20)
        ));
    }

    /// 즉시 통과한 요청은 미뤄뒀던 요청까지 덮는다 — 같은 프레임이 둘 다 반영한다.
    #[test]
    fn immediate_pass_absorbs_a_pending_deferral() {
        let mut gate = RepaintGate::with_min_interval(Duration::from_millis(20));
        let t0 = Instant::now();
        gate.note_present(t0);
        assert!(!gate.admit_at(RepaintSource::TerminalOutput, t0 + Duration::from_millis(1)));
        assert!(gate.deferred_deadline().is_some());
        assert!(gate.admit_at(RepaintSource::Interactive, t0 + Duration::from_millis(2)));
        assert_eq!(gate.deferred_deadline(), None);
    }

    /// present 는 미뤄둔 요청을 소진한다 — 이미 그린 화면을 위해 한 프레임 더 깨우지 않는다.
    #[test]
    fn present_consumes_the_deferral() {
        let mut gate = RepaintGate::with_min_interval(Duration::from_millis(20));
        let t0 = Instant::now();
        gate.note_present(t0);
        assert!(!gate.admit_at(RepaintSource::EguiAnimation, t0 + Duration::from_millis(1)));
        gate.note_present(t0 + Duration::from_millis(2));
        assert_eq!(gate.deferred_deadline(), None);
    }

    /// 아직 한 번도 그리지 않은 창은 첫 프레임을 막지 않는다.
    #[test]
    fn first_frame_is_never_deferred() {
        let mut gate = RepaintGate::with_min_interval(Duration::from_millis(20));
        assert!(gate.admit_at(RepaintSource::TerminalOutput, Instant::now()));
    }
}
