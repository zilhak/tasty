<!-- source-hash: adf2250fdfb3 -->
# Image (`com.tasty.image`)

- **Status**: Implemented (bundled plugin)
- **Actors**: local user (GUI surface) · AI agent (`tasty image` CLI)
- **Distribution / integration**: bundled · surface_kind (egui-mesh) · file handler — [plugin concepts](../../concepts/plugins.md)
- **Code**: `crates/tasty-plugin-image/` (`main.rs`/`doc.rs`/`render.rs`), registration `src/engine/surface_registry/egui_mesh.rs` (whitelist)
- **Permissions**: manifest `permissions`
- **Decisions**: [ADR-0028](../../adr/0028-plugin-egui-mesh-render-channel.md) (egui-mesh channel) · [ADR-0030](../../adr/0030-image-egui-mesh-bitmap-texture.md) (image mesh-only revision)
- **Screens**: [screens/image.md](screens/image.md)

> **As an example**: the example of an egui-mesh surface drawing **a bitmap texture together with chrome** — the plugin tessellates a mesh in its own egui `Context` and the host composites it (mesh-demo is a pure-widget PoC; image includes a texture). The starting point for a new egui-mesh surface → [plugin-development](../../dev-guide/plugin-development.md#surface-kind--rendering-3-종).

## Purpose

Provides the **`image` surface kind** (viewer + paint) for viewing images and simple drawing. `rendering = "egui-mesh"` — the plugin uploads the bitmap as a texture of its own egui `Context` (the same `TexturesDelta` channel as the font atlas), tessellates it into a mesh together with the chrome, and the host composites. There is no separate Canvas layer ([ADR-0030](../../adr/0030-image-egui-mesh-bitmap-texture.md)).

## Internal behaviour

- **surface_kind `image` (egui-mesh)** — the plugin (`ImageDoc`) owns pixels, edit state and zoom/pan, uploads the original image + edit overlay + floating selection as textures, and draws them together with the viewer/paint chrome (control bar · paint bar · 8 handles · zoom). The host `EguiMeshSurface` stand-in handles only file, display_name and persistence. Loaded from a file or a blank canvas (entering paint mode).
- **File handler** — `detector "image"` (extension rules) + `handler` `open_surface{surface_kind:"image"}`. Opening an image file yields this surface.
- **cli / IPC** — `image.save`/`export_png`/`paste`/`next`/`prev` are handled directly by the plugin (it owns pixel/edit/navigation state); `image.open` (surface conversion) and `image.list` (host surface enumeration) trampoline to the host.

## Interface

- **User**: open an image file → image surface. Use a blank canvas as a paint board.
- **AI agent**: `tasty image …` CLI / `image.*` IPC. Surface creation is [work-area](../../features/work-area/index.md) (`--type image`).

## Non-goals

- The surface placement/creation domain — [work-area](../../features/work-area/index.md).
- Paint editing tool details — design-system / implementation.

## Acceptance Criteria

- [ ] Given the image plugin is enabled When an image file is opened Then it is shown as an image surface.
- [ ] Given `tasty image open --file <f>` Then the active surface converts to the image kind and loads the file.
- [ ] Given a blank canvas Then one can draw on it as a paint board.

## Screens

- [screens/image.md](screens/image.md) — the image viewer / paint surface.
