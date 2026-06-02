use serde_json::json;

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

pub(crate) fn handle_surface_list(
    state: &AppState,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let mut surfaces = Vec::new();
    for ws in &engine.workspaces {
        for &pane_id in &ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                    collect_tab_surface_info(
                        state,
                        engine,
                        tab,
                        pane_id,
                        ws.id,
                        tab_idx,
                        &mut surfaces,
                    );
                }
            }
        }
    }
    JsonRpcResponse::success(id, json!(surfaces))
}

fn collect_tab_surface_info(
    state: &AppState,
    engine: &crate::core::CoreState,
    tab: &crate::model::Tab,
    pane_id: u32,
    workspace_id: u32,
    tab_idx: usize,
    out: &mut Vec<serde_json::Value>,
) {
    if tab.is_split() {
        // Split tab: iterate through the layout
        collect_surface_layout_info(
            state,
            engine,
            tab.layout(),
            pane_id,
            workspace_id,
            tab_idx,
            out,
        );
    } else {
        // Single surface tab
        let surface = tab.surface();
        if let Some(node) = surface
            .as_any()
            .downcast_ref::<crate::model::TerminalSurface>()
        {
            let t = engine.terminals.get(node.id);
            let mut entry = json!({
                "id": node.id,
                "pane_id": pane_id,
                "workspace_id": workspace_id,
                "tab_index": tab_idx,
                "type": "Terminal",
                "cols": t.map(|x| x.cols()).unwrap_or(0),
                "rows": t.map(|x| x.rows()).unwrap_or(0),
                "busy": engine.is_surface_busy(node.id),
                "pty_ready": engine.terminals.contains(node.id),
            });
            if let Some(fg) = t.and_then(|x| x.foreground_process_info()) {
                entry["foreground_process"] = json!(fg.name);
                entry["foreground_pid"] = json!(fg.pid);
            }
            out.push(entry);
        } else if let Some(id) = surface.surface_id() {
            // Non-terminal surfaces (Markdown, Explorer, Html, Empty)
            // EmptySurface placeholders backing a deferred terminal still
            // expose `type: "Terminal"` so agents can target them like any
            // other terminal — they just report `pty_ready: false` until the
            // PTY is spawned (auto on send, manual via `tasty wake`).
            let deferred = tab.is_surface_deferred(id);
            let mut entry = json!({
                "id": id,
                "pane_id": pane_id,
                "workspace_id": workspace_id,
                "tab_index": tab_idx,
                "type": if deferred { "Terminal" } else { surface.type_name() },
                "busy": false,
            });
            if deferred {
                entry["pty_ready"] = json!(false);
            }
            out.push(entry);
        }
    }
}

fn collect_surface_layout_info(
    state: &AppState,
    engine: &crate::core::CoreState,
    layout: &crate::model::SurfaceLayout,
    pane_id: u32,
    workspace_id: u32,
    tab_idx: usize,
    out: &mut Vec<serde_json::Value>,
) {
    match layout {
        crate::model::SurfaceLayout::Leaf(surface) => {
            let id = surface.surface_id().unwrap_or(0);
            let mut entry = json!({
                "id": id,
                "pane_id": pane_id,
                "workspace_id": workspace_id,
                "tab_index": tab_idx,
                "type": surface.type_name(),
                "busy": engine.is_surface_busy(id),
            });
            if let Some(terminal) = engine.terminals.get(id) {
                entry["cols"] = json!(terminal.cols());
                entry["rows"] = json!(terminal.rows());
                entry["pty_ready"] = json!(true);
                if let Some(fg) = terminal.foreground_process_info() {
                    entry["foreground_process"] = json!(fg.name);
                    entry["foreground_pid"] = json!(fg.pid);
                }
            }
            out.push(entry);
        }
        crate::model::SurfaceLayout::Split { first, second, .. } => {
            collect_surface_layout_info(state, engine, first, pane_id, workspace_id, tab_idx, out);
            collect_surface_layout_info(state, engine, second, pane_id, workspace_id, tab_idx, out);
        }
    }
}
