//! `tab.focused / renamed / created / closed / moved` 발화.

use tasty_plugin_protocol::EventScope;
use tasty_plugin_protocol::LifecycleReason;
use tasty_plugin_protocol::events::payloads::{TabClosed, TabCreated, TabFocused, TabMoved, TabRenamed};

use crate::plugin::PluginManager;

pub(super) fn emit_focused(
    mgr: &mut PluginManager,
    tab_id: u32,
    pane_id: u32,
    prev_tab_id: Option<u32>,
) {
    let payload = TabFocused {
        tab_id,
        pane_id,
        prev_tab_id,
    };
    mgr.emit_host_event("tab.focused", &payload, EventScope::System);
}

pub(super) fn emit_renamed(
    mgr: &mut PluginManager,
    lua: Option<&tasty_lua::LuaEngine>,
    tab_id: u32,
    title: String,
    user_direct: bool,
) {
    let payload = TabRenamed { tab_id, title };
    mgr.emit_host_event("tab.renamed", &payload, EventScope::System);
    if user_direct {
        crate::hooks::lua::fire(lua, "tab.change.post", &payload);
    }
}

pub(super) fn emit_created(
    mgr: &mut PluginManager,
    lua: Option<&tasty_lua::LuaEngine>,
    tab_id: u32,
    pane_id: u32,
    workspace_id: u32,
    kind: String,
) {
    let payload = TabCreated {
        tab_id,
        pane_id,
        workspace_id,
        kind,
    };
    mgr.emit_host_event("tab.created", &payload, EventScope::System);
    crate::hooks::lua::fire(lua, "tab.create.post", &payload);
}

pub(super) fn emit_closed(
    mgr: &mut PluginManager,
    lua: Option<&tasty_lua::LuaEngine>,
    tab_id: u32,
    pane_id: u32,
) {
    let payload = TabClosed {
        tab_id,
        pane_id,
        reason: LifecycleReason::User,
    };
    mgr.emit_host_event("tab.closed", &payload, EventScope::System);
    crate::hooks::lua::fire(lua, "tab.delete.post", &payload);
}

pub(super) fn emit_moved(mgr: &mut PluginManager, tab_id: u32, from_pane: u32, to_pane: u32) {
    let payload = TabMoved {
        tab_id,
        from_pane,
        to_pane,
    };
    mgr.emit_host_event("tab.moved", &payload, EventScope::System);
}
