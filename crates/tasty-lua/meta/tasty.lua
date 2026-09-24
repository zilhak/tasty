---@meta
--- LuaLS definitions for registered Tasty scripts.
--- Add this directory to workspace.library in .luarc.json for completion and type checking.
--- Example: { "workspace.library": ["<TASTY_REPO>/crates/tasty-lua/meta"] }

---@class tasty
tasty = {}

--- Register a callback for a host event. Multiple callbacks per event are
--- supported (fired in registration order). Errors in one callback are logged
--- and do not abort sibling callbacks. Tasty hooks are *observe-only* — the
--- callback's return value is ignored and cannot cancel/alter host behavior.
---@param event TastyEvent     -- canonical event name (see EventMatrix below)
---@param callback fun(ctx: table): nil
function tasty.on(event, callback) end

--- Log at INFO level via tracing.
---@param msg string
function tasty.log(msg) end

--- Log at WARN level via tracing.
---@param msg string
function tasty.warn(msg) end

--- Not registered by the current host; this stub does not provide notifications.
---@param title string
---@param body string
function tasty.notify(title, body) end

--- Ask the main thread to spawn the Tasty CLI without waiting for completion. Stdio is nulled.
---@param args string[]
function tasty.run_cli(args) end

-- ─────────────────────────────────────────────────────────────────────────────
-- Event names
-- ─────────────────────────────────────────────────────────────────────────────

---@alias TastyEvent
---| "tasty.startup.post"
---| "window.create.post"
---| "window.delete.post"
---| "workspace.create.post"
---| "workspace.delete.post"
---| "workspace.change.post"
---| "tab.create.post"
---| "tab.delete.post"
---| "tab.change.post"
---| "pane.create.post"
---| "pane.delete.post"
---| "surface.create.post"
---| "surface.delete.post"

-- ─────────────────────────────────────────────────────────────────────────────
-- Payload shapes
-- (Field names mirror tasty-plugin-protocol payloads. Optional fields use ?.)
-- ─────────────────────────────────────────────────────────────────────────────

---@class WindowCreated
---@field window_id integer
---@field kind string
---@field modality "modeless"|"modal"

---@class WindowClosed
---@field window_id integer
---@field reason string

---@class WorkspaceCreated
---@field workspace_id integer
---@field window_id integer
---@field name string

---@class WorkspaceClosed
---@field workspace_id integer
---@field reason string

---@class WorkspaceRenamed
---@field workspace_id integer
---@field name string|nil
---@field subtitle string|nil
---@field description string|nil

---@class TabCreated
---@field tab_id integer
---@field pane_id integer
---@field workspace_id integer
---@field kind string

---@class TabClosed
---@field tab_id integer
---@field pane_id integer
---@field reason string

---@class TabRenamed
---@field tab_id integer
---@field title string

---@class PaneCreated
---@field pane_id integer
---@field parent_pane_group integer|nil
---@field workspace_id integer

---@class PaneClosed
---@field pane_id integer
---@field reason string

---@class SurfaceCreatedBy
---@field kind "user"|"agent"
---@field source_plugin string|nil

---@class SurfaceCreated
---@field surface_id integer
---@field kind string
---@field tab_id integer
---@field pane_id integer
---@field workspace_id integer
---@field created_by SurfaceCreatedBy

---@class SurfaceClosed
---@field surface_id integer
---@field kind string
---@field reason string
