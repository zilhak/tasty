//! 주기 저장이 멈췄거나 계속 실패할 때 남길 경고 알림의 판정. 한 번의 실패는 다음 저장 주기가
//! 다시 시도하므로 알리지 않는다. 알린 원인은 그 저장이 다시 성공할 때까지 다시 알리지 않는다.
use super::*;
use std::hash::Hash;

/// 같은 저장이 이 횟수만큼 성공 없이 이어서 실패하면 경고한다. 실패한 저장은 다음 저장 주기에 다시
/// 시도하므로, 일시적인 실패는 이 횟수에 이르기 전에 성공한다.
pub(super) const REPEATED_FAILURES: u32 = 3;

#[derive(Default)]
struct Streak {
    failures: u32,
    failure_reported: bool,
    oversize_reported: bool,
}

/// 저장 대상(엔진 또는 View stream)별 연속 실패와 알림 여부.
pub(super) struct FailureStreaks<K> {
    streaks: HashMap<K, Streak>,
}

impl<K> Default for FailureStreaks<K> {
    fn default() -> Self {
        Self {
            streaks: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash> FailureStreaks<K> {
    /// 실패 하나를 센다. 기준에 처음 이른 실패면 연속 실패 횟수를 돌려준다.
    pub(super) fn failed(&mut self, key: K) -> Option<u32> {
        let streak = self.streaks.entry(key).or_default();
        streak.failures = streak.failures.saturating_add(1);
        if streak.failure_reported || streak.failures < REPEATED_FAILURES {
            return None;
        }
        streak.failure_reported = true;
        Some(streak.failures)
    }

    /// 상한 초과 거절. 다음 변경 전까지 다시 시도하지 않으므로 처음 거절할 때 바로 알린다.
    pub(super) fn oversized(&mut self, key: K) -> bool {
        let streak = self.streaks.entry(key).or_default();
        !std::mem::replace(&mut streak.oversize_reported, true)
    }

    pub(super) fn succeeded(&mut self, key: &K) {
        self.streaks.remove(key);
    }
}

impl JournalApplication {
    /// capture 실패는 dirty를 남겨 다음 저장 주기에 다시 시도한다. 기준에 이르면 경고한다.
    pub(super) fn capture_failed(&mut self, session: &EngineSession, error: &str) {
        tracing::warn!("surface capture failed; dirty state retained: {error}");
        if let Some(failures) = self.save_failures.failed(session.id) {
            self.notices
                .push(capture_failing_notice(session, failures, error));
        }
    }
}

/// 알림 본문에 적을 엔진의 workspace 이름. capture는 엔진의 workspace 전체를 한 번에 저장한다.
pub(super) fn workspace_names(session: &EngineSession) -> String {
    session
        .core_state
        .local_workspaces()
        .iter()
        .map(|workspace| format!("\"{}\"", workspace.name))
        .collect::<Vec<_>>()
        .join(", ")
}

fn mebibytes(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

pub(super) fn oversized_notice(
    session: &EngineSession,
    bytes: u64,
    limit: usize,
) -> notices::RecoveryNotice {
    notices::RecoveryNotice::warning(
        Some(session.id),
        crate::i18n::t("recovery.save_oversized_title").to_owned(),
        crate::i18n::t_args(
            "recovery.save_oversized_body",
            &[
                &workspace_names(session),
                &mebibytes(bytes),
                &mebibytes(limit as u64),
            ],
        ),
    )
}

pub(super) fn capture_failing_notice(
    session: &EngineSession,
    failures: u32,
    cause: &str,
) -> notices::RecoveryNotice {
    notices::RecoveryNotice::warning(
        Some(session.id),
        crate::i18n::t("recovery.save_failing_title").to_owned(),
        crate::i18n::t_args(
            "recovery.save_failing_body",
            &[&workspace_names(session), &failures.to_string(), cause],
        ),
    )
}

#[cfg(feature = "gui")]
pub(super) fn view_failing_notice(
    session: &EngineSession,
    failures: u32,
    cause: &str,
) -> notices::RecoveryNotice {
    notices::RecoveryNotice::warning(
        Some(session.id),
        crate::i18n::t("recovery.save_failing_title").to_owned(),
        crate::i18n::t_args(
            "recovery.view_save_failing_body",
            &[&workspace_names(session), &failures.to_string(), cause],
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_failures_report_once_until_a_success() {
        let mut streaks = FailureStreaks::default();
        for _ in 1..REPEATED_FAILURES {
            assert_eq!(streaks.failed(7), None);
        }
        assert_eq!(streaks.failed(7), Some(REPEATED_FAILURES));
        assert_eq!(streaks.failed(7), None, "same cause is not reported again");
        streaks.succeeded(&7);
        for _ in 1..REPEATED_FAILURES {
            assert_eq!(streaks.failed(7), None);
        }
        assert_eq!(streaks.failed(7), Some(REPEATED_FAILURES));
    }

    #[test]
    fn a_failure_followed_by_a_success_is_never_reported() {
        let mut streaks = FailureStreaks::default();
        for _ in 0..10 {
            assert_eq!(streaks.failed(1), None);
            streaks.succeeded(&1);
        }
    }

    #[test]
    fn oversize_is_reported_once_until_a_success() {
        let mut streaks = FailureStreaks::default();
        assert!(streaks.oversized(3));
        assert!(!streaks.oversized(3));
        streaks.succeeded(&3);
        assert!(streaks.oversized(3));
    }
}
