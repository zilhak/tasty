//! Lua 실행 결과를 시작 경로로 돌려준다. 단축키 실행은 원 창의 토스트, 자동실행은 알림 채널이 받는다.
//! 제출할 때 ticket과 시작 경로를 장부에 남기고, 메인 루프가 워커의 결과를 꺼내 짝을 맞춘다.
//! 성공한 실행은 아무것도 보이지 않는다. 같은 자동실행 알림의 반복은 `system_notice`의 반복 창이 합친다.

use std::collections::BTreeMap;

use tasty_lua::{RunFailure, RunRejected, RunTicket};

use crate::system_notice::SystemNotice;

/// 자동실행 알림의 `notification.created` source.
pub(crate) const NOTICE_SOURCE: &str = "lua.autofire";

/// 결과를 잃은 항목이 쌓이지 않게 하는 상한. 넘으면 가장 오래된 ticket을 버린다.
const LEDGER_LIMIT: usize = 512;

/// 실행을 요청한 경로.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RunOrigin {
    /// 사용자 단축키나 변경 확인 popup의 승인. 결과는 요청한 창에만 보인다.
    #[cfg(feature = "gui")]
    Shortcut {
        window: winit::window::WindowId,
        name: String,
    },
    Autofire {
        event: String,
        name: String,
    },
}

/// 결과를 기다리는 실행의 시작 경로.
#[derive(Default)]
pub(crate) struct LuaRuns {
    pending: BTreeMap<RunTicket, RunOrigin>,
}

impl LuaRuns {
    pub(crate) fn record(&mut self, ticket: RunTicket, origin: RunOrigin) {
        self.pending.insert(ticket, origin);
        while self.pending.len() > LEDGER_LIMIT {
            if let Some((lost, _)) = self.pending.pop_first() {
                tracing::warn!(target: "tasty_lua", ticket = lost, "lua run result never arrived; forgetting it");
            }
        }
    }

    pub(crate) fn take(&mut self, ticket: RunTicket) -> Option<RunOrigin> {
        self.pending.remove(&ticket)
    }

    /// 실패 결과를 시작 경로별로 나눈다. 장부에 없는 ticket(이미 잊은 결과)은 버린다.
    pub(crate) fn settle(&mut self, outcomes: Vec<tasty_lua::RunOutcome>) -> Settled {
        let mut settled = Settled::default();
        for outcome in outcomes {
            let Some(origin) = self.take(outcome.ticket) else {
                tracing::debug!(target: "tasty_lua", ticket = outcome.ticket, "lua run result without an origin");
                continue;
            };
            let Err(failure) = outcome.result else {
                continue;
            };
            match origin {
                RunOrigin::Autofire { event, name } => settled.notices.push(autofire_notice(
                    &event,
                    &name,
                    AutofireProblem::Failed(&failure),
                )),
                #[cfg(feature = "gui")]
                shortcut @ RunOrigin::Shortcut { .. } => {
                    settled.for_views.push((shortcut, failure))
                }
            }
        }
        settled
    }
}

/// 시작 경로별로 나눈 실패 결과.
#[derive(Default)]
pub(crate) struct Settled {
    /// 자동실행 실패. 호출자가 알림 채널에 넣는다.
    pub notices: Vec<SystemNotice>,
    /// 단축키 실행 실패. 호출자가 원 창의 토스트로 보인다.
    #[cfg_attr(
        not(any(feature = "gui", test)),
        expect(dead_code, reason = "headless has no shortcut runs to show")
    )]
    pub for_views: Vec<(RunOrigin, RunFailure)>,
}

/// 자동실행이 실행되지 못했거나 실패한 이유.
pub(crate) enum AutofireProblem<'a> {
    ReadFailed(&'a str),
    /// 등록 뒤 파일이 바뀌어 TOFU 규칙으로 막았다.
    Changed,
    Rejected(RunRejected),
    Failed(&'a RunFailure),
}

/// 자동실행 경고 알림. 경고 로그는 호출자가 따로 남긴다.
pub(crate) fn autofire_notice(
    event: &str,
    script: &str,
    problem: AutofireProblem<'_>,
) -> SystemNotice {
    use crate::i18n::{t, t_args};
    let (title, body) = match problem {
        AutofireProblem::ReadFailed(error) => (
            "lua_run.autofire_failed_title",
            t_args("lua_run.autofire_read_failed", &[script, event, error]),
        ),
        AutofireProblem::Changed => (
            "lua_run.autofire_blocked_title",
            t_args("lua_run.autofire_changed", &[script, event]),
        ),
        AutofireProblem::Rejected(RunRejected::QueueFull) => (
            "lua_run.autofire_failed_title",
            t_args("lua_run.autofire_queue_full", &[script, event]),
        ),
        AutofireProblem::Rejected(RunRejected::WorkerGone) => (
            "lua_run.autofire_failed_title",
            t_args("lua_run.autofire_unavailable", &[script, event]),
        ),
        AutofireProblem::Failed(RunFailure::Error(reason)) => (
            "lua_run.autofire_failed_title",
            t_args("lua_run.autofire_error", &[script, event, reason]),
        ),
        AutofireProblem::Failed(RunFailure::TimedOut) => (
            "lua_run.autofire_failed_title",
            t_args("lua_run.autofire_timed_out", &[script, event]),
        ),
    };
    SystemNotice::warning(NOTICE_SOURCE, t(title).to_string(), body)
}

/// 재진입 억제로 이벤트의 자동실행을 건너뛰었다는 경고 알림.
pub(crate) fn autofire_suppressed_notice(event: &str) -> SystemNotice {
    SystemNotice::warning(
        NOTICE_SOURCE,
        crate::i18n::t("lua_run.autofire_suppressed_title").to_string(),
        crate::i18n::t_fmt("lua_run.autofire_suppressed", event),
    )
}

/// 단축키 실행 실패 토스트 문구. 길이를 모르는 사유는 가운데를 줄여 상한에 맞춘다.
#[cfg(feature = "gui")]
pub(crate) fn shortcut_failure_text(name: &str, failure: &ShortcutFailure<'_>) -> String {
    use crate::i18n::{fit_fragment, t_fmt, t_fmt2};
    match failure {
        ShortcutFailure::NotRegistered => t_fmt("lua_run.toast.not_registered", name),
        ShortcutFailure::ReadQueueFull => t_fmt("lua_run.toast.read_queue_full", name),
        ShortcutFailure::ReadFailed(error) => {
            fit_fragment(error, |e| t_fmt2("lua_run.toast.read_failed", name, e))
        }
        ShortcutFailure::Rejected(RunRejected::QueueFull) => {
            t_fmt("lua_run.toast.queue_full", name)
        }
        ShortcutFailure::Rejected(RunRejected::WorkerGone) => {
            t_fmt("lua_run.toast.unavailable", name)
        }
        ShortcutFailure::Run(RunFailure::Error(reason)) => {
            fit_fragment(reason, |r| t_fmt2("lua_run.toast.failed", name, r))
        }
        ShortcutFailure::Run(RunFailure::TimedOut) => t_fmt("lua_run.toast.timed_out", name),
    }
}

/// 단축키 실행이 실패한 단계.
#[cfg(feature = "gui")]
pub(crate) enum ShortcutFailure<'a> {
    NotRegistered,
    ReadQueueFull,
    ReadFailed(&'a str),
    Rejected(RunRejected),
    Run(&'a RunFailure),
}

#[cfg(test)]
mod tests {
    use tasty_lua::RunOutcome;

    use super::*;

    fn autofire(name: &str) -> RunOrigin {
        RunOrigin::Autofire {
            event: "workspace.create.post".into(),
            name: name.into(),
        }
    }

    #[test]
    fn a_result_is_matched_once_and_success_shows_nothing() {
        let mut runs = LuaRuns::default();
        runs.record(1, autofire("ok.lua"));
        let settled = runs.settle(vec![RunOutcome {
            ticket: 1,
            result: Ok(()),
        }]);
        assert!(settled.notices.is_empty() && settled.for_views.is_empty());
        assert!(
            runs.take(1).is_none(),
            "결과를 받은 ticket은 장부에서 지운다"
        );
    }

    #[test]
    fn an_autofire_failure_becomes_a_lua_autofire_warning() {
        let mut runs = LuaRuns::default();
        runs.record(3, autofire("b.lua"));
        runs.record(4, autofire("slow.lua"));
        let settled = runs.settle(vec![
            RunOutcome {
                ticket: 3,
                result: Err(RunFailure::Error("boom".into())),
            },
            RunOutcome {
                ticket: 4,
                result: Err(RunFailure::TimedOut),
            },
        ]);
        assert!(
            settled.for_views.is_empty(),
            "자동실행 결과는 토스트가 되지 않는다"
        );
        assert_eq!(settled.notices.len(), 2);
        for n in &settled.notices {
            assert_eq!(n.level, crate::notification::NotificationLevel::Warning);
            assert_eq!(n.source, NOTICE_SOURCE);
            assert_eq!(n.surface_id, 0);
        }
        assert!(settled.notices[0].body.contains("b.lua"));
        assert!(settled.notices[0].body.contains("boom"));
        assert!(settled.notices[1].body.contains("slow.lua"));
    }

    #[test]
    fn the_ledger_forgets_the_oldest_past_the_limit() {
        let mut runs = LuaRuns::default();
        for ticket in 0..=LEDGER_LIMIT as u64 {
            runs.record(ticket, autofire("s"));
        }
        assert!(runs.take(0).is_none());
        assert!(runs.take(LEDGER_LIMIT as u64).is_some());
    }

    #[cfg(feature = "gui")]
    #[test]
    fn shortcut_failures_go_back_to_the_caller_not_to_the_notice_channel() {
        let mut runs = LuaRuns::default();
        let window = winit::window::WindowId::dummy();
        runs.record(
            7,
            RunOrigin::Shortcut {
                window,
                name: "a.lua".into(),
            },
        );
        let settled = runs.settle(vec![RunOutcome {
            ticket: 7,
            result: Err(RunFailure::Error("boom".into())),
        }]);
        assert!(
            settled.notices.is_empty(),
            "단축키 결과는 알림이 되지 않는다"
        );
        assert_eq!(
            settled.for_views,
            vec![(
                RunOrigin::Shortcut {
                    window,
                    name: "a.lua".into()
                },
                RunFailure::Error("boom".into())
            )]
        );
    }
}
