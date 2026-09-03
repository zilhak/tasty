<!-- source-hash: 918299814a4e -->
# HTML Viewer (`com.tasty.html`)

- **Status**: Implemented (bundled plugin)
- **Actors**: local user (GUI surface) · AI agent (`tasty html` CLI)
- **Distribution / integration**: bundled · surface_kind (webview) · file handler — [plugin concepts](../../concepts/plugins.md)
- **Code**: `crates/tasty-plugin-html/`, host WebView overlay
- **Permissions**: manifest `permissions`
- **Screens**: [screens/html.md](screens/html.md)

> **As an example**: the example of a `rendering = "webview"` surface → [plugin-development](../../dev-guide/plugin-development.md#surface-kind--rendering-3-종).

## Purpose

Provides the **`html` surface kind** for viewing HTML / web content. `rendering = "webview"` — drawn with tasty's **native WebView overlay** (the host synchronises the URL per surface).

## Internal behaviour

- **surface_kind `html` (webview)** — a `RemoteSurface` marker in the host tree; the actual content is the native WebView overlay. The URL is identified via the surface's `webview_url()`.
- **File handler** — `handler` `open_surface{surface_kind:"html"}`. The `detector "html"` is **kept by the host** (`default-file-format.toml`) — so extension recognition survives even when the plugin is disabled. Opening an HTML file yields this surface.
- **cli** — `tasty html open …`. `html.*` IPC (URL setting etc. — `webview.set_url`).

## Interface

- **User**: open an HTML file → html surface (WebView).
- **AI agent**: `tasty html …` CLI / `html.*` IPC. Surface creation is [work-area](../../features/work-area/index.md) (`--type html --url …`).

## Non-goals

- The WebView overlay synchronisation mechanism — host (gpu/webview) implementation.
- The surface placement/creation domain — [work-area](../../features/work-area/index.md).

## Acceptance Criteria

- [ ] Given the html plugin is enabled When `tasty new tab --type html --url <u>` Then a WebView surface shows that URL.
- [ ] Given an HTML file is opened Then it opens as an html surface.
- [ ] Given the plugin is disabled Then the host keeps the `html` extension detector.

## Screens

- [screens/html.md](screens/html.md) — the WebView surface.
