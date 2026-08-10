//! 닫힌 항목 (closed_items) 복원 Intent 핸들러.
//!
//! 정책:
//! - **RestoreClosedItem**: Ctrl+Shift+T 등 사용자 단축키 전용. target_pane_id
//!   는 호출 시점의 focused pane (있으면). closed 스택 top 이 Surface/Tab
//!   인 상태에서 workspaces 가 비어있으면 사전에 `ensure_workspace_exists`
//!   처리해 add_workspace 부수효과를 정상화.

use super::{DispatchedIntent, Intent};
use crate::core::Core;
use crate::core::CoreState;
use crate::state::AppState;

/// Surface / Tab / Pane 복원에 대비해 workspace 확보 (closed top peek). `list()`
/// 는 newest-first 이므로 next() 가 stack top (pop 대상). Workspace 복원은 새
/// workspace 를 자체적으로 만들므로 이 사전 확보가 불필요.
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
    let domain_intent = crate::core::intent::DomainIntent::RestoreClosedItem { target_pane_id };
    let events = match core.apply(engine, domain_intent) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("RestoreClosedItem failed: {e}");
            return;
        }
    };
    for ev in events {
        if let crate::core::intent::CoreEvent::ClosedItemRestored { restored, kind } = ev
            && restored
        {
            crate::app::dispatch_domain::cascade_closed_item_restored(state, engine, kind);
        }
    }
}
