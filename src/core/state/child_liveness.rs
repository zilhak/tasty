//! child-terminal 상태의 **관측 축 합성** — hook push 캐시(`ChildTerminalRegistry`)
//! 단독 판정을 라이브 surface 트리 · PTY busy · 무출력 경과시간 · 전경 프로그램과
//! 융합해 파생 상태를 만든다 (ADR-0072).
//!
//! # 왜 필요한가
//!
//! `ChildTerminalRegistry::state_of` 는 `idle`/`needs_input` bool 맵 두 개를 되읽을
//! 뿐이라, 둘 다 false 면 `"active"` 를 반환한다 — 그 값의 실제 의미는 "작업 중" 이
//! 아니라 **"idle 이라는 증거가 없음"** 이다. 상태를 바꾸는 유일한 경로가 에이전트
//! hook push 단방향이므로, hook 이 한 번이라도 유실되거나 자식 프로세스가 멈추면
//! 마지막으로 찍힌 `active` 가 영구히 남고 되돌리는 경로가 없다. 소비 에이전트는
//! 그것을 "지금 작업 중" 으로 읽어 이미 끝난 자식을 무한히 기다린다.
//!
//! # 계층 경계
//!
//! `state_of` 와 그 계약(미등록 surface → `"active"`)은 **불변**이다. registry 는
//! host-IPC-free 단위 테스트 계층이라 터미널/프로세스에 접근할 수 없다. 관측 축은
//! `CoreState` 를 가진 이 상위 계층에서만 합성하며, registry 는 "언제 보고받았나"
//! 축(`last_state_report_at`)만 추가로 들고 있다.
//!
//! # 판정은 출력 전용이다
//!
//! 파생 상태(`stale`)는 `terminal.set_state` 의 **입력으로 받지 않는다**. hook 은
//! `idle`/`needs_input`/`active` 세 값만 push 할 수 있고, `stale` 은 호스트가 관측
//! 으로만 만들어낸다.
//!
//! # 확실성의 한계
//!
//! 무출력 기반 정지 판정은 원리적으로 휴리스틱이다 — SIGSTOP 으로 멈춘 프로세스,
//! 긴 추론 중인 에이전트, 출력이 없는 긴 명령은 관측상 구별되지 않는다. 확정으로
//! 취급 가능한 관측은 **surface 부재**와 **전경 프로세스가 셸로 되돌아옴** 두 가지
//! 뿐이며, 나머지 stale 판정은 [`ChildStateConfidence::Heuristic`] 로 표시된다.
//! 소비자는 confidence 를 보고 확정 판정만 종결로 다룰 수 있다.
//!
//! # 능동 프로빙 배제
//!
//! 대상 surface 에 입력을 주입해 반응을 보는 능동 프로빙은 사용자 입력 재현이라
//! release 금지 대상이고(`docs/identity.md` 원칙 1) 자식 에이전트 상태도 오염시키
//! 므로, 이 모듈은 **수동 관측만** 한다.

use std::collections::HashSet;
use std::time::Duration;

use super::CoreState;

/// PTY 가 이만큼 아무 출력도 내지 않으면 무출력 축이 "침묵" 으로 넘어간다.
///
/// `BUSY_OUTPUT_WINDOW`(2s) 를 그대로 쓸 수 없다 — 그 창은 "지금 화면이 갱신되는
/// 중인가" 를 재는 렌더/상태점 용도라, 사람이 프롬프트를 읽는 몇 초만으로도 즉시
/// 넘어간다. 여기서 재려는 것은 "이 자식이 몇 분째 아무것도 안 한다" 이므로 두
/// 자릿수 배율이 필요하다. 2 분은 에이전트 CLI 의 스피너/진행표시가 초 단위로
/// 출력을 내는 것을 전제로, 그것이 완전히 멎은 상태만 잡도록 잡은 값이다.
pub const CHILD_OUTPUT_SILENCE: Duration = Duration::from_secs(120);

/// registry 가 마지막 상태 보고를 받은 뒤 이만큼 지나면 hook 축이 "침묵" 이다.
///
/// 무출력 축보다 길게 잡는다 — hook 은 상태 **전환** 시에만 발화하므로, 한 작업을
/// 오래 수행하는 정상 자식도 hook 사이 간격이 분 단위로 벌어진다. 5 분은 그 정상
/// 간격보다 확실히 길면서, 사고 사례(2 시간 대기)를 훨씬 못 미쳐 잡는 값이다.
pub const CHILD_HOOK_SILENCE: Duration = Duration::from_secs(300);

/// 파생된 자식 상태. `state_of` 의 세 값에 `exited`/`stale` 이 더해진 집합이며,
/// 이 열거형이 `terminal.children` / `terminal.state` 두 경로의 **공통 SoT** 다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildState {
    /// surface 가 라이브 트리에 없다. 확정 종료 — kill/respawn 대상.
    Exited,
    /// 자식이 입력을 기다린다(hook 보고). 관측이 덮어쓰지 않는다.
    NeedsInput,
    /// 자식이 작업을 마치고 놀고 있다(hook 보고). 관측이 덮어쓰지 않는다.
    Idle,
    /// 활동 중이거나, 정지했다는 증거가 없다.
    Active,
    /// hook 은 `active` 라고 하는데 관측상 활동 증거가 없다 — hook 유실 의심.
    ///
    /// **`exited` 가 아니다.** surface 는 살아 있고, "이 surface 에서 에이전트
    /// 프로세스가 돌고 있지 않다" 는 뜻일 뿐이다. `terminal.adopt` 로 들어온 자식은
    /// 애초에 에이전트가 아닌 일반 셸일 수 있으므로 종료로 단정해선 안 된다.
    Stale,
}

impl ChildState {
    /// IPC 응답에 싣는 문자열. 기존 세 값은 그대로라 하위호환된다.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exited => "exited",
            Self::NeedsInput => "needs_input",
            Self::Idle => "idle",
            Self::Active => "active",
            Self::Stale => "stale",
        }
    }
}

/// 판정을 뒷받침한 관측/보고가 무엇이었나 — 같은 `state` 라도 근거가 갈린다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildStateEvidence {
    /// surface 가 라이브 트리에 없다.
    SurfaceGone,
    /// hook 이 `needs_input` 을 push 했다.
    HookNeedsInput,
    /// hook 이 `idle` 을 push 했다.
    HookIdle,
    /// PTY 가 busy — 셸이 아닌 전경 프로그램이 지금 출력을 내는 중.
    PtyBusy,
    /// PTY 가 아직 기동되지 않았다(deferred terminal). 출력을 낸 적이 없으므로
    /// 무출력 경과시간으로 판정하면 안 된다.
    PtyNotStarted,
    /// 전경 프로그램이 셸로 되돌아왔다 — 이 surface 에서 돌던 프로그램이 끝났다.
    ForegroundIsShell,
    /// 관측 축을 구할 수 없다(로컬 `Terminal` 부재 — mirror surface 등).
    ObservationUnavailable,
    /// 임계값 이내에 PTY 출력이 있었다.
    RecentOutput,
    /// PTY 는 침묵이지만 hook 보고가 임계값 이내였다.
    RecentHookReport,
    /// PTY 무출력 + hook 침묵이 둘 다 임계값을 넘었다.
    OutputAndHookSilent,
}

impl ChildStateEvidence {
    /// IPC 응답 `evidence` 필드에 싣는 근거 슬러그. 슬러그를 판정 열거형과 같은
    /// 파일에 두어 값 집합이 판정과 갈리지 않게 한다.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SurfaceGone => "surface_gone",
            Self::HookNeedsInput => "hook_needs_input",
            Self::HookIdle => "hook_idle",
            Self::PtyBusy => "pty_busy",
            Self::PtyNotStarted => "pty_not_started",
            Self::ForegroundIsShell => "foreground_is_shell",
            Self::ObservationUnavailable => "observation_unavailable",
            Self::RecentOutput => "recent_output",
            Self::RecentHookReport => "recent_hook_report",
            Self::OutputAndHookSilent => "output_and_hook_silent",
        }
    }
}

/// 판정을 얼마나 믿어도 되는가. 소비자가 "종결로 다뤄도 되는 값" 을 고르는 축이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildStateConfidence {
    /// 관측으로 확정. surface 부재 · 전경 셸 복귀 · busy 양성 세 가지뿐이다.
    Confirmed,
    /// 에이전트 hook 이 직접 보고한 값. 관측이 아니라 보고지만, hook 은 거짓 idle 을
    /// 만들지 않으므로 관측이 덮어쓰지 않는다.
    Reported,
    /// 임계값 기반 추정. SIGSTOP · 긴 추론 · 무출력 명령은 구별되지 않는다.
    Heuristic,
    /// 관측 축을 구할 수 없어 registry 캐시를 그대로 되읽었다. 이 값에 근거해
    /// 종결 판정을 내리면 안 된다.
    Unobserved,
}

impl ChildStateConfidence {
    /// IPC 응답 `confidence` 필드에 싣는 문자열.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Reported => "reported",
            Self::Heuristic => "heuristic",
            Self::Unobserved => "unobserved",
        }
    }
}

/// 파생 판정 결과 — `state` + 근거 + 확실성 3 축.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildLiveness {
    pub state: ChildState,
    pub evidence: ChildStateEvidence,
    pub confidence: ChildStateConfidence,
}

/// [`derive_child_state`] 에 주입하는 관측 스냅샷. 호스트가 채우고, 판정 함수는
/// 순수 함수라 단위 테스트에서 임의 조합을 직접 넣을 수 있다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChildObservation {
    /// 라이브 surface 트리에 존재하는가.
    pub surface_live: bool,
    /// PTY 가 기동됐는가(`terminals.contains`). deferred terminal 은 false.
    pub pty_ready: bool,
    /// `CoreState::is_surface_busy` — 로컬 전경 프로그램 또는 mirror push 기준.
    pub busy: bool,
    /// 전경 프로그램이 알려진 셸인가. `None` 이면 전경 미해결(mirror·1Hz 폴링 전).
    pub foreground_is_shell: Option<bool>,
    /// 마지막 PTY 출력 이후 경과. `None` 이면 로컬 `Terminal` 이 없어 관측 불가.
    pub output_silence: Option<Duration>,
    /// 마지막 상태 보고 이후 경과. `None` 이면 잴 기준점이 없다.
    pub hook_silence: Option<Duration>,
}

/// registry 의 원 상태와 관측 스냅샷을 합성해 파생 상태를 만든다 — 순수 함수.
///
/// 우선순위는 위에서 아래로 고정이다:
///
/// 1. surface 부재 → `exited` (확정). 기존 `terminal.state` 단건 동작 보존.
/// 2. hook 이 `needs_input`/`idle` 을 보고했다 → 그대로 유지. hook 은 거짓 idle 을
///    만들지 않으므로 관측이 이 둘을 덮어쓰지 않는다(우선순위도 registry 와 동일).
/// 3. 이하 registry 가 `active` 인 경우:
///    - busy → `active` (확정 관측)
///    - PTY 미기동 → `active` (무출력 판정 게이트 — 출력을 낸 적이 없다)
///    - 전경이 셸 → `stale` (확정: 이 surface 에서 돌던 프로그램이 끝났다)
///    - 무출력 관측 불가 → `active` (판정 불가, 임의값 금지)
///    - 무출력 침묵 && hook 침묵 → `stale` (휴리스틱)
///    - 그 외 → `active` (최근 출력 또는 최근 hook 보고)
pub fn derive_child_state(registry_state: &str, obs: &ChildObservation) -> ChildLiveness {
    use ChildStateConfidence as C;
    use ChildStateEvidence as E;

    let out = |state, evidence, confidence| ChildLiveness {
        state,
        evidence,
        confidence,
    };

    if !obs.surface_live {
        return out(ChildState::Exited, E::SurfaceGone, C::Confirmed);
    }
    match registry_state {
        "needs_input" => return out(ChildState::NeedsInput, E::HookNeedsInput, C::Reported),
        "idle" => return out(ChildState::Idle, E::HookIdle, C::Reported),
        _ => {}
    }

    if obs.busy {
        return out(ChildState::Active, E::PtyBusy, C::Confirmed);
    }
    // deferred terminal 은 출력을 낸 적이 자체가 없다 — 무출력 임계값 판정 **전에**
    // 걸러내지 않으면 spawn 직후 전부 stale 로 오판정된다.
    if !obs.pty_ready {
        return out(ChildState::Active, E::PtyNotStarted, C::Unobserved);
    }
    if obs.foreground_is_shell == Some(true) {
        return out(ChildState::Stale, E::ForegroundIsShell, C::Confirmed);
    }
    // mirror(remote attach) surface 는 로컬 `Terminal` 이 없어 무출력 경과시간을 잴
    // 수 없다. 구할 수 없는 축에 임의 기본값을 넣지 않고 판정 불가로 표시한다.
    let Some(output_silence) = obs.output_silence else {
        return out(ChildState::Active, E::ObservationUnavailable, C::Unobserved);
    };

    if output_silence < CHILD_OUTPUT_SILENCE {
        return out(ChildState::Active, E::RecentOutput, C::Heuristic);
    }
    // hook 침묵 기준점이 없으면(업그레이드 전 영속 항목) 침묵으로 간주한다 —
    // 무출력 축이 이미 임계값을 넘긴 상태라 두 축 모두 반증이 없다.
    let hook_silent = obs
        .hook_silence
        .is_none_or(|silence| silence >= CHILD_HOOK_SILENCE);
    if hook_silent {
        out(ChildState::Stale, E::OutputAndHookSilent, C::Heuristic)
    } else {
        out(ChildState::Active, E::RecentHookReport, C::Heuristic)
    }
}

impl CoreState {
    /// 한 자식 surface 의 관측 스냅샷을 모은다. `live` 는 호출자가 한 번 계산해
    /// 넘긴다(`terminal.children` 이 자식마다 전 워크스페이스를 다시 순회하지 않도록).
    ///
    /// 전경 프로그램 이름은 1Hz 일괄 스냅샷 캐시(`foreground_names`)에서만 읽는다 —
    /// 자식마다 `Terminal::foreground_process_info()` 를 개별 호출하면 O(surfaces ×
    /// processes) 를 되살리는 회귀다(`core/state/busy.rs` 의 폴링 주석 참고).
    fn observe_child(&self, child_surface: u32, live: &HashSet<u32>) -> ChildObservation {
        ChildObservation {
            surface_live: live.contains(&child_surface),
            pty_ready: self.terminals.contains(child_surface),
            busy: self.is_surface_busy(child_surface),
            foreground_is_shell: self
                .foreground_name(child_surface)
                .map(tasty_terminal::foreground_process::is_known_shell_name),
            output_silence: self
                .find_terminal_by_id(child_surface)
                .map(|t| t.last_output_at().elapsed()),
            hook_silence: self
                .child_terminals
                .hook_silence(child_surface, crate::core::child_terminal::now_epoch_ms()),
        }
    }

    /// 자식 surface 의 파생 상태 — `terminal.children` 과 `terminal.state` 가 **같은**
    /// 판정을 쓰도록 하는 단일 진입점. 두 경로가 갈리면 목록과 단건 조회가 서로 다른
    /// 값을 보고한다(개선 전 실제 상태: `exited` 판정이 단건에만 있었다).
    pub fn child_liveness_with_live(
        &self,
        child_surface: u32,
        live: &HashSet<u32>,
    ) -> ChildLiveness {
        let obs = self.observe_child(child_surface, live);
        derive_child_state(self.child_terminals.state_of(child_surface), &obs)
    }

    /// 라이브 집합을 자체 계산하는 단건 판정 편의 래퍼.
    pub fn child_liveness(&self, child_surface: u32) -> ChildLiveness {
        let live = self.live_surface_ids();
        self.child_liveness_with_live(child_surface, &live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// registry 가 `active` 이고 surface 는 살아 있는 "정상 로컬 자식" 기준선.
    fn live_active() -> ChildObservation {
        ChildObservation {
            surface_live: true,
            pty_ready: true,
            busy: false,
            foreground_is_shell: Some(false),
            output_silence: Some(Duration::from_secs(1)),
            hook_silence: Some(Duration::from_secs(1)),
        }
    }

    #[test]
    fn busy_child_stays_active() {
        let obs = ChildObservation {
            busy: true,
            ..live_active()
        };
        let l = derive_child_state("active", &obs);
        assert_eq!(l.state, ChildState::Active);
        assert_eq!(l.evidence, ChildStateEvidence::PtyBusy);
        assert_eq!(l.confidence, ChildStateConfidence::Confirmed);
    }

    #[test]
    fn long_silence_becomes_stale() {
        let obs = ChildObservation {
            output_silence: Some(CHILD_OUTPUT_SILENCE + Duration::from_secs(1)),
            hook_silence: Some(CHILD_HOOK_SILENCE + Duration::from_secs(1)),
            ..live_active()
        };
        let l = derive_child_state("active", &obs);
        assert_eq!(l.state, ChildState::Stale);
        assert_eq!(l.evidence, ChildStateEvidence::OutputAndHookSilent);
        assert_eq!(l.confidence, ChildStateConfidence::Heuristic);
    }

    #[test]
    fn recent_hook_report_holds_active_despite_output_silence() {
        let obs = ChildObservation {
            output_silence: Some(CHILD_OUTPUT_SILENCE + Duration::from_secs(1)),
            hook_silence: Some(Duration::from_secs(5)),
            ..live_active()
        };
        let l = derive_child_state("active", &obs);
        assert_eq!(l.state, ChildState::Active);
        assert_eq!(l.evidence, ChildStateEvidence::RecentHookReport);
    }

    #[test]
    fn missing_hook_baseline_counts_as_silent() {
        let obs = ChildObservation {
            output_silence: Some(CHILD_OUTPUT_SILENCE + Duration::from_secs(1)),
            hook_silence: None,
            ..live_active()
        };
        assert_eq!(
            derive_child_state("active", &obs).state,
            ChildState::Stale,
            "업그레이드 전 영속 항목은 기준점이 없다 — 무출력 축 단독으로 판정"
        );
    }

    #[test]
    fn dead_surface_is_exited_regardless_of_registry() {
        let obs = ChildObservation {
            surface_live: false,
            ..live_active()
        };
        for registry_state in ["active", "idle", "needs_input"] {
            let l = derive_child_state(registry_state, &obs);
            assert_eq!(l.state, ChildState::Exited, "{registry_state}");
            assert_eq!(l.confidence, ChildStateConfidence::Confirmed);
        }
    }

    #[test]
    fn hook_idle_is_not_overwritten_by_observation() {
        let obs = ChildObservation {
            output_silence: Some(CHILD_OUTPUT_SILENCE * 100),
            hook_silence: Some(CHILD_HOOK_SILENCE * 100),
            foreground_is_shell: Some(true),
            ..live_active()
        };
        let l = derive_child_state("idle", &obs);
        assert_eq!(l.state, ChildState::Idle);
        assert_eq!(l.confidence, ChildStateConfidence::Reported);
    }

    #[test]
    fn hook_needs_input_wins_over_busy() {
        let obs = ChildObservation {
            busy: true,
            ..live_active()
        };
        let l = derive_child_state("needs_input", &obs);
        assert_eq!(l.state, ChildState::NeedsInput);
        assert_eq!(l.confidence, ChildStateConfidence::Reported);
    }

    #[test]
    fn foreground_back_to_shell_is_confirmed_stale() {
        let obs = ChildObservation {
            foreground_is_shell: Some(true),
            ..live_active()
        };
        let l = derive_child_state("active", &obs);
        assert_eq!(l.state, ChildState::Stale);
        assert_eq!(l.evidence, ChildStateEvidence::ForegroundIsShell);
        assert_eq!(
            l.confidence,
            ChildStateConfidence::Confirmed,
            "전경 프로세스 부재는 확정 관측"
        );
    }

    #[test]
    fn deferred_pty_is_gated_before_silence_check() {
        let obs = ChildObservation {
            pty_ready: false,
            foreground_is_shell: None,
            output_silence: None,
            hook_silence: Some(CHILD_HOOK_SILENCE * 10),
            ..live_active()
        };
        let l = derive_child_state("active", &obs);
        assert_eq!(l.state, ChildState::Active, "출력을 낸 적이 없는 surface");
        assert_eq!(l.evidence, ChildStateEvidence::PtyNotStarted);
        assert_eq!(l.confidence, ChildStateConfidence::Unobserved);
    }

    #[test]
    fn mirror_surface_without_output_axis_is_unobserved() {
        let obs = ChildObservation {
            foreground_is_shell: None,
            output_silence: None,
            hook_silence: Some(CHILD_HOOK_SILENCE * 10),
            ..live_active()
        };
        let l = derive_child_state("active", &obs);
        assert_eq!(l.state, ChildState::Active);
        assert_eq!(l.evidence, ChildStateEvidence::ObservationUnavailable);
        assert_eq!(
            l.confidence,
            ChildStateConfidence::Unobserved,
            "구할 수 없는 축에 임의 기본값을 넣지 않는다"
        );
    }

    #[test]
    fn recent_output_holds_active() {
        let l = derive_child_state("active", &live_active());
        assert_eq!(l.state, ChildState::Active);
        assert_eq!(l.evidence, ChildStateEvidence::RecentOutput);
    }

    /// `docs/features/child-terminal/index.md` "판정 우선순위" 표 10 행을 **응답에
    /// 실리는 슬러그 그대로** 고정한다. 위의 개별 테스트들은 열거형 변형을 보는데,
    /// 소비자가 실제로 읽는 것은 `as_str()` 문자열이라 그 사상이 어긋나면 문서가
    /// 약속한 조합이 응답에서 재현되지 않는다.
    #[test]
    fn priority_table_rows_match_documented_slugs() {
        let long_output = CHILD_OUTPUT_SILENCE + Duration::from_secs(1);
        let long_hook = CHILD_HOOK_SILENCE + Duration::from_secs(1);
        // (문서 행 번호, registry 상태, 관측, 기대 (state, confidence, evidence))
        let rows: Vec<(u32, &str, ChildObservation, (&str, &str, &str))> = vec![
            (
                1,
                "active",
                ChildObservation {
                    surface_live: false,
                    ..live_active()
                },
                ("exited", "confirmed", "surface_gone"),
            ),
            (
                2,
                "needs_input",
                live_active(),
                ("needs_input", "reported", "hook_needs_input"),
            ),
            (3, "idle", live_active(), ("idle", "reported", "hook_idle")),
            (
                4,
                "active",
                ChildObservation {
                    busy: true,
                    ..live_active()
                },
                ("active", "confirmed", "pty_busy"),
            ),
            (
                5,
                "active",
                ChildObservation {
                    pty_ready: false,
                    ..live_active()
                },
                ("active", "unobserved", "pty_not_started"),
            ),
            (
                6,
                "active",
                ChildObservation {
                    foreground_is_shell: Some(true),
                    ..live_active()
                },
                ("stale", "confirmed", "foreground_is_shell"),
            ),
            (
                7,
                "active",
                ChildObservation {
                    output_silence: None,
                    ..live_active()
                },
                ("active", "unobserved", "observation_unavailable"),
            ),
            (
                8,
                "active",
                live_active(),
                ("active", "heuristic", "recent_output"),
            ),
            (
                9,
                "active",
                ChildObservation {
                    output_silence: Some(long_output),
                    ..live_active()
                },
                ("active", "heuristic", "recent_hook_report"),
            ),
            (
                10,
                "active",
                ChildObservation {
                    output_silence: Some(long_output),
                    hook_silence: Some(long_hook),
                    ..live_active()
                },
                ("stale", "heuristic", "output_and_hook_silent"),
            ),
        ];
        for (row, registry_state, obs, expected) in rows {
            let l = derive_child_state(registry_state, &obs);
            assert_eq!(
                (l.state.as_str(), l.confidence.as_str(), l.evidence.as_str()),
                expected,
                "판정 우선순위표 {row} 행이 문서와 어긋난다"
            );
        }
    }

    #[test]
    fn state_strings_are_backward_compatible() {
        assert_eq!(ChildState::Active.as_str(), "active");
        assert_eq!(ChildState::Idle.as_str(), "idle");
        assert_eq!(ChildState::NeedsInput.as_str(), "needs_input");
        assert_eq!(ChildState::Exited.as_str(), "exited");
        assert_eq!(ChildState::Stale.as_str(), "stale");
    }
}
