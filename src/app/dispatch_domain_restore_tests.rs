//! 닫은 항목 복원은 사용자 origin에서만 포커스를 옮겨야 한다.

use super::cascade_closed_item_restored;
use crate::core::intent::RestoredKind;
use crate::intent::{AgentSource, IntentOrigin, UserSource};

fn restored_workspace_index(origin: &IntentOrigin) -> usize {
    let (mut state, mut engine) = crate::state::tests::test_state();
    assert_eq!(state.active_workspace, 0);
    cascade_closed_item_restored(
        &mut state,
        &mut engine,
        origin,
        RestoredKind::Workspace { new_ws_index: 1 },
    );
    state.active_workspace
}

#[test]
fn a_user_restore_moves_the_active_workspace_to_the_restored_one() {
    let user = IntentOrigin::User {
        source: UserSource::Shortcut("restore_closed"),
    };
    assert_eq!(restored_workspace_index(&user), 1);
}

#[test]
fn a_restore_the_user_did_not_raise_leaves_the_active_workspace_alone() {
    let agent = IntentOrigin::Agent {
        source: AgentSource::Ipc,
    };
    assert_eq!(restored_workspace_index(&agent), 0, "agent");
    assert_eq!(restored_workspace_index(&IntentOrigin::System), 0, "system");
}
