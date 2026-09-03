<!-- source-hash: 58e5878fad5c -->
# Closed item restore

- **Status**: Implemented
- **Actors**: local user only (`restore_closed`, default `Ctrl+Shift+T`)
- **ADR**: none (the principle is [identity](../../identity.md) §1) — panes not being restored was not an original scope decision but a bug where `close_case_pane`/`close_active_pane` did not call `push_closed_item` (`ClosedPane`/`rebuild_pane` already existed for workspace restore but were not wired to standalone pane restore). A fix completing the existing intent rather than a decision setting new scope, so not an ADR candidate.
- **Code**: the `ClosedItem` LIFO (`crates/tasty-model`), snapshot push `src/state/{pane,tab,workspace}.rs` + `src/core/impl_close.rs` (the `close_case_pane`/`close_case_tab`/`close_case_workspace` cascade), tree reinsertion `crates/tasty-model/src/pane_tree.rs` (`locate_split_context`/`insert_pane_beside`) + `src/core/impl_workspace.rs` (`apply_restore_closed_item`)
- **Screens**: none (a restore is reflected immediately in the focused pane)

## Purpose

Immediately brings back a surface/tab/pane/workspace the user closed by mistake. A volatile safety net that keeps a snapshot at close time (`ClosedItem`, an **in-memory LIFO stack**) and restores it via shortcut — not saved to disk. Permanent storage for repeated use is [layout-presets](../layout-presets/index.md).

## Internal behaviour

- Trigger: the `restore_closed` shortcut ([keybindings](../keybindings/index.md), `Ctrl+Shift+T` common to all four presets).
- Restores the item at the top of the stack. Surface/Tab go into the pane focused at call time. A Pane is reinserted into the current workspace tree reproducing its close-time split geometry (direction/ratio/sibling pane) as far as possible; if that sibling pane has since disappeared, it splits from the pane focused at call time instead. With zero workspaces, Surface/Tab/Pane restore first creates the default workspace.
- **Scrollback is always restored regardless of `general.restore_surface_content`** (the in-memory copy is reused immediately, no disk involved).
- If the surface meta `restore.command` existed at close time (e.g. the claude plugin's `claude -r <id>`), it runs automatically right after the shell starts → resuming the TUI session (the same mechanism as [layout-persistence](../layout-persistence/index.md)).
- With nothing to restore, a no-op (no toast/notification).

## User/agent separation (core)

The restore stack is **the user's viewpoint state**, so it is absent from the agent surface ([identity](../../identity.md) §1):

- No CLI/IPC triggers a restore (the `RestoreClosedItem` intent is user-shortcut only).
- **Items an agent closed do not enter the stack** — the IPC `surface.close` uses `save_snapshot=false`, and `tab.close`/`pane.close` (DomainIntent) have no snapshot path at all. Snapshot pushes happen only on the user shortcut/mouse close paths.

## Non-goals

- A restore API for agents — an agent recreates the resources it made by ID.
- Persistence across an app restart — [layout-persistence](../layout-persistence/index.md) / [layout-presets](../layout-presets/index.md).
- Restoring PTY process state — the shell starts fresh; only scrollback + `restore.command` carry over.

## Related

- [layout-persistence](../layout-persistence/index.md) · [layout-presets](../layout-presets/index.md) · [work-area](../work-area/index.md)
