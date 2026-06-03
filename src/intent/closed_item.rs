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

pub fn handle(
    core: &mut Core,
    state: &mut AppState,
    engine: &mut CoreState,
    intent: &DispatchedIntent,
) {
    if !matches!(&intent.body, Intent::RestoreClosedItem) {
        return;
    }
    // Surface / Tab 복원에 대비해 workspace 확보 (closed top peek).
    // `list()` 는 newest-first 이므로 next() 가 stack top (pop 대상).
    let top_needs_workspace = {
        use crate::model::closed_item::ClosedItem;
        matches!(
            engine.closed_items.list().next(),
            Some(ClosedItem::Surface { .. } | ClosedItem::Tab(_))
        )
    };
    if top_needs_workspace && engine.workspaces.is_empty() {
        match core.create_default_workspace(engine) {
            Ok(idx) => state.active_workspace = idx,
            Err(e) => {
                tracing::warn!("RestoreClosedItem precondition workspace failed: {e}");
            }
        }
    }
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
