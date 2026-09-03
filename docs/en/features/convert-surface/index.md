<!-- source-hash: 272db029b434 -->
# Convert surface

- **Status**: Implemented
- **Actors**: local user (the `Alt+'` popup) · AI agent (per-kind IPC — no generic convert)
- **ADR**: [ADR-0043](../../adr/0043-convert-input-popup-capability.md) (the convert routing capability for kinds that need file input)
- **Code**: the `ConvertSurface` intent (`src/intent/surface.rs`), the popup `src/adapters/ui/popup/convert.rs`
- **Screens**: the convert popup (`PopupScope::Surface`)

## Purpose

Replaces the *kind* (Terminal/Markdown/Explorer/Image/…) of an open surface in place. Changes only the content type while keeping the current position and layout, without creating a new tab.

## Internal behaviour

### User trigger

- The `convert_surface` shortcut (default `Alt+'`) → the Surface-scope popup. The type-switch button in the centre of an Empty surface opens the same popup. The `convert_to_markdown` direct-conversion shortcut (no default).
- Popup entries are **enumerated dynamically** from the `SurfaceKindRegistry` (built-in + plugin-provided kinds). System kinds such as `empty` are excluded via `HIDDEN_KINDS`. The entry equal to the current type is checked + disabled. `dag_graph` ([screen](../agent-collaboration/screens/dag-graph-surface.md)) has no file input so it takes the immediate-conversion path with empty params — no separate branch.
- Keyboard navigation (Up/Down+Enter), immediate selection by the kind's first letter. **Convert routing is decided by registry capability** (the host does not hard-code kind names, [ADR-0043](../../adr/0043-convert-input-popup-capability.md)): `terminal` is the host PTY-spawn-only path; a kind declaring the `convert_requires_input` capability (e.g. markdown) first opens the file-input popup of the plugin that owns that kind (`convert_input_popup`; in-place conversion carries `surface_id` in the context); every other kind converts immediately with empty params.
- **Individual surface replacement principle**: only the target surface's implementation is replaced; the tab layout and other surfaces are unaffected.
- **cwd carry**: after `cd /foo/bar` in a terminal, converting to Explorer makes the Explorer root `/foo/bar` (no fallback to the host's starting cwd) — the [surface-cwd invariant](../../architecture/invariants/surface-cwd.md). The same in a mirror (remote attach) workspace — when the convert is forwarded to the remote the cwd travels with it, and without a forwarded value the remote resolves it directly from the target surface's real PTY ([§3-1](../../architecture/invariants/surface-cwd.md)).

### Agent

- **There is no generic `surface.convert` IPC/CLI.** The convert popup is a user-shortcut-only UI — in both release and debug there is no path that opens the popup (no reproduction of user input).
- Per-kind IPCs use the same mechanism internally (`DomainIntent::ConvertSurface`) — e.g. `image.open{surface_id, path}` converts the target to the image kind.
- When a surface of a new type is needed, the default path for an agent is to **create one** via the `type` of `tab.create`/`split`.

## Non-goals

- Preserving/merging the surface state from before the conversion — the previous implementation is released from memory (terminal → another kind → back to terminal does not return to the previous shell session).
- Focus movement on conversion (focus independence) · agent triggering of the release popup.

## Related

- [work-area](../work-area/index.md) (Surface kinds) · [surface-cwd invariant](../../architecture/invariants/surface-cwd.md) · [popup-implementation](../../dev-guide/popup-implementation.md)
