<!-- source-hash: 24ef388a27af -->
# Clipboard Viewer (`com.tasty.clipboard-viewer`)

- **Status**: Implemented (bundled plugin)
- **Actors**: local user (tools menu / shortcut → popup)
- **Distribution / integration**: bundled · tools-menu item + popup — [plugin concepts](../../concepts/plugins.md)
- **Code**: `crates/tasty-plugin-clipboard-viewer/`
- **Permissions**: `ui.tool_item` · `ui.popup` · `clipboard.read`
- **Screens**: [screens/clipboard-viewer.md](screens/clipboard-viewer.md)

> **As an example**: the **tools-menu item + popup** (master-detail) example. The reference for the [ADR-0009](../../adr/0009-plugin-sandbox-deferred.md) non-sandboxed model in which **the plugin process reads the clipboard directly with `arboard`** with no host backend → [plugin-development](../../dev-guide/plugin-development.md#도구-메뉴-항목--popup).

## Purpose

Provides a read-only popup that **previews the current system clipboard contents, classified by type**. History (accumulating past items) is out of scope — it only shows what is in the clipboard right now. The preview differs by type — Text shows the body as-is, while Image is **not rendered inline** (an explicit decision by the design system): only an icon + dimension/size meta + a "no inline preview" notice. All remaining raw formats that belong to none of Text/Files/Image/HTML are grouped into a single **"Other" bucket** — arboard does not expose clipboard-format enumeration at all (`Error::ContentNotAvailable` does not distinguish "empty" from "a format other than these four"), so the plugin enumerates directly through the platform raw APIs (Windows `clipboard-win` / macOS `objc2-app-kit` / Linux `x11rb` `TARGETS`), excludes the variants already consumed as text/files/image/html (a per-platform mapping table, not a single-ID comparison), and shows the rest roughly as raw text.

## Internal behaviour

- **tool** `open-viewer` — adds an item to the [tools menu](../../features/tools-menu/index.md) (`ui.tool_item`), action `open_popup{com.tasty.clipboard-viewer/viewer}`.
- **command** `open_viewer` — also opens the viewer via a shortcut (`scope = "global"`, default `ctrl+shift+h` — changeable under Settings > Keybindings > Plugins). The action is the same `open_popup{com.tasty.clipboard-viewer/viewer}` as the tool item (migrated from the old host-hard-coded dedicated `toggle_clipboard_viewer` field to the same plugin command registry as [git-viewer](../git-viewer/index.md)).
- **popup** `viewer` — trigger `ipc`. A four-tier structure: header (icon + title + snapshot badge + close) → type bar (icon + badge for a single type, a horizontal segmented switch for two or more) → body (a scrolling well preview) → footer (mime [+ meta] + Close) (structural transcription of the design system; the previous left-rail master-detail layout was retired).
- On popup open, the clipboard is read once with `arboard::Clipboard` to build the list of available types. Currently Text/Files/Image/HTML/Other are populated (Files via `arboard::Get::file_list()`; Image via `arboard::Clipboard::get_image()` keeping only the dimensions (width/height) and byte count, not the pixel data itself — not needed since it is not rendered; HTML via `arboard::Clipboard::get().html()`; Other bypasses arboard and is enumerated directly by the `raw_formats` module through the platform raw APIs). Selecting a type → the body preview updates.
- **HTML type**: not rendered; the original source is shown as the same mono text as the text type (the default state). A "Pretty print" checkbox appears in the right slot of the type bar; when checked, the body switches to the result of `html_format::prettify()` (a tag-depth indenter with no new dependency; `<script>` / `<style>` / `<pre>` are preserved verbatim). The checked state lasts only for the lifetime of the popup instance (reset on close, not persisted to settings). The displaced meta (char count / line count) is combined into the footer as `{mime} · {meta}`.
- **Other type**: every raw format matched by none of text/files/image/html is grouped into one type. The body lists, for each discovered format, a block with name (mono, bold) + size (mono, muted) on one line and below it a textualised preview of the raw bytes (`from_utf8_lossy` + a size cap + a hex-summary fallback when judged binary), stacked vertically with 1 px separators between blocks — the list itself (the number of formats) is never collapsed. Long previews are truncated with `+N more lines`. Per-format lookup failures are isolated (one failing does not affect the rest), and raw byte contents are never logged. In a pure Wayland session (no XWayland), if the X11 connection itself fails, that is distinguished with an empty list + a debug log (a silently empty list would confuse "no others" with "could not query"). The footer shows "{n} unrecognized formats" in place of the mime (a bucket of heterogeneous formats has no single mime).
- **Single instance**: if already open, re-invocation is ignored (`already_open`).

## Interface

- **User**: tools menu `Clipboard Viewer` or the shortcut assigned under Settings > Keybindings > Plugins → popup. Pick a type in the type bar → check the contents in the body.
- **AI agent**: one-shot clipboard read/write is each agent process's own direct-access territory, not the host's (ADR-0009). This plugin is a pure viewer that exposes no IPC namespace.

## Non-goals

- Clipboard **history** collection / re-copy — removed (the host `ClipboardHistory` backend was retired). This plugin shows only the *current* clipboard.
- Clipboard **writing / editing** — read-only.
- The tools menu itself — [tools-menu](../../features/tools-menu/index.md).

## Acceptance Criteria

- [ ] Given the plugin is enabled Then the tools menu shows a `Clipboard Viewer` item.
- [ ] Given the shortcut (plugin command `open_viewer`) Then the viewer popup opens.
- [ ] Given text in the clipboard Then the type bar shows the text-type badge and the body previews the contents.
- [ ] Given files (a path list) in the clipboard Then the type bar shows the files type and, when selected, the body lists icon + path one per line.
- [ ] Given an image in the clipboard Then the type bar exposes the image type and, when selected, the body shows an icon + dimension/size meta + the "no inline preview" notice (no actual picture is rendered).
- [ ] Given HTML in the clipboard Then the type bar shows the HTML type, the body shows the unrendered original source, and a "Pretty print" checkbox appears on the right of the type bar.
- [ ] Given "Pretty print" checked on the HTML type Then the body switches to an indented form and returns to the original when unchecked. The footer shows `{mime} · {n} chars · {n} line(s)`.
- [ ] Given a raw format in the clipboard that belongs to none of text/files/image/html (e.g. an app-specific custom format) Then an "Other" type appears in the type bar and, when selected, the body lists a name + size + textualised preview block per format, separated by separators. The footer shows "{n} unrecognized formats".
- [ ] Given rich text copied from a browser so that both text and html are in the clipboard Then the raw variants of that text/html are not caught as duplicates under "Other".
- [ ] Given an empty clipboard Then the empty-state message is shown (and Other does not appear either).

## Screens

- [screens/clipboard-viewer.md](screens/clipboard-viewer.md) — the master-detail viewer popup.
