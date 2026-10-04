//! 사용자 요청 실패만 토스트로 표시하는지 검사한다.
//! 토스트와 attach 클라이언트가 필요한 GUI 전용 시험이다.

use super::*;

fn fixture() -> (
    crate::app::services::AppServices,
    crate::state::RequestContext,
    crate::runtime::engine_session::EngineSession,
) {
    let (state, engine_session) = crate::state::tests::test_state();
    (
        crate::ipc::handler::cli_entry_tests::test_core(),
        state,
        engine_session,
    )
}

fn agent() -> IntentOrigin {
    IntentOrigin::Agent {
        source: AgentSource::Ipc,
    }
}

fn user() -> IntentOrigin {
    IntentOrigin::User {
        source: UserSource::Shortcut("test"),
    }
}

#[test]
fn a_withdrawn_kind_refusal_toasts_only_for_the_user() {
    let (_core, mut state, _engine_session) = fixture();
    let err = anyhow::Error::new(crate::runtime::surface_registry::SurfaceKindWithdrawn {
        kind: "markdown".to_string(),
        plugin_id: "com.tasty.markdown".to_string(),
    });
    report_apply_error(&mut state, &agent(), "t", &err);
    assert_eq!(
        state.toasts.len(),
        0,
        "에이전트 요청 거절은 토스트로 표시하지 않는다"
    );

    report_apply_error(&mut state, &user(), "t", &err);
    assert_eq!(
        state.toasts.len(),
        1,
        "사용자 요청 거절은 사유를 토스트로 표시한다"
    );
}
