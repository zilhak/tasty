//! Mirror selection mapping; no transport or execution owner is required.

#[cfg(test)]
mod layout_tests;
#[cfg(test)]
mod tests;

use crate::ipc::stream::StructuralOp;
use crate::model::Workspace;
use std::collections::HashMap;
use tasty_remote::client_session::PendingOpFocus;
pub(super) fn pending_op_focus_for(
    op: &StructuralOp,
    close_focus_candidates: &[u32],
    remote_to_local: &HashMap<u32, u32>,
) -> Option<PendingOpFocus> {
    match op {
        StructuralOp::NewTab { .. }
        | StructuralOp::SplitSurface { .. }
        | StructuralOp::SplitPane { .. }
        | StructuralOp::RestoreClosedItem { .. } => Some(PendingOpFocus::NewResource),
        StructuralOp::CloseSurface { .. }
        | StructuralOp::CloseTab { .. }
        | StructuralOp::ClosePane { .. } => {
            let candidates: Vec<u32> = close_focus_candidates
                .iter()
                .filter_map(|local_sid| {
                    remote_to_local
                        .iter()
                        .find(|&(_, l)| l == local_sid)
                        .map(|(&r, _)| r)
                })
                .collect();
            if candidates.is_empty() {
                None
            } else {
                Some(PendingOpFocus::Close { candidates })
            }
        }
        _ => None,
    }
}

pub(super) fn restore_focus_after_delta(
    navigation: &mut crate::state::navigation::NavigationState,
    ws: &mut Workspace,
    old_focused_remote: Option<u32>,
    remote_to_local: &HashMap<u32, u32>,
) -> bool {
    let Some(remote_sid) = old_focused_remote else {
        return false;
    };
    let Some(&new_local_sid) = remote_to_local.get(&remote_sid) else {
        return false;
    };
    set_focus_to_surface(navigation, ws, new_local_sid)
}

pub(super) fn set_focus_to_surface(
    navigation: &mut crate::state::navigation::NavigationState,
    ws: &Workspace,
    local_sid: u32,
) -> bool {
    let Some((pane_id, tab_id)) = find_pane_and_tab_for_surface(ws, local_sid) else {
        return false;
    };
    navigation.select_pane(ws, pane_id);
    if let Some(pane) = ws.pane_layout().find_pane(pane_id)
        && let Some(tab_index) = pane.tabs.iter().position(|t| t.id == tab_id)
    {
        navigation.select_tab(pane, tab_id);
        navigation.select_surface(&pane.tabs[tab_index], local_sid);
        true
    } else {
        false
    }
}

/// Surviving remote surface IDs restore the active branch after a subtree moves.
pub(super) fn capture_focused_remote(
    navigation: &crate::state::navigation::NavigationState,
    ws: &Workspace,
    remote_to_local: &HashMap<u32, u32>,
) -> Option<u32> {
    let pane = ws.pane_layout().find_pane(navigation.pane_id(ws)?)?;
    let tab = pane.tabs.get(navigation.tab_index(pane))?;
    let local_sid = navigation.surface_id(tab)?;
    remote_to_local
        .iter()
        .find(|&(_, &l)| l == local_sid)
        .map(|(&r, _)| r)
}

/// 아직 engine에 넣지 않은 새 workspace에서도 찾을 수 있도록 범위를 workspace 하나로 제한한다.
fn find_pane_and_tab_for_surface(ws: &Workspace, surface_id: u32) -> Option<(u32, u32)> {
    for pane_id in ws.pane_layout().all_pane_ids() {
        let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
            continue;
        };
        for tab in &pane.tabs {
            if tab.contains_surface(surface_id) {
                return Some((pane_id, tab.id));
            }
        }
    }
    None
}
