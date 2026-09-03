<!-- source-hash: abc62d7e978d -->
# Typed length

tasty's internal source **never handles a length as a bare `f32`.** In code where a DPI scale factor is involved, confusing *physical pixels* with *logical pixels* produces position/size bugs that only show up at runtime. So lengths are split into two newtypes, and **code that mixes them directly is a compile error** — the type system blocks DPI mix-ups.

- Definition: `crates/tasty-type-geometry/src/length.rs`
- Enforcement (mandatory): the "Length types" section of [`../../CLAUDE.md`](../../CLAUDE.md) — every new length value must be one of the two.

## The two types

| Type | Meaning | Where it is used |
|------|------|---------|
| `PhysicalPx(pub f32)` | Actual device pixels (after the scale factor) | GPU/wgpu, winit mouse coordinates, `Rect` fields, GPU viewport and scissor |
| `LogicalPx(pub f32)` | DPI-independent logical pixels | egui UI, `Theme` constants, sidebar width, every egui coordinate/size |

Both are `#[repr(transparent)]`, so there is no runtime overhead (zero cost). `Add` / `Sub` / `Mul<f32>` / `Div<f32>` / `Neg` / `*Assign` and `max` / `min` / `floor` / `abs` are defined **only within the same type**, so an expression like `PhysicalPx + LogicalPx` is a type error.

## Conversion — pass the scale factor explicitly

Direct assignment between the two types is impossible. Always go through a conversion function, and pass the scale factor at that point:

```rust
let physical: PhysicalPx = logical.to_physical(scale_factor); // logical → physical (× sf)
let logical:  LogicalPx  = physical.to_logical(scale_factor); // physical → logical (÷ sf)
```

The key point is that the scale factor is a *required argument* of the conversion — it forces you to think "which coordinate space is this?" every time.

## `.value()` only at external API boundaries

External libraries such as egui and wgpu take `f32`. Extract the raw `f32` with `.value()` **only at that boundary**:

```rust
egui::FontId::proportional(th.font_size_body.value());
```

Dropping out with `.value()` in the middle of internal logic to do `f32` arithmetic is an anti-pattern — it throws away the type protection by hand.

## Values that stay `f32`

Anything that is not a length stays `f32` — ratios (ratio / opacity / scale_factor), colour channels, and values extracted right before being handed to an external API.

## When writing new code

1. Never create a bare `f32` field/variable that represents a length.
2. **If the value changes with the scale factor it is `PhysicalPx`; otherwise `LogicalPx`.**
3. Extract `f32` with `.value()` only at external API boundaries.

## Related

- [`../design/systems/theme.md`](../design/systems/theme.md) — every Theme constant is a `LogicalPx`
- Colours follow the same newtype policy as lengths (`GpuRgba` etc.) — see "single colour-creation path" in theme.md
