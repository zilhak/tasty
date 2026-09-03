<!-- source-hash: 2a2542313084 -->
# Plugins

Many of tasty's features are provided as **plugins** — a separate process declares its contributions (surface kinds, tool items, CLI, file handlers, …) in a manifest, and the host integrates them within the granted permissions. This document is the single source for *what a plugin is and along which axes it is classified and integrated*. The behaviour of each bundled plugin is in [`plugins/`](../plugins/index.md); how to build one is in the dev-guide (below).

## Two classification axes

A plugin is classified independently along a **distribution axis** and an **integration axis**. (E.g. `com.tasty.markdown` is distribution = bundled, integration = webview surface kind + file handler.)

### Distribution axis — who installs/owns it

| Category | Location | Listed | Installed by | Replaceable |
|----------|------|-----------|------|------|
| **host-native** (built in by default) | host code (the main binary) | ✗ | the tasty binary itself | no |
| **bundled plugin** (default plugin) | `~/.tasty/plugins/<id>/` | ✓ | `BUILTINS` auto-installed on first boot | ✓ disable/remove |
| **user plugin** | `~/.tasty/plugins/<id>/` | ✓ | `tasty plugin install <path>` | ✓ |

- **host-native** is host code that does not go through the plugin mechanism. Most viewers have moved to bundled, but the `explorer` surface kind was moved back from plugin → host-native in T11 (see [hierarchy.md](hierarchy.md)) — currently the only host-native entry. (A category used only when users must not perceive it as a plugin and any room for replacement must be closed off at the source.)
- **bundled plugin** ships with tasty and is auto-installed on first boot, but afterwards follows the same lifecycle as an external plugin (enable / disable / remove / permissions). Once removed it is pinned in `removed_builtins` and never reinstalled.
- **user plugin** is an external plugin the user installed. The host merely does not recognise it as an auto-install target; directory and lifecycle are identical.

When it is hard to decide for a new addition, review **bundled as the default** (leaving room to disable is the safer side). Detailed category criteria and the `BUILTINS` auto-upgrade procedure are in the dev-guide (below).

### Integration axis — what it contributes to the host

A plugin declares its contributions in the manifest (`tasty-plugin.toml`) under `[[surface_kinds]]` / `[contributes]`. The main kinds:

| Contribution | What | Examples |
|------|------|----|
| **surface_kind** | Registers a new Surface kind. `rendering` decides who draws it | markdown / image / html / mesh-demo |
| **tool** | Adds a [tools menu](../features/tools-menu/index.md) item (`ui.tool_item`) | clipboard-viewer / git-viewer |
| **popup** | Registers a host popup (`ui.popup`) | clipboard-viewer / git-viewer |
| **cli / ipc_namespace** | Adds `tasty <prefix> …` CLI + IPC methods | claude / codex / html / image / markdown |
| **detector / handler** | File extension → surface mapping (opening files) | markdown / image / html |
| **settings_pages** | Adds a plugin page to the [settings window](../features/settings/index.md) (`ui.settings_page`) | markdown / html / claude / codex |
| **commands** | Commands for the command palette / shortcuts | clipboard-viewer / git-viewer |
| **event_subscribe / hooks** | Subscribes to host events / pre·post hooks | claude · codex (`surface.closed`) |

#### `rendering` of a surface_kind — who draws it

Surface kinds split again by **who renders the content** (→ [work-area Surface kinds](../features/work-area/index.md#surface-종류)):

- **`rendering = "egui-mesh"`** — the plugin tessellates egui in its own process and the host composites the mesh (ADR-0028). Bundled-only allowlist + api_version gate. Examples: image, mesh-demo (plus only the two confirmation popups of markdown — large-file / open-file — not its body).
- **`rendering = "webview"`** — drawn with the host's native WebView overlay. Examples: html, markdown ([ADR-0065](../adr/0065-markdown-webview-render-channel.md), Stage B — the body).
- **(default)** — the plugin process draws directly. The host keeps only a `RemoteSurface` marker in the tree and receives content through the plugin UI DSL. No bundled plugin uses this mode today (explorer was the example, but was promoted to host-native — see [hierarchy.md](hierarchy.md)) — all have migrated to `egui-mesh` or `webview`.

## Permissions

A plugin declares the permissions it needs in the manifest's `permissions`, and makes **host IPC calls** only within what the host has granted (the permission gate blocks host API calls, not OS resources — [plugin-permissions](../dev-guide/plugin-permissions.md)). The tokens are defined once, in the `Permission` enum of `crates/tasty-plugin-manifest/src/types.rs`.

**Tokens without scope**:
`surface.read` · `surface.write` · `fs.read` · `fs.write` · `clipboard.read` · `clipboard.write` · `notification` · `process.spawn` · `terminal.spawn` · `terminal.write` · `terminal.read` · `network` · `memory.read` · `memory.write` · `memory.secret` · `approval` · `telemetry` · `agent` · `ui.tool_item` · `ui.popup` · `ui.settings_page` · `window.spawn` · `file_handler.define` · `hook_handler.define`

**Tokens with scope** (`<name>:<scope>`):
`ipc.invoke:<prefix>` (call another plugin's namespace) · `ext:<plugin_id>` (extend another plugin) · `file_handler.extend:<id>` · `file_handler.handle:<id>` · `hook_handler.handle:<id>`

The grant/display UI and management are in [plugin-system](../features/plugin-system/index.md); the permission model and the procedure for adding a token in [dev-guide/plugin-permissions](../dev-guide/plugin-permissions.md); handling sensitive data in [dev-guide/plugin-sensitive-data](../dev-guide/plugin-sensitive-data.md).

## Related

- **Management / install / permissions UI** (user feature) → [`features/plugin-system/`](../features/plugin-system/index.md)
- **Behaviour of each bundled plugin** → [`plugins/`](../plugins/index.md)
- **Authoring guide** → [dev-guide/plugin-development](../dev-guide/plugin-development.md) (per contribution type, citing bundled plugins as examples)
- **Permission model / sensitive data** → [dev-guide/plugin-permissions](../dev-guide/plugin-permissions.md) · [dev-guide/plugin-sensitive-data](../dev-guide/plugin-sensitive-data.md)
- **Surface kinds and render dispatch** → [`features/work-area/`](../features/work-area/index.md#surface-종류)
