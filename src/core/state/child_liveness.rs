//! hook 보고와 surface·PTY 관측을 합쳐 자식 상태를 만든다. 입력을 주입하지 않는다.
//! stale은 출력용 추정값이며 정지·긴 추론·무출력 명령을 구별하지 못한다.
//! confidence는 분류 이름이다. surface 부재나 전경 셸 관측만으로 프로세스 종료를 증명하지 않는다.

use std::collections::HashSet;
use std::time::Duration;

use super::CoreState;

/// 짧은 출력 공백으로 stale을 만들지 않기 위한 무출력 기준. 작업별 정상 소요 시간을 보장하지 않는다.
pub const CHILD_OUTPUT_SILENCE: Duration = Duration::from_secs(120);

/// hook은 상태가 바뀔 때 보고하므로 무출력 기준보다 긴 간격을 허용한다.
pub const CHILD_HOOK_SILENCE: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildState {
    /// 라이브 surface 트리에 없다. OS 프로세스의 종료를 직접 확인한 값은 아니다.
    Exited,
    /// hook의 입력 대기 보고를 유지한 값.
    NeedsInput,
    /// hook의 idle 보고를 유지한 값.
    Idle,
    /// busy·최근 보고·관측 부족 등의 이유로 active를 유지한 값.
    Active,
    /// surface는 있지만 전경이 셸이거나 출력·hook 보고가 오래 없었다.
    /// adopted terminal은 일반 셸일 수도 있으므로 에이전트 종료로 단정하지 않는다.
    Stale,
}

impl ChildState {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildStateEvidence {
    SurfaceGone,
    HookNeedsInput,
    HookIdle,
    /// busy 캐시가 true다. 출력 외에 터미널 락 경합으로도 true가 될 수 있다.
    PtyBusy,
    /// 로컬 TerminalStore에 없다. deferred와 mirror를 이 값만으로 구별하지 않는다.
    PtyNotStarted,
    /// 캐시된 전경 이름이 알려진 셸 이름이다.
    ForegroundIsShell,
    ObservationUnavailable,
    RecentOutput,
    RecentHookReport,
    OutputAndHookSilent,
}

impl ChildStateEvidence {
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

/// 응답에 쓰는 판정 근거의 분류. 프로세스 생존·종료의 직접 검증 결과는 아니다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChildStateConfidence {
    /// surface 부재, busy 양성 또는 전경 셸 분기에 붙이는 분류.
    Confirmed,
    /// hook 보고를 우선한 값. 보고 내용의 진위나 최신성을 검증하지는 않는다.
    Reported,
    /// 시간 기준 추정. SIGSTOP·긴 추론·무출력 명령을 구별하지 못한다.
    Heuristic,
    /// 필요한 관측이 없어 active를 반환한 값.
    Unobserved,
}

impl ChildStateConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Confirmed => "confirmed",
            Self::Reported => "reported",
            Self::Heuristic => "heuristic",
            Self::Unobserved => "unobserved",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildLiveness {
    pub state: ChildState,
    pub evidence: ChildStateEvidence,
    pub confidence: ChildStateConfidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChildObservation {
    /// 라이브 surface 트리에 있는지 여부.
    pub surface_live: bool,
    /// 로컬 TerminalStore에 있는지 여부.
    pub pty_ready: bool,
    /// 로컬 busy 폴링 또는 mirror의 원격 busy 캐시.
    pub busy: bool,
    /// 캐시된 전경 이름이 알려진 셸인지 여부. 미해석이면 None.
    pub foreground_is_shell: Option<bool>,
    /// 마지막 로컬 Terminal 출력 후 경과. Terminal이 없으면 None.
    pub output_silence: Option<Duration>,
    /// 마지막 상태 보고 후 경과. 기준 시각이 없으면 None.
    pub hook_silence: Option<Duration>,
}

/// surface 부재를 먼저 처리하고 hook의 needs_input·idle을 우선한다.
/// 나머지는 busy, 로컬 Terminal 유무, 전경 셸, 출력·hook 경과 순서로 판단한다.
/// hook 보고를 관측이 덮어쓰지 않는 것은 정책이며 보고가 항상 정확하다는 뜻은 아니다.
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
    // 로컬 Terminal이 없는 경우를 무출력 시간만으로 stale로 판단하지 않는다.
    if !obs.pty_ready {
        return out(ChildState::Active, E::PtyNotStarted, C::Unobserved);
    }
    if obs.foreground_is_shell == Some(true) {
        return out(ChildState::Stale, E::ForegroundIsShell, C::Confirmed);
    }
    let Some(output_silence) = obs.output_silence else {
        return out(ChildState::Active, E::ObservationUnavailable, C::Unobserved);
    };

    if output_silence < CHILD_OUTPUT_SILENCE {
        return out(ChildState::Active, E::RecentOutput, C::Heuristic);
    }
    // 보고 시각이 없으면 hook도 오래 조용했던 것으로 취급한다.
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
    /// 라이브 집합과 전경 이름 캐시를 재사용해 자식마다 전체 트리·프로세스를 다시 조회하지 않는다.
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

    /// 목록 조회와 단건 조회가 같은 상태 판정을 사용한다.
    pub fn child_liveness_with_live(
        &self,
        child_surface: u32,
        live: &HashSet<u32>,
    ) -> ChildLiveness {
        let obs = self.observe_child(child_surface, live);
        derive_child_state(self.child_terminals.state_of(child_surface), &obs)
    }

    pub fn child_liveness(&self, child_surface: u32) -> ChildLiveness {
        let live = self.live_surface_ids();
        self.child_liveness_with_live(child_surface, &live)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "보고 시각이 없으면 출력 경과 기준만으로 stale을 판단한다"
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
            "전경 셸 분기는 confirmed로 분류한다"
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
            "출력 경과를 모르면 unobserved로 분류한다"
        );
    }

    #[test]
    fn recent_output_holds_active() {
        let l = derive_child_state("active", &live_active());
        assert_eq!(l.state, ChildState::Active);
        assert_eq!(l.evidence, ChildStateEvidence::RecentOutput);
    }

    /// 대표 입력 10개의 응답 문자열 조합을 고정한다. 실제 문서 내용을 파싱해 비교하지는 않는다.
    #[test]
    fn priority_table_rows_match_documented_slugs() {
        let long_output = CHILD_OUTPUT_SILENCE + Duration::from_secs(1);
        let long_hook = CHILD_HOOK_SILENCE + Duration::from_secs(1);
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
                "판정 입력 {row}의 응답 조합이 기대값과 다르다"
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
