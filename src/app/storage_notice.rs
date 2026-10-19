//! 부팅 때 열지 못한 로컬 저장소를 경고 알림으로 남긴다.
//! 이 저장소는 터미널 작업과 레이아웃 복원에 쓰이지 않으므로 앱은 멈추지 않고 계속한다.
//! 알림 저장소는 첫 engine이 준비된 뒤에 생기므로 호출자는 그 시점에 한 번 부른다.

use crate::notification::NotificationLevel;
use crate::runtime::engine_access::EngineMut;

/// state.db를 열지 못했으면 원인과 영향(최근 목록·튜토리얼 진행 미저장)을 경고 알림으로 남긴다.
#[cfg(feature = "gui")]
pub(crate) fn report_state_db_unavailable(
    engine: &mut EngineMut<'_>,
    error: &crate::db::DbInitError,
) -> bool {
    let (key, args) = error.user_message_i18n();
    let cause = match args.len() {
        0 => crate::i18n::t(key).to_string(),
        1 => crate::i18n::t_fmt(key, &args[0]),
        _ => crate::i18n::t_fmt2(key, &args[0], &args[1]),
    };
    engine.push_host_notification(
        NotificationLevel::Warning,
        crate::i18n::t("storage_notice.state_db_title").to_string(),
        crate::i18n::t_fmt("storage_notice.state_db_body", &cause),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 알림 출처로 쓸 기본 workspace가 있는 engine.
    fn engine() -> crate::runtime::engine_session::EngineSession {
        crate::state::tests::test_state().1
    }

    fn warnings(engine: &EngineMut<'_>) -> Vec<(String, String)> {
        engine
            .live
            .notifications
            .all()
            .filter(|n| n.level == NotificationLevel::Warning)
            .map(|n| (n.title.clone(), n.body.clone()))
            .collect()
    }

    #[cfg(feature = "gui")]
    #[test]
    fn state_db_failure_leaves_one_warning_with_its_cause() {
        let mut session = engine();
        let mut engine = session.borrow_mut();
        let error = crate::db::DbInitError::Corrupt(std::path::PathBuf::from("/home/x/state.db"));
        assert!(report_state_db_unavailable(&mut engine, &error));
        let found = warnings(&engine);
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(found[0].1.contains("/home/x/state.db"));
    }
}
