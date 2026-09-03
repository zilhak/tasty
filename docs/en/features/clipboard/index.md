<!-- source-hash: 13af56997ce0 -->
# Clipboard

- **Status**: Implemented
- **Actors**: local user (copy/paste are user actions)
- **ADR**: [ADR-0009](../../adr/0009-plugin-sandbox-deferred.md) (the viewer's plugin direct-read model)
- **Code**: system clipboard `arboard`; copy/paste/selection = `src/view/main/`
- **Screens**: the current-clipboard viewer is the [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md)

## Purpose

**Copy/paste/selection** of terminal text and OSC 52 clipboard setting. Copy/paste are user actions so they give toast feedback, but the agent (IPC) and OSC 52 paths do not touch the user's visual state (the [toast](../../design/systems/toast.md) trigger policy).

> **The history feature was removed.** The clipboard history the host used to accumulate by polling the OS clipboard (in-memory `ClipboardHistory` + a DB table + the `tasty clipboard` CLI + the `tool.clipboard.*` IPC + the `clipboard.copied` event) is all gone. Now, *with no history accumulation*, only the current clipboard contents are shown read-only by the [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md).

## Internal behaviour

### Copy / paste — via KeybindingSettings

Acts when one of the `copy`/`paste` binding lists matches (multiple bindings). Defaults differ per OS (Win `ctrl+c`/`ctrl+v`, Linux `ctrl+shift+c/v`, macOS `alt+c/v`). Binding editing is [keybindings](../keybindings/index.md). Position-based mapping is [key-mapping](../../design/policies/key-mapping.md).

- **Soft-wrap-aware copy**: lines the shell auto-wrapped to the width are joined into one line on copy; real hard newlines are preserved.
- **Paste**: bracketed paste (DECSET 2004) supported. If there is no text but an image, it is saved as PNG and the path is pasted (so an AI agent can reference the image).
- **Ctrl+C guard after paste (500 ms)**: a Ctrl+C within 500 ms after a paste is ignored (no SIGINT, no copy) — preventing the accident of wiping input through a typo on the key next to Ctrl+V; a toast when ignored.

### Text selection

Mouse drag (Normal) / double-click (Word) / triple-click (Line) / `Ctrl+v` in vi copy mode (Block). Selection crosses screen↔scrollback and handles full-width (CJK) two-cell widths exactly. The vi-style keyboard copy mode (the `enter_copy_mode` action) provides hjkl movement · visual selection · `/`·`?` search · `y` copy.

Mouse selection works by default **only on screens with mouse tracking off**. When an app turns on mouse tracking (DECSET 1000/1002/1003) (vim `:set mouse=a`, htop, Claude Code etc.), the mouse is fully delegated to the app and a plain left-click drag is reported to the app — rationale: [ADR-0019](../../adr/0019-mouse-button-reporting-app-delegation.md).

**Even with tracking ON, `Shift`+left-click drag selects text locally** (the xterm/iTerm2 standard modifier bypass). Whether Shift is held is judged once at press time and kept until release, so releasing Shift mid-drag does not break the selection. `Shift`+double/triple-click selects word/line. After selecting, the copy shortcut copies to the clipboard — so even over a tracking app a mouse selection path is open besides keyboard vi copy mode. A plain left click is delegated to the app as-is, so no regression (the right-click `Shift` bypass is the same pattern in [ADR-0022](../../adr/0022-shift-rightclick-context-menu-bypass.md)).

### OSC 52

**Write (set)**: a terminal program can set text on the system clipboard via OSC 52 (termwiz `SetSelection` → reflected through arboard). Not an action the user pressed, so **no toast**.

**Read (query)**: the `OSC 52 ; c ; ? ST` clipboard read query is gated by the settings toggle `general.allow_clipboard_read` (default **off**). When off, **no reply** (not a single byte is emitted) — blocking an arbitrary program inside the terminal (including remote/SSH processes) from silently exfiltrating the local clipboard (passwords, tokens) (the xterm/iTerm family policy). When on, the system clipboard is base64-encoded and returned as `OSC 52 ; c ; <base64> ST`. Path: the terminal crate only fires a `TerminalEventKind::ClipboardQuery` event (unaware of settings and clipboard) → the host (`Core::drain_terminal_events`) gates, reads and encodes, then `send_bytes` to that surface's PTY. The settings UI is in the Terminal › TUI section (toggle + a bordered warning callout right below). No toast.

### Copying text selections in egui-mesh plugins (the `egui_copy` capability)

For a kind that draws text with the plugin's own egui `Context` rather than the host's (`rendering = "egui-mesh"`), declaring `egui_copy = true` in the manifest makes the copy shortcut (the `KeybindingSettings` binding above) be forwarded to that surface as a `Copy` wire event (`src/adapters/ui/input/shortcuts/copy_paste.rs` → `src/view/main/egui_mesh.rs`). The host's own top-level egui `Context` holds no plugin widgets, so it cannot be the target — it must be sent to the focused egui-mesh surface itself.

The plugin side (`tasty-plugin-sdk`) maps this wire event to `egui::Event::Copy` and feeds it into its own `Context::run`. When egui's built-in select-and-copy logic (selectable labels, `TextEdit` etc.) produces text, the plugin retrieves that value with `EguiMeshSurface::take_copied_text()` and writes it to the OS clipboard **directly in its own process** ([ADR-0009](../../adr/0009-plugin-sandbox-deferred.md) — the write counterpart of the clipboard-viewer plugin's read precedent, no host round trip). The mechanism itself remains in host code ([`src/view/main/egui_mesh.rs`](../../../src/view/main/egui_mesh.rs) etc.), but since `markdown` switched to webview ([ADR-0065](../../adr/0065-markdown-webview-render-channel.md)) no bundled plugin currently declares it — a webview surface's native WebView handles Ctrl+C itself, so the wire event is unnecessary there.

### The current-clipboard viewer

The contents currently on the system clipboard are shown as a popup by the [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md). **The plugin process reads directly with `arboard`** without a host backend ([ADR-0009](../../adr/0009-plugin-sandbox-deferred.md) — a plugin is a non-sandboxed OS process, so the host cannot block OS clipboard access, and a one-shot read does not go through the host). No history accumulation or re-copy.

## Interface

- **User**: copy/paste/selection (above); the current clipboard contents via the plugin viewer popup.
- **AI agent / CLI**: a one-shot clipboard **read** is each agent process's own direct-access territory (ADR-0009) — the host exposes no read IPC. **Write** is exposed by the host as the `clipboard.set_text` IPC (`Permission::ClipboardWrite`)/CLI `tasty clipboard set-text <text>` — [remote-screenshot-clipboard](../remote-screenshot-clipboard/index.md) uses this path to put a remote mirror capture on the remote clipboard.

## Non-goals

- Clipboard **history** accumulation/re-copy — removed (see Purpose above).
- The current-clipboard **viewer UI** — provided as a popup by the built-in [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md).
- The IME (Korean/CJK) input pipeline — a separate area.

## Related

- [clipboard-viewer plugin](../../plugins/clipboard-viewer/index.md) · [keybindings](../keybindings/index.md) · [settings](../settings/index.md)
