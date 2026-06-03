//! `surface.focused / resized / title_changed / created` 발화.
//!
//! 본 모듈은 호스트 자동 감지 (`detect_*`) 후 큐에 push 된 이벤트만 다룬다. surface 의
//! 라이프사이클 close (`surface.closed`) 은 별도 `dispatch_pending_surface_lifecycle`
//! 에서 처리.

use tasty_plugin_protocol::EventScope;
use tasty_plugin_protocol::events::payloads::{
    SurfaceCreated, SurfaceCreatedBy, SurfaceFocused, SurfaceTitleChanged,
};

use crate::plugin::PluginManager;

pub(super) fn emit_focused(mgr: &mut PluginManager, surface_id: u32, prev_surface_id: Option<u32>) {
    let payload = SurfaceFocused {
        surface_id,
        prev_surface_id,
    };
    mgr.emit_host_event("surface.focused", &payload, EventScope::Surface);
}

pub(super) fn emit_title_changed(mgr: &mut PluginManager, surface_id: u32, title: String) {
    let payload = SurfaceTitleChanged { surface_id, title };
    mgr.emit_host_event("surface.title_changed", &payload, EventScope::Surface);
}

#[allow(clippy::too_many_arguments)] // reason: host event payload 전체 컨텍스트
pub(super) fn emit_created(
    mgr: &mut PluginManager,
    lua: Option<&tasty_lua::LuaEngine>,
    surface_id: u32,
    kind: &'static str,
    tab_id: u32,
    pane_id: u32,
    workspace_id: u32,
    created_by_plugin: Option<String>,
) {
    let created_by = match created_by_plugin {
        Some(pid) => SurfaceCreatedBy::Agent { source_plugin: pid },
        None => SurfaceCreatedBy::User,
    };
    let payload = SurfaceCreated {
        surface_id,
        kind: kind.to_string(),
        tab_id,
        pane_id,
        workspace_id,
        created_by,
    };
    mgr.emit_host_event("surface.created", &payload, EventScope::Surface);
    crate::hooks::lua::fire(lua, "surface.create.post", &payload);
}
