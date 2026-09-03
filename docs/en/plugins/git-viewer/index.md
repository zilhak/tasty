<!-- source-hash: 057f7ca08cc2 -->
# Git Viewer (`com.tasty.git-viewer`)

- **Status**: Implemented (bundled plugin)
- **Actors**: local user (tools menu → popup) · AI agent (IPC trigger)
- **Distribution / integration**: bundled · tools-menu item + popup — [plugin concepts](../../concepts/plugins.md)
- **Code**: `crates/tasty-plugin-git-viewer/` · query logic shared with host core in
  `crates/tasty-git-core/`
- **Permissions**: `ui.popup` · `ui.tool_item` · `fs.read`
- **Screens**: [screens/git-viewer.md](screens/git-viewer.md)

> **As an example**: the **tools-menu item + popup** example — the reference for a clean structure with main/git/view modules split → [plugin-development](../../dev-guide/plugin-development.md#도구-메뉴-항목--popup).

## Purpose

Provides a popup that shows git **status / log / diff read-only**.
A left **worktree rail** shows main + every linked worktree at a glance; pick a worktree to switch
status/log/diff to that worktree (read-only).

## Internal behaviour

- **tool** `open-viewer` — adds an item to the [tools menu](../../features/tools-menu/index.md) (`ui.tool_item`), action `open_popup{com.tasty.git-viewer/viewer}`.
- **command** `open_viewer` — also opens the viewer via a shortcut (`scope = "global"`, no `default_keybinding` — the user assigns one under Settings > Keybindings > Plugins). The action is the same `open_popup{com.tasty.git-viewer/viewer}` as the tool item.
- **popup** `viewer` — trigger `ipc` (also opens via IPC), `rendering = egui-mesh`. Reads status/log/diff with `fs.read` and displays them. **Read-only** (no commits/staging or other changes).
- **Rendering** — the popup content is drawn with **egui-mesh** (ADR-0028 / B3): the plugin paints the design
  (`overlays/git_viewer.jsx`) directly in its own egui Context and the host owns only the shell (scrim/border/Esc/outside-click).
  The Theme is received every frame as `ThemeWire` in `popup.set_context` and reconstructed.
- **Complete worktree list** — libgit2's `worktrees()` yields only linked ones, so the main working tree is
  identified directly via the `commondir` file (fallback: path inference) and synthesised at the head of the list (equivalent to `git worktree list`).
  The worktree containing the cwd the popup received gets a `current` marker; `locked`/`invalid` status badges are shown.
- **Worktree switching** — selecting a rail row reopens at that worktree's workdir and rebinds status/log/diff.
  **No actual checkout/working dir/HEAD change** (only the plugin popup's internal state changes).
- **Repo handle** — local mode holds one `Repository` handle for the active worktree and reuses it rather
  than reopening per operation. Invalidation happens on exactly three conditions — worktree switch · Refresh · repo
  loss — and Refresh always drops the cache (external edits, worktree add/remove and external commits are always
  reflected). [ADR-0099](../../adr/0099-git-viewer-repo-handle-cache-and-canonical-dedup.md).
- **fs access** — git2 reads files directly (bypassing the host fs port), so worktrees outside the cwd are readable too.
  The permission declaration stays `fs.read`.
- **attach mirror workspaces** — when opened on a mirror surface that has no real PTY/filesystem in the local process
  (`mirror`/`local_surface_id` in the `popup.open` context), instead of a local `git2::Repository::discover` the host
  round-trips the query over the attach channel to the **remote** (mirror target) tasty instance
  (`git_viewer.query` plugin→host IPC → the attach `git_query_request`/`git_query_result` event
  pair → Event Bus unicast reply to the plugin). status/log/diff/worktrees all work through this path, and
  refresh, worktree switching and file→diff clicks each trigger a separate round trip. The server discovers the
  repository from its own real remote PTY (`Terminal::get_cwd()` found via `surface_id`), not the cwd string the client forwarded.
  If the response is large (700KiB budget) it is truncated in status/log/diff order. Design rationale and wire
  format details: [ADR-0056](../../adr/0056-git-viewer-remote-attach-git-query-channel.md).

## Interface

- **User**: tools menu `Git viewer` → popup.
- **AI agent**: the popup trigger is ipc — it can be opened via IPC.

## Non-goals

- git *writes* (commits/staging/branch manipulation) — read-only.
- **Worktree manipulation** (add/remove/prune/lock toggling) — keeps its read-only identity. List and switch only.
- The branch display in the status bar — [workspace-status-bar](../../features/workspace-status-bar/index.md) (separate).

## Acceptance Criteria

- [ ] Given the plugin is enabled Then the tools menu shows a git viewer item.
- [ ] Given the popup is opened inside a repo Then status/log/diff are shown.
- [ ] Given a non-repo Then an appropriate empty/error display.
- [ ] Given a repo with several worktrees Then the rail lists main + every linked worktree and
      the worktree containing the cwd shows a `current` marker.
- [ ] Given a worktree row is selected Then status/log/diff switch to that worktree and
      the actual working dir/checkout does not change.
- [ ] Given an ordinary repo with no worktrees Then the rail shows the single main entry (backwards compatible).
- [ ] Given the popup is opened in an attach mirror workspace Then the remote repository's actual status/log/diff/
      worktrees are shown (not the local host's information or a spurious "No git repository found").
- [ ] Given Refresh in a mirror popup Then a new commit on the remote appears in the list.

## Screens

- [screens/git-viewer.md](screens/git-viewer.md) — the git status/log/diff popup.
