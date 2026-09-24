//! 복원 핸들러가 요청의 origin을 후속 처리에 그대로 전달하는지 검사한다.
//! app/dispatch_domain_restore_tests.rs의 직접 호출과 달리 실제 핸들러를 거친다.

use super::handle;
use crate::intent::Intent;
use crate::state::WorkspaceCloseOrigin;

fn active_after_restoring_a_closed_workspace(intent: crate::intent::DispatchedIntent) -> usize {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let event = crate::core::apply_create_workspace_inner(
        &mut engine,
        crate::core::WorkspaceCreationParams::terminal(),
    )
    .expect("second workspace");
    let crate::core::intent::CoreEvent::WorkspaceCreated { index, .. } = event else {
        panic!("apply_create_workspace_inner가 WorkspaceCreated를 반환해야 한다");
    };
    assert_eq!(index, 1);
    assert!(state.close_workspace_at(&mut engine, 1, WorkspaceCloseOrigin::User));
    state.active_workspace = 0;
    assert_eq!(engine.workspaces.len(), 1);
    assert_eq!(
        engine.closed_items.len(),
        1,
        "사용자가 닫은 워크스페이스는 복원 스택에 기록한다"
    );

    let mut core = crate::adapters::ipc::handler::cli_entry_tests::test_core();
    handle(&mut core, &mut state, &mut engine, &intent);
    assert_eq!(
        engine.workspaces.len(),
        2,
        "복원이 워크스페이스를 되살려야 한다"
    );
    assert_eq!(engine.closed_items.len(), 0);
    state.active_workspace
}

#[test]
fn a_shortcut_restore_moves_the_active_workspace_to_the_restored_one() {
    let intent = Intent::RestoreClosedItem.from_user_shortcut("restore_closed");
    assert_eq!(active_after_restoring_a_closed_workspace(intent), 1);
}

#[test]
fn an_agent_restore_leaves_the_active_workspace_alone() {
    let intent = Intent::RestoreClosedItem.from_agent_ipc();
    assert_eq!(active_after_restoring_a_closed_workspace(intent), 0);
}
