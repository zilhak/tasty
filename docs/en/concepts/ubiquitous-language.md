<!-- source-hash: 6f5830249fd4 -->
# Ubiquitous language (unified glossary)

tasty's code, docs, and IPC/CLI surface all use the same vocabulary. This document is the **glossary (index)** — the *body* of each definition lives in the relevant concept document; here there is only a one-line definition plus a canonical link. Follow the link when you need depth.

> Using a term wrongly breaks consistency across code, docs, and API. In particular, never confuse **Window/View**, the **Workspace/Pane/Tab/Surface** hierarchy, **upper/lower layout**, or the **Modal / Popup / Toast / Banner / Modifier-hint overlay** distinctions.

## Canonical documents

| Area | Canonical | Terms covered |
|------|------|-------------|
| Actors | [actors.md](actors.md) | Local user · AI agent · Remote user · Occupancy |
| Structure | [hierarchy.md](hierarchy.md) | Engine · View · Workspace · Pane · Tab · Surface · two layout levels |
| Plugins | [plugins.md](plugins.md) | Distribution/integration axes · surface_kind · permissions |
| attach | [`../dev-guide/attach-behavior.md`](../dev-guide/attach-behavior.md) | server/client · mirror · lock |
| Remote connections | [`../features/remote-profiles/index.md`](../features/remote-profiles/index.md) | Remote profile · Passkey · kind |
| Hooks | [`../features/hooks/index.md`](../features/hooks/index.md) | Surface hook · Global hook · hook handler registry |
| Claude plugin | [`../plugins/claude/index.md`](../plugins/claude/index.md) | Claude session profile · reboot · spawn/respawn |

## One-line definitions

### Actors (→ [actors.md](actors.md))

- **Local user** — the person using the GUI directly on this machine. Owner of focus. Needs no occupancy. The only actor who can break an occupancy.
- **AI agent** — an AI operating tasty over IPC/CLI. Targets addressed directly by ID. **Acts without occupancy by default**, but can take one (soft/hard) when needed (e.g. the soft occupancy of a `terminal` child terminal). Follows the **isolation contract** (side effects never touch user state).
- **Remote user** — a person connecting by attach from across SSH. Behaves like an AI agent (connection-based) and **must pass through the gate of occupancy**.
- **Occupancy** — a persistent, visible relationship an actor (remote user | AI agent) declares over a surface/workspace. Two tiers (ADR-0040): **soft (advisory marker, writes allowed) / hard (exclusive + other actors read-only; remote attach is the example)**. Regardless of tier, target → occupant is 1:1 (exclusive) and actor → targets is 1:N. Released only by self-release or the local user's force-detach.

### Remote connections (→ [features/remote-profiles](../features/remote-profiles/index.md))

- **Remote profile** — a general-purpose connection descriptor tagged with a type (`kind`, an open string). Holds no secrets; references a Passkey by name only. Two layers (ADR-0032): **`ssh`** = pure connection info; **`tasty-attach`** = an attach spec (references an ssh profile via `ssh_ref`, or inline, plus remote_tasty / port_mode / port_file). attach is a **consumer** that reads the tasty-attach kind — "storing an address (ssh) ≠ an attach spec (tasty-attach)".
- **Passkey** — a separate named credential store. `kind = path` (file reference) | `inline` (materialised as a 0600 file). At rest it is always a file path (zero secrets in toml). Values are viewable only via Reveal in the local GUI; permanently masked from IPC/agents ([ADR-0016](../adr/0016-passkey-store-path-convergence.md)).
- **Unregistered type** — a `kind` claimed neither by the core built-ins (ssh/smb) nor by any installed plugin. Registration is allowed, but flagged with a yellow badge.

### Structure (→ [hierarchy.md](hierarchy.md))

- **Engine** — entry point + server. Owns the IPC port, manages the lifecycle of every View. **Headless, only Engine + `CoreState` run, with no View.**
- **Window** — the winit OS window resource (`winit::window::Window`). There is **no** tasty-side `Window` type — the word means the OS window only.
- **View** — tasty's representation of a window (kind + content + behaviour). Owns the winit Window. **1 View : 1 Window.** Implemented by `MainView` / `SettingsView` / ….
- **CoreState** — the domain tree of the structural hierarchy (Workspace … Surface). **Built even without a GUI**; in the GUI, `MainView` hosts and projects it.
- **Workspace** — the top-level container of the domain. Switched from the sidebar.
- **Workspace category (sidebar folder)** — a **grouping tier** for workspaces (a sidebar section). Toggled with the `workspace_categories_enabled` setting. The reserved category **`normal`** (id `0`, pinned at `categories[0]`, cannot be renamed or deleted) always exists and is the default home of unassigned workspaces. Category *CRUD, reordering, and membership changes* are agent operations (IPC/CLI both, release) — *selection (active) and collapse toggling* are user UI state (not exposed over IPC). Canonical: [`features/workspace-category`](../features/workspace-category/index.md).
- **Pane** — a region with its own tab bar. Positioned by the **upper layout** (fixed regardless of tab). tasty-specific.
- **Tab** — one tab inside a Pane. Holds the **lower layout** of Surfaces (switched together with the tab).
- **Surface** — the leaf container. Has a `surface_id` + a **kind (type)**. Close / focus / list are identical regardless of kind.
- **Upper layout / lower layout** — arrangement of Panes (tab-independent) / arrangement of Surfaces (tab-dependent). Providing **both** levels is tasty's core design.

### Inside a View (→ hierarchy.md, `design/systems/`)

- **Modal** — a form of View, one globally, that blocks input while active (not a separate entity). `SettingsView` / `QuitView` / `PluginsView`.
- **Popup** — a virtual window inside a View (title bar + content, dragging, z-order). Has a scope. Details: [`design/systems/popup.md`](../design/systems/popup.md).
- **Toast** — a transient notification inside a View. Takes no focus, consumes no input. Fired **only by user actions** (agent IPC never fires one). Details: [`design/systems/toast.md`](../design/systems/toast.md).
- **Banner** — a persistent, interactive overlay floating at the top of its parent scope that provides **info + an action**. Takes no focus but **consumes the mouse and has buttons inside** (a fourth concept that fits neither Toast nor Popup). TTL, queue (1 + at most 5 waiting per scope), tiered z-index. Fired **only by user actions** (agent IPC never fires one). Details: [`design/systems/banner.md`](../design/systems/banner.md).
- **Modifier-hint overlay** — an overlay that appears with a 200 ms fade when a modifier is held (default **500 ms**, **2000 ms for Shift alone**), lists the shortcuts whose combination **contains (is a superset of)** the held one, and **disappears the instant the key is released**. Narrowing the combination narrows the list immediately (Ctrl → Ctrl+Shift). **Never takes keyboard focus** (input still goes to the terminal) and **consumes only the mouse** (drag to move, edge/corner resize, X to close). Neither a Popup (focus / title bar / z-order), nor a Toast (non-interactive TTL), nor a Banner (top-pinned action) — a fifth concept: **hold lifetime + mouse-interactive + focus-less**. The hold state reflects only winit `ModifiersChanged` (real user input) — cannot be forced via IPC/CLI (principle 1). With the `enabled` setting off it never appears. Geometry (pos/size) persists to `Settings::modifier_hint` when the user moves or resizes it. Details: the modifier-hint section of [`design/systems/design-token-mapping.md`](../design/systems/design-token-mapping.md) · content model in `src/adapters/ui/input/shortcuts/modifier_hint.rs`, body in `src/adapters/ui/modifier_hint_overlay.rs`.
- **Marker overlay** — an overlay that **lays an independent floating shape (ring/glow) at top-most z over a coordinate (rect)** without touching the target widget's border. What decisively distinguishes it from Modal / Popup / Toast / Banner / Modifier-hint: **a pure geometric marker with no message or severity model at all** (contrast Banner = info + action, Toast = message) — a sixth concept that, when external logic (the tutorial runtime) injects coordinates, simply draws a ring there and carries no meaning. `pointer-events: none` (marker and scrim pass clicks through); interaction is handled only by the **callout** beside it. Coordinates are re-derived every frame from `LayoutContext` / `terminal_rect` / `tab_bar_height` (nothing static goes stale). Fired **only by user actions** (tools menu → enter tutorial, Next click to advance — no agent IPC/CLI trigger, same family as Toast / Banner / Modifier-hint · principle 1). The tutorial is currently its only producer. Details: [`features/tutorial`](../features/tutorial/index.md) · body `src/adapters/ui/tutorial/`.
- **Fullscreen stage** — an **independent surface** that monopolises the whole window. Not an enlargement of an existing element: it exists **in parallel** with the Workspace/Pane/Tab/Surface tree and holds **separate data** with no internal relationship to what is behind it ("this popup, fullscreen" = a separate instance of the same shape composed on the stage). While the stage is up, what is behind is hidden, so it is not redrawn; it is redrawn on exit. **At most one per window** (independent per window when there are several); only what is declared in the static table (`StageDef`) can go on stage; never persisted. Fired **only by user actions** (entry is a popup title-bar button — the only agent surface is the debug-only `debug.fullscreen.*`, absent from release; Toast / Banner / marker family · principle 1). A seventh concept that fits neither Modal (input-blocking View) nor Popup (virtual window) — it does not compete for focus or z-order; it swaps the frame itself. Details: [`design/systems/fullscreen-stage.md`](../design/systems/fullscreen-stage.md) · rationale [ADR-0082](../adr/0082-fullscreen-independent-stage.md).
  - **Do not confuse with Zoom** — in tasty, `Zoom` is already taken to mean **UI scale** (Settings › Keybindings › Zoom). The tmux-style name "pane zoom" is not used; the unified terms are **fullscreen / stage**.
- **Workspace status bar** — a fixed strip that always occupies the bottom of the work area (`bottom_inset`, symmetric with the title bar's `top_inset`). Shows the focused surface's context plus quick actions on the right (palette, theme). A GUI-only display widget (no agent surface). Canonical: [`features/workspace-status-bar`](../features/workspace-status-bar/index.md).

### Surface attention (→ [`features/surface-highlight`](../features/surface-highlight/index.md))

- **Attention** — **producer-neutral shared state** indicating that a surface is "awaiting acknowledgement (calling for attention)" (CoreState `attention: AttentionStore`, surface id → `{ kind, raised_at }`). **A separate concept from Notification** — an attention record is not itself a notification-panel item (whether it appears in the panel is decided by the per-kind policy `effects_of().panel_item`, and the actual panel item is created by the producer calling `NotificationStore` directly). Cleared automatically when the surface gains **focus at actual render time** (`gpu.rs`, not agent-injected → safe under inviolable principle 1). **Several producers** (toast notifications, completion, Claude hooks, OSC 133 command completion) can raise it — it belongs to no particular producer. Currently one kind, `Completion` (planned: `NeedsInput`).
- **Highlight** — the **View-layer name** under which Attention is projected on screen (three effect channels: border emphasis + tab-title emphasis (yellow) + a count badge on the right of the owning workspace). The consuming function/type names (`draw_surface_highlights`, `SurfaceHighlightRegion`, `highlight_count`, …) use this name as-is — the Core state name (Attention) and the View display name (Highlight) are deliberately separate. Also distinct from Toast (a transient View overlay): a highlight is persistent state attached to a surface.
- **Completion** — the event/signal "the surface has finished its work" (release-official IPC/CLI: `surface.completion` · `tasty surface completion`). **Merely one kind of Attention (`AttentionKind::Completion`) and one of the producers that raise it** — completion ≠ attention. Legitimate in release because an agent is reporting the result of its own work (same family as PushNotification). If completion-specific effects appear later, the cascade is extended.

### Surface kinds (→ [hierarchy.md](hierarchy.md#surface-타입) · [plugins.md](plugins.md))

- **host built-in** — `terminal` (PTY + GPU shader) / `empty` / `explorer` (moved back from plugin → host-native in T11).
- **egui-mesh plugin** — `image` (the plugin declares `rendering=egui-mesh`; the host composites the mesh the plugin process tessellated).
- **webview plugin** — `html` / `markdown` ([ADR-0065](../adr/0065-markdown-webview-render-channel.md), from Stage B) — `rendering=webview`, drawn with the host's native WebView overlay.

### Claude plugin (→ [plugins/claude](../plugins/claude/index.md))

- **Claude session profile** — the `settings.json` fragment that `tasty claude spawn/respawn/launch/reboot` injects into Claude Code via `--settings <path>`. Specified one of two ways: `--profile-file <path>` (a file directly) or `--profile <name[,name2,...]>` (names registered in the registry below); the two are mutually exclusive. Claude Code reads hooks only once at process start, so injecting `--settings` at this restart point is the only way to attach a new hook to an already-running session — merged **as an addition, not a replacement**, firing alongside tasty's built-in hooks. `reboot` records the attachment (path or names) in surface meta so the next argument-less reboot inherits it; `--clear-profile` detaches.
  - **Claude session profile registry** — the tier that registers profiles by name so they can be attached with `--profile` (`crates/tasty-plugin-claude/src/profile.rs`). `tasty claude profile-register/-unregister/-list/-show/-current`. When two or more names are attached at once, the registered contents are **merged** into one file (`profile_merge.rs`) to avoid the trap that repeated `--settings` flags are last-wins — hook arrays are unioned, objects merged key by key, `permissions.allow` / `deny` unioned with **deny beating allow**, other scalars last-wins (with a warning on conflict), and `permissions.defaultMode` rejected on conflict. It mirrors the shape of the host registries (`src/hook_handler/registry.rs`, …: patch semantics, `<owner>/<short>` ids) but lives inside the plugin because this plugin is the only consumer (no shared types).
  It shares only the word "profile" with the two terms below; they are otherwise unrelated:
  - **Remote profile** (the "Remote connections" section above) — an SSH/attach connection descriptor. Nothing to do with Claude sessions.
  - **surface hook / hook handler** (the "Hooks" section above) — event → handler bindings owned by tasty. A Claude session profile is the other side: the **Claude Code process's own** hook configuration file — it does not go through tasty's hook handler registry.

### attach (→ [attach-behavior.md](../dev-guide/attach-behavior.md))

- **server / client** — the side being occupied (the authoritative PTY owner, only ever accepting on loopback) / the side occupying (absorbs remoteness). "Local/remote" is a **client-side notion**.
- **mirror** — a replica screen the client reconstructs, without a PTY, from the output it receives. GUI mirror = a remote workspace shown in the local GUI as an ordinary workspace.
- **remote** — when the client is across SSH. tasty has no remote protocol of its own and delegates to SSH → release CLI `tasty remote attach`. (Local self-attach is debug-only, [ADR-0007](../adr/0007-attach-targets-remote.md).)
- **SSH delegation** — the whole client-side layer that absorbs remoteness. In tasty's vocabulary, **"SSH" means the act of delegating to the system `ssh` binary, not a protocol implementation** — process spawn, tunnel lifetime, remote port discovery, backoff, and cancellation belong here. The layer lives in the `tasty-ssh` crate (`crates/tasty-ssh/`), consumed by both the CLI and the main GUI. The tunnel is **part** of this layer, not all of it (port discovery, profile re-detection, and interactive connection are not the tunnel).
- **Remote capability** — the layer *on top of* SSH delegation that actually talks to the remote tasty instance — workspace **browse** and **create**. Lives in the `tasty-remote` crate. Keep the three similar names apart: `tasty-ssh` (how to reach it) → `tasty-remote` (what to do once reached) → `tasty-remote-profiles` (a registry that stores where to reach, by name — the "Remote profile" entry above).

### CLI command branches (→ [`crates/tasty-cli/src/dispatch.rs`](../../crates/tasty-cli/src/dispatch.rs))

- **One-shot RPC** — the branch where one CLI command is exactly **one** JSON-RPC request. Send it, print the response, done — the client does not drive a flow. Most commands are here.
- **Client-driven execution** — the branch that does not end with a one-shot RPC and in which **the client holds the flow**. Local file/process manipulation (`tasty port`, `tool passkey`), raw streams (`remote attach`), polling loops (`plugin audit-follow`), and queries over an SSH tunnel (`remote workspaces`) are all here. "Client" means the same client as in the attach section above — the axis emphasised here is **who drives**, not whether there is communication. That is why it is not called "local": half of this branch goes over IPC (several times).

## Mapping to existing terminals

Pane is a tasty-specific design with **no** counterpart in tmux/iTerm2. That is why the split policy has two levels.

| Behaviour | tmux | iTerm2 | tasty |
|------|------|--------|-------|
| Where the split lives | fixed to the window | belongs to the tab | **choose either level** (upper = Pane / lower = Surface) |
| Split on tab switch | kept | switched | upper kept + lower switched |

| tasty | tmux | iTerm2 |
|-------|------|--------|
| Workspace | Session | Window |
| Pane | — | — |
| Tab | Window (tab) | Tab |
| Surface (terminal) | Pane | Pane (split) |

## Code-symbol crosswalk

| Term | Rust symbol |
|------|-----------|
| Engine | `core::Core` + `core::CoreState` |
| Structural domain tree | `core::CoreState` (holds Workspace … Surface) |
| View (base) | `view::ui::View` (sealed trait) |
| View family | `ModalView` supertrait (non-modal implementations implement `View` + `sealed::Sealed` directly) |
| View implementations | `MainView` / `SettingsView` / `QuitView` / `PluginsView` / `PresetView` |
| Upper layout | `PaneNode` (binary tree: Leaf/Split) |
| Pane / Tab | `Pane` / `Tab` |
| Lower layout | `SurfaceLayout` (binary tree: Leaf/Split) |
| Surface | `Surface` trait; plugin surfaces are kept in the host as `RemoteSurface` |
| Popup / Toast / Banner | `PopupDef` + `PopupManager` / `ToastState` + `ToastManager` / `BannerDef` + `BannerManager` |
| Status bar | the `StatusBar` family (`StatusBarData` / `StatusBarAction` / `draw_status_bar`) |
| Length types | `PhysicalPx` / `LogicalPx` (→ [typed-length.md](typed-length.md)) |
| One-shot RPC / client-driven execution | `dispatch::Dispatch::Rpc` / `dispatch::Dispatch::ClientDriven` (+ `ClientCommand`) |

## Related

- [identity.md](../identity.md) — identity and inviolable principles (the *why* behind these terms)
- [typed-length.md](typed-length.md) — the length newtypes
