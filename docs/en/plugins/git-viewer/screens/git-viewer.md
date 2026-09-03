<!-- source-hash: 2683f02261ca -->
# Git Viewer popup screen

- **Parent plan**: [../index.md](../index.md)
- **Visual source**: design `ui_kits/terminal/overlays/git_viewer.jsx`.

The read-only git status/log/diff popup opened from the tools menu / IPC. Drawn with **egui-mesh** — the plugin
paints the content directly in its own process's egui Context and the host owns only the shell (scrim/border/Esc/outside-click)
(ADR-0028 / B3). The Theme is delivered every frame as `ThemeWire` in `popup.set_context` and reconstructed as the
same `Theme` the host uses.

## Trigger

The [tools menu](../../../features/tools-menu/screens/tools-menu.md) git viewer item or IPC. Single
instance — a second open shows an "already open" notice.

## Layout

960×640. `header + context strip + body`.
- **header** — `Git` title + `Refresh` (secondary). `separator` below.
- **context strip** — a `bg-sidebar` band: active worktree · branch · HEAD oid pill · repo path (right, ellipsis).
- **body** — left **worktree rail** (fixed 232px) | right **right column**. The right column is split 50/50 into
  **Changes** above / **Commits↔Diff** below.

## UI element inventory

- **worktree rail** — main + every linked worktree as two-line rows. line1 = name (mono) + type pill
  (`main`=sky / `linked`=neutral), line2 = short oid (sky) + status pill (`current`/`locked`/`invalid`, dot).
  Selected row = `surface-active` + a 2px inset accent bar on the left. Selecting rebinds the right side to that worktree.
  Invalid rows are dimmed and cannot be switched to.
- **Changes** — the list of changed files. A fixed-width status pill (`M/A/D/R/?/U`) + path (dir `text-muted` / file
  `text-primary`). Selecting a row replaces the lower pane with the diff (inset bar on the selected row).
- **Commits** — commit history. oid (sky) + refs pills (sky, only when present) + summary (flex) + author + time.
- **Diff** — the unified hunks of the selected file. Toolbar (`Back` ghost + file path) + a recessed `bg-app` well:
  old/new line-number gutter + `+`/`-`/context sign column + ±-line background tint + hunk header band (sky).
- All **read-only** (no action buttons). Interactions = Refresh · worktree selection (rebind) · file selection (→diff) ·
  diff Back. Input is real user input forwarded by the host, handled inside the plugin's egui.

## Scrolling (virtualisation)

All four lists (worktree rail · Changes · Commits · Diff) **lay out only the visible rows** —
`ScrollArea::show_rows` draws only the row range intersecting the viewport (rationale and alternatives:
[ADR-0095](../../../adr/0095-plugin-list-virtualization-and-fixed-content-width.md)). The commit list has a
query cap of 200 rows and the diff is the file's full lines, so per-frame cost is proportional to viewport height
regardless of list length.

- Row height is a single value per list (Changes 26 · Commits 28 fixed; the worktree rail and diff derive from the theme
  but are identical per row). The row function and the height helper (`wt_row_h` / `diff_row_h`) come in pairs, so
  fixing only one makes rows overlap or gap.
- Selection handlers receive **indices relative to the whole list** — iterating a partial range does not change which
  worktree is selected or which file→diff is switched to.
- The diff's horizontal scroll width is the widest of all lines (hunk headers included), **measured once and cached**,
  and every row is allocated that width. Measuring only the visible lines would make the width jitter with scroll
  position. The cache is keyed by font size and cleared when the diff changes.
- The diff's ±-line tint and hunk header band are painted only up to **that row's own text width** — a value separate
  from the allocated width above (widest of all lines). Painting the band to the allocated width would stretch a short
  row's band to the end of the horizontally scrolled content.

## Visuals per state

- repo / non-repo (centred notice) / detached (`detached` in place of the branch).
- worktree: current (green dot pill) / linked / locked (yellow dot pill, reason on hover) / invalid (red dot pill) /
  zero worktrees (`no_worktrees`).
- Empty/none notices (Changes `no_changes` · Commits `no_commits` · rail `no_worktrees`) are a single centred line in the pane.
- error (`accent-danger` line, tinted band) is shown below the header, above the body.
- already-open (second instance of the single instance) centred notice.

## Design token mapping

All colours, fonts and spacing are `Theme` tokens (host catppuccin → semantic tokens). UI inventory ↔ tokens:

| UI element | Token | Notes |
|---|---|---|
| popup frame (host shell) | `bg-panel` · `border-default` | 960×640 |
| context / section strip | `bg-sidebar` · `separator` | top band |
| section title | `text-muted` · `font-size-micro` mono uppercase | includes count |
| selected row | `surface-active` + `accent-primary` inset bar | worktree / change |
| HEAD·commit oid · refs · `main` · hunk | `accent-info` | sky (Tag `Info` tone) |
| `current` · added (`A`) · diff `+` | `accent-success` | `green` |
| `locked` · modified (`M`) | `accent-warning` | `yellow` |
| `invalid` · deleted (`D`) · unmerged (`U`) · diff `-` · error | `accent-danger` | `red` |
| `linked` · untracked (`?`) | neutral (`text-secondary`/`border-default`) | Tag `Default` |
| dir path · author · time · gutter | `text-muted` / `text-disabled` | |
| diff well | `bg-app` | recessed |

## Gallery specimen

`crates/tasty-gallery/src/catalog/components/git_viewer.rs` — Overlays › `Git worktree viewer
popup`. Transcribes the context strip · section strip · two-line worktree rows · Changes · Commits · diff well with
token/structure parity (pixel identity a non-goal — ADR-0020 completeness). Three-way mapping:
[design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#git-viewer-plugins).

## Visual source

Design `ui_kits/terminal/overlays/git_viewer.jsx` (+`.html` preview). The popup implementation is the egui-mesh
channel (ADR-0028) + the `EguiMeshPopup` SDK helper.
