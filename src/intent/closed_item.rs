//! 사용자 단축키에서 닫힌 항목을 복원한다.
//! mirror에서는 서버가 PTY와 스크롤백을 소유하므로 복원 요청을 원격으로 전달한다.
//! 이때 로컬 복원 스택은 바꾸지 않는다.
//! 상세 규칙: docs/adr/0023-attach-state-sync-and-forwarding.md.

use super::{DispatchedIntent, Intent};
use crate::core::Core;
use crate::core::CoreState;
use crate::state::AppState;

/// Surface·Tab·Pane을 복원할 워크스페이스가 없으면 먼저 만든다.
/// Workspace 복원은 자체적으로 워크스페이스를 만들므로 제외한다.
fn ensure_workspace_for_restore(core: &mut Core, state: &mut AppState, engine: &mut CoreState) {
    use crate::model::closed_item::ClosedItem;
    let top_needs_workspace = matches!(
        engine.closed_items.list().next(),
        Some(ClosedItem::Surface { .. } | ClosedItem::Tab(_) | ClosedItem::Pane { .. })
    );
    if !top_needs_workspace || !engine.workspaces.is_empty() {
        return;
    }
    match core.create_default_workspace(engine) {
        Ok(idx) => state.active_workspace = idx,
        Err(e) => {
            tracing::warn!("RestoreClosedItem precondition workspace failed: {e}");
        }
    }
}

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    if !matches!(&intent.body, Intent::RestoreClosedItem) {
        return;
    }
    ensure_workspace_for_restore(core, state, engine);
    let target_pane_id = state.focused_pane(engine).map(|p| p.id);
    let domain_intent = crate::core::intent::DomainIntent::RestoreClosedItem {
        target_pane_id,
        scope: crate::core::intent::RestoreScope::Local,
    };
    let events = match core.apply(engine, domain_intent) {
        Ok(e) => e,
        Err(e) => {
            // 사용자 요청임을 전달해야 원격 복원 결과에 맞춰 포커스도 옮긴다.
            crate::core::mark_last_forward_user_triggered(engine, &e, &intent.origin);
            crate::intent::report_apply_error(
                state,
                engine,
                &intent.origin,
                "RestoreClosedItem",
                &e,
            );
            return;
        }
    };
    for ev in events {
        if let crate::core::intent::CoreEvent::ClosedItemRestored { restored, kind } = ev
            && restored
        {
            crate::app::dispatch_domain::cascade_closed_item_restored(
                state,
                engine,
                &intent.origin,
                kind,
            );
        }
    }
}

// 헤드리스의 복원 후 처리는 포커스를 옮기지 않으므로 GUI 조합에서 검사한다.
#[cfg(all(test, feature = "gui"))]
#[path = "closed_item_origin_tests.rs"]
mod origin_tests;
