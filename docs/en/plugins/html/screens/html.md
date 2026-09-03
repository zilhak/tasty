<!-- source-hash: 09d880315e8d -->
# HTML surface screen

- **Parent plan**: [../index.md](../index.md)
- **Visual source**: the native WebView overlay — the content is drawn by the OS WebView (design tokens do not apply).

An HTML surface drawn as a WebView overlay at the tile position in the [work area](../../../features/work-area/screens/work-area.md).

## Trigger

Opening an HTML file or creating an `html` surface (`--url`).

## UI element inventory

- **WebView content** — the web render of the URL/file. Aligned as an overlay on the tile rect.
- The tab display name is the file name/URL.

## Visuals per state

In the tree the surface is a `RemoteSurface` marker. The native WebView's navigation lifecycle
(start/finish/fail) is delivered to the host as `NavState`
(Idle/Loading/Done/Failed) by the three backends (WebView2 / WKNavigationDelegate / WebKitGTK), and the host draws chrome per that state:

- **Idle** — no URL set. Placeholder (`GLOBE` · "No page loaded").
- **Loading** — navigating. The WebView overlay is hidden and a `Spinner` + "Loading…" chrome shown.
- **Done** — success. The WebView overlay draws the page (the boundary chrome backdrop is exposed only while the overlay is
  temporarily hidden by a menu/popup).
- **Failed** — failure. With the overlay hidden, `ALERT_CIRCLE` (`accent-danger`) + "Failed to load"
  + URL chrome. The failure reason is recorded only as a `tracing::warn!` log, not on screen.

## Design token mapping

The content pixels are drawn by the OS WebView and are **token-independent**. What tasty is responsible for with tokens is
only the *chrome* before the overlay attaches / on failure — defined thinly:

| chrome element | Token | Notes |
|---|---|---|
| tile boundary | `bg-panel` + `border-default` | overlay mount area |
| boundary / placeholder glyph | `GLOBE` glyph · `text-muted`/`text-disabled` | gallery `icons.rs` SURFACES |
| notice caption / URL | `text-muted` · `text-disabled` | "No page loaded" etc. |
| loading | `Spinner` (ui-widgets) · `text-muted` | while attaching/navigating |
| error | `ALERT_CIRCLE` glyph + `accent-danger` | load failure |
| content area | (empty) | covered by the native overlay |

## Gallery specimen

`crates/tasty-gallery/src/catalog/components/html_chrome.rs` — Layouts › `Content viewers` ›
`HTML (webview) chrome`. Only the four chrome states boundary / placeholder / loading / error are transcribed (the content is
the overlay). Three-way mapping: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-layouts).

## Visual source

The content is rendered by the OS-native WebView, so design-system tokens do not apply (the web page's own styles). Only tile alignment/boundary follows the work-area layout.
