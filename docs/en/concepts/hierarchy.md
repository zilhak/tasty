<!-- source-hash: 97f0959c947f -->
# Structural hierarchy

tasty's screen structure consists of one object hierarchy plus **two layout levels** on top of it. Every window and surface feature document uses these terms. (The *actors* who look at the screen are in [actors.md](actors.md).)

## Window and View

- **`winit::window::Window`** — the window resource the OS provides (frame / event source / render surface). Identified by a winit `WindowId`.
- **`View`** — tasty's representation of a window. An object bundling that window's *kind + content + behaviour* (render / events / modality), owning the winit Window through an `Arc`. **1 View : 1 Window.**

> The old tasty `Window` trait was confused with winit's `Window`, so it was **renamed to `View`** (`WindowBase` → `ViewBase`, `*Window` → `*View`). **There is no tasty-side `Window` trait** — today `Window` refers only to the winit OS window. So *View = the tasty-side word for a window*. In the same spirit the old gloss **"terminal window" is no longer used** — the formal name is `MainView`.
>
> **Guidance for AI agents**: in the ubiquitous language, "window" is ambiguous — the user may mean a *View* (a tasty-side window such as `MainView`) or an *actual winit OS window*. When the term comes up in conversation or a request, do not assume which one; **asking once to confirm is recommended.**

## Kinds of View (= kinds of window)

Implementations of the `View` trait are the kinds of window. **`MainView` is one of them** — the View that hosts terminals. Each implementation is a separate OS window (winit Window), and the engine manages them uniformly as a `HashMap<WindowId, Box<dyn View>>`.

| Implementation | Family (supertrait) | What |
|--------|-------------------|------|
| **`MainView`** | implements `View` + `sealed::Sealed` directly | The main window hosting the sidebar and workspaces. Several may exist. ← the main subject of this document |
| `SettingsView` / `PluginsView` / `QuitView` | `ModalView` | Modal windows — one globally, blocks input while active |
| `PresetView` | implements `View` + `sealed::Sealed` directly | Editor window — modeless |

(**Engine** = entry point + server. Owns the IPC port, manages the lifecycle of every window. **Headless, only Engine + `CoreState` run, with no View (GUI)** — the structural hierarchy below is the domain of that `CoreState`, so it is built even without a GUI.)

## Structural hierarchy = the CoreState domain (built even without a GUI)

The containment hierarchy — this is the substance of the "structural hierarchy". It is **the domain tree of `CoreState`** and **is built and operates without a GUI (View)**. Headless, boot creates `CoreState` directly so Workspace/Pane/Tab/Surface and their PTYs are alive, and **`MainView` is merely the shell that hosts and projects this `CoreState` when a GUI is present** (→ headless, behaviour-first in [identity](../identity.md); domain behaviour in [`features/work-area/`](../features/work-area/index.md)).

```
CoreState   domain tree — built and running even headless
└── Workspace   top-level container. Several; switched from the sidebar.
    └── Pane    independent tab bar. Positioned by the **upper layout** (fixed regardless of tab).
        └── Tab        owns a **lower layout** (arrangement of Surfaces).
            └── Surface   the leaf. Has a type (Terminal/Markdown/…).
```

In the GUI, `MainView` (a View) hosts and renders this `CoreState`. With several windows, each MainView has its own `CoreState`. Headless, only `CoreState` exists, with no MainView.

- **Workspace** — the top-level container of the domain. (In the GUI) one MainView holds several workspaces and switches between them from the sidebar.
- **Pane** — a screen region with its own independent tab bar. Its position is decided by the **upper layout** and stays fixed regardless of tab switches. A tasty-specific design with no counterpart in tmux/iTerm2.
- **Tab** — one tab inside a Pane. Holds the **lower layout** of Surfaces inside it. Switching tabs switches the whole lower layout with it.
- **Surface** — the leaf container inside a Tab. Has a **type** (below) and a unique `surface_id`. Close / focus / list behave identically regardless of type.

## Two layout levels (tasty's core design)

Existing tools have exactly one split policy — in tmux the split is fixed to the window (survives tab switches); in iTerm2 the split belongs to the tab (changes when switching tabs). tasty provides **both**:

- **Upper layout (tab-independent)** — arranges **Panes** inside a Workspace. Switching tabs does not change this split. It carves the screen into physical regions, each of which switches tabs independently.
- **Lower layout (tab-dependent)** — arranges **Surfaces** inside a Tab. Switching tabs switches this split too. Several terminals side by side within one tab.

Example: an upper-layout left/right Pane split — the left is dedicated to Claude Code, the right has several tabs (logs/build). Switching the right-hand tabs does not affect the Claude on the left.

## Surface types

| kind | Origin | Content | Render |
|------|------|--------|------|
| `terminal` | **host built-in** | Shell session (PTY attached) | GPU shader |
| `empty` | **host built-in** | Empty surface (type-conversion buttons); placeholder for a deferred terminal | egui |
| `markdown` | `com.tasty.markdown` plugin (`rendering=webview`) | Markdown viewer | Native WebView overlay — the plugin produces a sanitised HTML document (`RemoteSurface`) |
| `image` | `com.tasty.image` plugin (`rendering=egui-mesh`) | Image viewer/editor | Plugin self-rendered mesh (bitmap = egui texture) |
| `explorer` | **host built-in** (T11) | File explorer | egui |
| `html` | `com.tasty.html` plugin (`rendering=webview`) | HTML/web viewer | Native WebView overlay (`RemoteSurface`) |

Three origins: **host built-in** (`register_builtin_kinds`) / **egui-mesh plugin** (the plugin declares `rendering=egui-mesh` and is on the host allowlist; the host composites the mesh the plugin process rendered itself) / **webview plugin** (`rendering=webview`, a RemoteSurface stand-in plus a native WebView overlay). Details and behaviour per kind: [`features/work-area/`](../features/work-area/index.md#surface-종류).

## Related

- [actors.md](actors.md) — the actors that use this structure (local / AI / remote)
- Overlays inside a View: [`design/systems/popup.md`](../design/systems/popup.md) · [`design/systems/toast.md`](../design/systems/toast.md). For the modal family of Views see the "Kinds of View" table above.
