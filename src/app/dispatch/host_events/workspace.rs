//! `workspace.activated / renamed / created / closed` 발화.

use tasty_plugin_protocol::EventScope;
use tasty_plugin_protocol::LifecycleReason;
use tasty_plugin_protocol::events::payloads::{
    WorkspaceActivated, WorkspaceClosed, WorkspaceCreated, WorkspaceRenamed,
};

use crate::plugin::PluginManager;

pub(super) fn emit_activated(
    mgr: &mut PluginManager,
    workspace_id: u32,
    prev_workspace_id: Option<u32>,
) {
    let payload = WorkspaceActivated {
        workspace_id,
        prev_workspace_id,
    };
    mgr.emit_host_event("workspace.activated", &payload, EventScope::System);
}

pub(super) fn emit_renamed(
    mgr: &mut PluginManager,
    lua: Option<&tasty_lua::LuaEngine>,
    workspace_id: u32,
    name: Option<String>,
    subtitle: Option<String>,
    description: Option<String>,
    user_direct: bool,
) {
    let payload = WorkspaceRenamed {
        workspace_id,
        name,
        subtitle,
        description,
    };
    mgr.emit_host_event("workspace.renamed", &payload, EventScope::System);
    // 사용자 직접 변경(GUI rename dialog)만 Lua hook 발화 — IPC 경유는 제외.
    if user_direct {
        crate::hooks::lua::fire(lua, "workspace.change.post", &payload);
    }
}

pub(super) fn emit_created(
    mgr: &mut PluginManager,
    lua: Option<&tasty_lua::LuaEngine>,
    workspace_id: u32,
    window_id: u64,
    name: String,
) {
    let payload = WorkspaceCreated {
        workspace_id,
        window_id,
        name,
    };
    mgr.emit_host_event("workspace.created", &payload, EventScope::System);
    crate::hooks::lua::fire(lua, "workspace.create.post", &payload);
}

pub(super) fn emit_closed(
    mgr: &mut PluginManager,
    lua: Option<&tasty_lua::LuaEngine>,
    workspace_id: u32,
) {
    let payload = WorkspaceClosed {
        workspace_id,
        reason: LifecycleReason::User,
    };
    mgr.emit_host_event("workspace.closed", &payload, EventScope::System);
    crate::hooks::lua::fire(lua, "workspace.delete.post", &payload);
}
