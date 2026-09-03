<!-- source-hash: ce7205105d3b -->
# Command palette

- **Status**: Implemented
- **Actor**: local user (remote users see it as a mirror)
- **ADR**: none
- **Code**: `src/state/command_palette.rs`, `src/adapters/ui/popup/command_palette.rs`
- **Screens**: [screens/command-palette.md](screens/command-palette.md)

## Purpose

A VS Code-style command palette. Every shortcut command can be searched by query and executed — keyboard-only access to every feature without memorising shortcuts. Opened from the [tools menu](../tools-menu/index.md) or with its dedicated shortcut.

## Internals

### Candidate list

Two sources are merged (`PaletteCommand::Host` / `PaletteCommand::Plugin`):

- Host: `KeybindingSettings::GENERAL_BINDING_FIELDS` (every command that appears in the shortcut settings tab). `toggle_command_palette` itself is excluded (we are already inside the palette).
- Plugin: `AppState.palette_plugin_commands` — a snapshot of `PluginManager::plugin_palette_commands()`. Of the commands declared with `[[contributes.commands]]`, **only `scope = "global"`** are exposed — a `surface` scope is meaningful only while the owner plugin surface is focused, and that context cannot be guaranteed at palette execution time (the same judgement as the focus-free keyboard shortcut path `match_global_shortcut`). Commands of disabled plugins are excluded (`plugin_tool_items` = the same filter as the Tools menu — unlike the settings UI, whose purpose is binding keys in advance, the palette is an execution UI that must show only commands "runnable right now").

### Matching

Query input → case-insensitive. Scored as exact substring (word-start bonus) → subsequence (gap penalty). Labels resolve the host `label_key` or plugin `title_i18n_key` through `t()` (plugin lang namespaces are registered in the same global resolver at discovery, so no separate routing is needed; the same mechanism as tools_menu).

### Navigation / execution

`↑/↓` moves, `Enter` runs, `Esc` closes, click also runs. The first binding is shown in grey on the right of a row (plugin commands show no keycap — override resolution needs `PluginManager` access, which the palette draw function does not have). Only the 6 host commands have dedicated icons; the rest (dynamic host commands + plugin commands) fall back to `COMMAND`.

### Execution path

On Enter/click the selected `PaletteCommand` is loaded into `command_palette.pending_run` → `MainView::handle_redraw` drains it on the next frame:

- `Host`: calls `dispatch_action_by_id` — takes **exactly the same action body as the shortcut**, so the effect is identical.
- `Plugin`: enqueues `(plugin_id, command_id)` into `AppState.pending_plugin_command_invokes` (the palette draw/redraw path cannot reach `PluginManager` — the fixed `PopupDef` signature constraint). `App::dispatch_pending_palette_plugin_commands` drains it on the next IPC processing tick and looks it up in `command_registry`: with an `action` it runs directly via `invoke_tool` (the same pattern as the action branch of `try_plugin_shortcut`); without one it only fires the `command.invoked` event via `key_dispatch::dispatch_plugin_command(.., surface_id: None)` (same as a global shortcut matched without focus — the old `command.invoke` IPC is skipped since there is no target surface).

### AppState synchronisation

`AppState.palette_plugin_commands` is isomorphic to `tool_registry` — populated once at first window assembly (`assemble_app_state`), then refreshed on plugin lifecycle changes (`install`/`remove`/`enable`/`disable`/`grant`/`revoke`/`upgrade_builtins`) by `App::refresh_palette_plugin_commands` (same triggers and call sites as `refresh_tool_registry`).

## Interface

- **User**: the `toggle_command_palette` shortcut + the `Command palette` item in the tools menu.
- **IPC/CLI**: none — intentional. The palette is the *user's keyboard launcher*. Agents perform their actions directly via IPC/CLI, so they have no need for the palette.

## Non-goals

- IPC/CLI exposure.
- Defining the shortcut commands themselves — the palette is only an *execution entry point*; each command's behaviour belongs to its feature.
- Exposing plugin `surface`-scope commands — see "Candidate list" above.

## Acceptance Criteria

- [ ] The palette opens via the shortcut or the tools menu.
- [ ] Typing a query filters every shortcut command (except itself) + the global commands of active plugins by score.
- [ ] `↑/↓`/Enter/Esc and click navigate, execute and close.
- [ ] Executing an item gives the same result as running the shortcut directly (host) / clicking the corresponding tools-menu item (plugin).
- [ ] The palette works normally even with no plugin enabled (no regression).

> As a GUI keyboard feature, visuals are verified by screenshot and matching/filter logic by unit tests. Real plugin command search/execution was confirmed on a live instance via debug IPC (`debug.host_popup.open`/`debug.inject_egui_mouse`).

## Implementation

- State: `src/state/command_palette.rs` (`PaletteCommand::Host`/`Plugin`, `all_commands`, `match_score`, `pending_run`).
- Popup: `src/adapters/ui/popup/command_palette.rs`.
- Plugin command snapshot sync: `src/app/plugin_glue/palette_commands.rs` (`refresh_palette_plugin_commands`), initial populate in `src/app/window_lifecycle.rs` (`assemble_app_state`).
- Plugin command query filter: `crates/tasty-host-plugin/src/manager/queries.rs` (`plugin_palette_commands`).
- Dispatch: `src/view/main/redraw.rs` drains `pending_run` → host goes to `dispatch_action_by_id`, plugin is enqueued into `AppState.pending_plugin_command_invokes`. `src/app/dispatch/palette_plugin_commands.rs` (`dispatch_pending_palette_plugin_commands`) drains it in the App main loop and runs the action / fires the event.

## Screens

- [screens/command-palette.md](screens/command-palette.md) — search input + candidate list layout.
