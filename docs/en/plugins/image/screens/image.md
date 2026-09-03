<!-- source-hash: 437302482eb8 -->
# Image surface screen

- **Parent plan**: [../index.md](../index.md)
- **Visual source**: plugin egui-mesh self-render (bitmap = egui texture) — the image surface design in `design-system/` (if any), to be vendored.

The image viewer / paint surface that opens inside a [work area](../../../features/work-area/screens/work-area.md) tile.

## Trigger

Opening an image file, creating/converting to an `image` surface, or a blank canvas.

## UI element inventory

- **Image view** — displays the loaded image (fit/zoom etc.).
- **Blank canvas** — a paint board started without a file.
- The tab display name is the file name (default "Image" for a blank canvas).

## Visuals per state

- Loaded / blank canvas / load failure.

## Design token mapping

`crates/tasty-plugin-image/src/render.rs::draw` draws the top control bar + image area in its own egui
`Context` and the host composites the mesh ([ADR-0030](../../../adr/0030-image-egui-mesh-bitmap-texture.md)).
Toolbar button icons are `tasty-icons` build-time baked vectors ([ADR-0036](../../../adr/0036-plugin-icon-buildtime-bake-tasty-icons-single-source.md)),
zoom uses text buttons:

| UI element | Token | Notes |
|---|---|---|
| canvas background | `bg-sidebar` | host `mantle` |
| toolbar icon buttons | `surface-raised` + `border-default` · glyph tint `text-primary` (disabled=`text-muted`) | chevron-left/right (prev/next) · refresh · edit · plus (new) — `tasty-icons` baked vectors, fixed 24×20. The no-image state has only refresh/new |
| zoom controls | `surface-raised` + `border-default` | `Fit`/`+`/`-` **text** buttons (30×20·24×20) + `%` label |
| file name label | `text-muted` · `font-size-caption` | `subtext0` |
| zoom percentage | `text-muted` · `font-size-caption` | right-aligned |
| loaded picture frame | `bg-panel` + `border-default` | fit-to-window |
| fallback / empty notice glyph | `IMAGE` glyph · `text-muted`/`text-disabled` | `tasty-icons` SURFACES |
| "No image" text | `text-muted` | plugin `no_image` |

## Gallery specimen

`crates/tasty-gallery/src/catalog/components/image_viewer.rs` — Layouts › `Content viewers` ›
`Image surface / canvas`. The two states viewer (picture fit) / no-image (fallback glyph) are transcribed with tokens.
Three-way mapping: [design-gallery-mapping.md](../../../design/systems/design-gallery-mapping.md#surface-viewers-layouts).

## Visual source

The plugin self-renders with the Theme tokens forwarded by the host. To be replaced with a link after the design-system is vendored.
