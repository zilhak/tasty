<!-- source-hash: 3c2d986f1ee5 -->
# Clipboard Viewer popup screen

- **Parent plan**: [../index.md](../index.md)
- **Visual source**: Claude Design project `Tasty Design System` (projectId `41fd3f5a-4bb9-4877-999f-db5124dc2925`)
  `ui_kits/terminal/overlays/clipboard_viewer.jsx` — structural transcription complete.

The clipboard viewer popup opened from the tools menu / shortcut. A four-tier vertical stack of header → type-bar → body → footer
(the left-rail master-detail layout was retired).

## Trigger

[Tools menu](../../../features/tools-menu/screens/tools-menu.md) `Clipboard Viewer` or the plugin command `open_viewer` (Settings > Keybindings > Plugins, default `ctrl+shift+h`).

## UI element inventory

- **header** — clipboard icon + "Clipboard" title (14px/600) + `snapshot` badge (default tag) + close IconButton on the right.
- **type-bar** — left [`type_switch`]: with one available type, an icon + accent badge (read-only); with two or more, a horizontal segmented button group (no rail). At five or more (`SEG_COMPACT_AT`) inactive segments compact to icon-only and show the full type name as a hover tooltip. The Other segment/badge's hover tooltip shows "{n} unrecognized formats" (the number of formats found) instead of the default label ("Other"). The right slot is swapped for a "Pretty print" `Checkbox` (`tasty_ui_widgets::checkbox`) only for the HTML type; empty for other types.
- **body** — per-type content inside a well (border+radius+bg-app). Text is a mono pre scroll (`well`). Files is icon + mono path one per line (long paths ellipsised, `well` scroll). Image has **no inline rendering** (design decision) — the well switches to centred alignment (`well_centered`) showing only an icon (fixed 30px) + dimension/size meta (mono caption) + a "no inline preview" notice (caption, italic, `text-disabled`). HTML (the original source or the prettified result, unrendered, in the same style). Other (every raw format that is not text/files/image/html, listed as per-format blocks inside a `well` scroll; each block shows name (mono caption, bold, `text-secondary`) + size (mono caption, `text-muted`) on one line and the textualised preview (mono term-sm, `text-primary`) below, 1px `separator` between blocks, truncated with `+N more lines` when long — the list itself is never collapsed).
- **footer** — mime text (mono caption, left; the HTML type combines meta as `{mime} · {n} chars · {n} line(s)`, the Other type has no mime so "{n} unrecognized formats" replaces that slot entirely) + Close button (secondary, right). Functionally redundant with the host's outside-click/Esc, but explicitly required by the design.
- (empty state) icon + bold title + two faint subtitle lines.
- (read failure) same structure as above, danger tone.
- (already open) same structure as above, lock icon.

## Visuals per state

- Has types (header+type-bar+body+footer four tiers) / empty clipboard / read failure / already open (re-invocation ignored) — the latter three keep the header and replace only the body with a CenterState (icon+title+subtitle).

## Render path

The popup is drawn with **egui-mesh** (ADR-0028 / B4). The plugin tessellates the popup content with egui in its own
process and the host composites the mesh into the content area. The host sends a Theme snapshot (`ThemeWire`) in
`popup.set_context` and the plugin reconstructs it with `Theme::with_colors_and_zoom` to draw per the design tokens.
The chrome (scrim/border/outside-click/Esc/single-instance shell) is host-owned —
the plugin draws only the header~footer content area (`cbFrame`/`Scrim` are standalone-preview mockups in the design and
the plugin does not redraw them).

Clicking the header/footer Close button is returned as a `bool` from `view::draw`/`draw_already_open`, and
`main.rs` asks the host to close via the `popup.close` IPC on that value (the host keeps owning the shell lifecycle —
[popup-implementation.md](../../../dev-guide/popup-implementation.md)).

Icons are drawn by `tasty_plugin_sdk::baked_icon::draw` from a build-time SVG→vector bake (`build.rs`, the canonical
pattern being `tasty-plugin-image/build.rs`).

## Design token mapping

All colours, fonts and spacing come from the `Theme` tokens the host sends (no from_rgb / raw px). UI inventory ↔ tokens:

| UI element | Token | Notes |
|---|---|---|
| popup frame | `bg-panel` | fixed 480×360 (size_hint); plugin content uses the same fill |
| header/type-bar/footer horizontal inset | `spacing-md` | approximates the design's `var(--tasty-size-14)` (Theme has no dedicated 14px token) |
| header title | `font-size-max`(14) + `text-primary` | `.strong()` |
| snapshot badge | `tag`(Default variant) | `tasty_ui_widgets::tag` |
| type-bar row background | `bg-sidebar` | |
| single-type badge | `tag`(Accent variant) + `text-muted` icon | |
| segments (two or more) | `border-default` group border + `corner-radius`, active `accent-primary`/`text-on-accent`, idle `text-secondary` | |
| body well | `bg-app` fill + `separator`+`border-width` + `corner-radius` | `ScrollArea` (text) or centred (image, `well_centered`) |
| body preview text | `font-size-term-sm`(12) mono + `text-primary` | |
| type-bar right meta (image etc.) | `font-size-caption`(11) mono + `text-muted` | design `cbMetaMono`, `meta_label` |
| image body icon | fixed 30px (outside the Theme icon token cap of 16) + `text-muted` | same policy as `CENTER_ICON_SIZE`(28) |
| image body "no preview" notice | `font-size-caption`(11) italic + `text-disabled` | design `fontStyle: italic` |
| footer mime text | `font-size-caption`(11) mono + `text-muted` | HTML type combines as `{mime} · {meta}`, Other's meta replaces the mime entirely |
| footer Close button | `tasty_ui_widgets::Button`(Secondary) | |
| type-bar right Pretty print checkbox | `tasty_ui_widgets::checkbox` own tokens | HTML type only, no new tokens |
| other format name | `font-size-caption`(11) mono + `text-secondary` | `.strong()`, no new tokens |
| other format size / +N more lines | `font-size-caption`(11) mono + `text-muted` | no new tokens |
| other block separator | `separator` + `border-width` | 1px hline between blocks |
| CenterState title | `font-size-body`(13) + `text-secondary` (or `accent-danger` on danger) | `.strong()` |
| CenterState subtitle | `font-size-term-sm`(12) + `text-muted` | |
| read-failure tone | `accent-danger` | |

## HTML prettify indenter

`crates/tasty-plugin-clipboard-viewer/src/html_format.rs::prettify` — not a proper HTML5 parser but a heuristic
tokeniser that counts tag depth (no new external dependency). It ports the reference algorithm validated by the Claude
Design draft as-is (`>\s+<` whitespace normalisation → split on `<...>` tag boundaries → closing tags decrease depth
first, opening tags increase depth after output, void elements / self-closing add nothing). However, the insides of
`<script>`/`<style>`/`<pre>` are extracted as complete spans and preserved verbatim — a literal port of the reference
algorithm broke the original in scripts containing `<`/`>` (e.g. `if (a < b)`), so only that part was replaced with an
explicit exception. A display-only re-indenter — no DOM parsing/sanitising/rendering. Malformed input (unclosed tags
etc.) produces a best-effort result without panicking (guaranteed by unit tests, including idempotence).

## Enumerating Other raw formats

`crates/tasty-plugin-clipboard-viewer/src/raw_formats/{mod,windows,macos,x11}.rs` — arboard does not expose
clipboard-format enumeration (the `arboard::Error::ContentNotAvailable` doc comment states it does not distinguish
"empty" from "a format other than these four (text/files/image/html)"). The remainder is enumerated by calling the
platform raw APIs directly:

- **Windows** — `clipboard-win`'s `EnumFormats` (enumerate all format IDs) + `format_name_big` (human-readable name) +
  `get_vec` (raw bytes of any format). Excludes `CF_TEXT`/`CF_UNICODETEXT`/`CF_OEMTEXT` (all text variants) ·
  `CF_HDROP` (files) · `CF_DIB`/`CF_DIBV5` + the registered format named "PNG" (image) · the registered format named
  "HTML Format" (html).
- **macOS** — `NSPasteboard.types()` (the full UTI array) + `dataForType:` (raw bytes of any type).
  Excludes text variants such as `public.utf8-plain-text` · `public.file-url` (files) · image variants such as
  `public.tiff`/`public.png` · `public.html`.
- **Linux (X11/XWayland)** — the ICCCM standard procedure implemented directly with x11rb: request the `TARGETS` atom via
  `ConvertSelection` → `SelectionNotify` response → retrieve the supported atom list with `GetProperty`, then re-query
  each atom the same way. Excludes `UTF8_STRING`/`STRING`/`TEXT`/`text/plain*` · `text/uri-list` (files) · `image/png` ·
  `text/html`. The `wayland-data-control` feature is not enabled (the other types already go this route), so on a pure
  Wayland session (no XWayland) the connection itself fails — an empty vector + `tracing::debug!` records the cause,
  distinguishing "no others" from "could not query".
- **Common** — on all three platforms exclusion is a mapping table (listing every raw variant of the same semantic
  type), not a single ID/name comparison — just as text/html land on the clipboard together when a browser copies rich
  text, the same semantic format can exist under several raw IDs/names at once. A race where the clipboard owner changes
  between the TARGETS (or EnumFormats/types) query and the per-format re-query is handled by per-format isolation that
  skips just that format (it does not fail the whole "Other" enumeration). Raw bytes → text is shared by all three
  platforms through `clipboard::OtherFormatEntry::from_bytes` (`from_utf8_lossy` + a size cap + a hex-summary fallback
  when the U+FFFD ratio says binary), and the raw byte contents never reach the log through any path.

## Gallery specimen

`crates/tasty-gallery/src/catalog/components/clipboard_viewer.rs` — Overlays › `Clipboard viewer
popup`. Transcribed with tokens: four rows header/type-bar (badge)/body (well)/footer (text) + four rows header/type-bar
(Text/Files segments)/body (icon+path rows)/footer (files) + the image state (icon+meta+notice) + two HTML rows raw/pretty
(including the Pretty print checkbox on the right of the type-bar) + the other state (two format blocks listed — one a
short text, one a `+N more lines` truncation example) + the three CenterStates empty/read-failed/already-open (independent
of the host/plugin crates, pixel identity a non-goal). The indented result of the HTML pretty state and the format-block
samples of the other state are hand-prepared samples following the same rules as `html_format::prettify()` /
`clipboard::OtherFormatEntry` respectively (the gallery cannot depend on the plugin crate). Compact segments at
`SEG_COMPACT_AT`(5) or more are not yet in the specimen because the real data has only five kinds (Text/Files/Image/
Html/Other) and a scenario where all co-occur is uncommon. Three-way mapping:
[design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#clipboard-viewer-overlays).

## Visual source

Claude Design project `Tasty Design System` (projectId `41fd3f5a-4bb9-4877-999f-db5124dc2925`)
`ui_kits/terminal/overlays/clipboard_viewer.jsx` (structural transcription source) ·
`clipboard_viewer.html` (standalone preview) · `shared.jsx` (`Scrim`/`Icon`/`Spinner` shared
primitives). The popup is self-rendered by the plugin over the egui-mesh channel
([popup-implementation.md](../../../dev-guide/popup-implementation.md), ADR-0028).
