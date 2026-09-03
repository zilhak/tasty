<!-- source-hash: c2562aba4f5b -->
# Accessibility

- **Status**: Implemented (Phase 1 — manual toggles)
- **Actors**: local user
- **ADR**: none
- **Code**: `AccessibilitySettings` · `ModifierHintSettings` (`tasty-settings`), the toast alpha branch (`src/adapters/ui/toast.rs`), the modifier-hint content model (`src/adapters/ui/input/shortcuts/modifier_hint.rs`) · the overlay body (`src/adapters/ui/modifier_hint_overlay.rs`)
- **Screens**: [Settings window](../settings/screens/settings.md) Accessibility tab

## Purpose

Options turned on directly in Settings → Accessibility tab. **Phase 1 is manual toggles only** — OS auto-detection (Windows ANIMATIONS / macOS NSWorkspace), AccessKit integration, colour-blind palettes and screen-reader labels are Phase 2 and later.

## Internal behaviour

### Reduced motion

`accessibility.reduced_motion: bool` (default false). When active, toast fade-in/out is 0 ms — 100% for the lifetime, 0% immediately on expiry. Terminal content is unaffected (already motionless under the "terminal content animation 0 ms" principle of [theme](../../design/systems/theme.md)).

### Modifier key hints

`modifier_hint.enabled: bool` (default **true**). The second toggle of the Accessibility tab. **Holding a modifier key** shows, with a 200 ms fade (opacity 0.2→1.0) at the bottom of the sidebar, an overlay listing the shortcuts whose combos **contain (as a superset)** the pressed combo, and **it disappears immediately when the key is released**. Narrowing the combo (e.g. Ctrl→Ctrl+Shift) **narrows the list immediately**. The reveal delay is **500 ms** by default, but **1200 ms** for a **Shift-only** hold (Shift is constantly pressed for capitals and symbols and grazed often, so this suppresses the overlay popping up while typing — [ADR-0035](../../adr/0035-modifier-hint-combo-narrowing-and-shift-delay.md)). If, while holding, **a registered tasty shortcut is actually executed**, the delay timer restarts from that point (only while not yet shown — [ADR-0064](../../adr/0064-modifier-hint-reveal-timer-reset-on-shortcut.md)). With `reduced_motion` the fade is skipped but **the delay is kept** (the delay is not motion but a gate against accidental grazes).

A **fifth overlay element** that fits none of Modal/Popup/Toast/Banner — it never takes keyboard focus (input goes to the terminal as-is) and **consumes only the mouse** (move via the drag strip · resize via edge/corner grips · X dismisses for this hold session · vertical wheel scroll of the list). The `modifier_hint_hovered` flag blocks propagation to the underlying surface (click-to-activate/wheel/drag) at four points in `mouse.rs` ([input-layer](../../architecture/input-layer.md)).

- **Wheel scrolling (modifiers ignored)**: this overlay is up **while a modifier is held**, so egui's default handling (`Ctrl+wheel`=zoom, `Shift+wheel`=horizontal scroll) would not move the vertical `ScrollArea`. `modifier_free_wheel_y()` re-reads raw `MouseWheel` events regardless of modifiers while the pointer is over the panel (the same unit scale as egui) and injects only the vertical component via `scroll_with_delta` → **the wheel is pure vertical scroll whatever modifier is pressed**. alt/option alone is already handled vertically by egui, so to avoid double scrolling it injects only during Ctrl·Cmd·Shift holds.

- **Content**: `build_hint_sections(held: Combo, …)` (the modifier-hint content model) — exposes only combos that contain the pressed four-axis combo `held` as a subset (`combos_containing_all`, `Combo::contains_all`). Fixed host actions + user scripts + special roles (tab/workspace switching · mouse-capture bypass · link opening) sorted by combo size and priority. On a multi-axis hold the first section is the held combo itself, matching the header. An empty combo (neither binding nor role) **keeps** its section, and the overlay draws a muted "no bindings" placeholder under the ChordHead (`modifier_hint.empty`, no keycap/wash/glyph, min-height 20 px · inner gap 3 px) — holding an unassigned combo still shows the panel and states the absence explicitly ([ADR-0038](../../adr/0038-modifier-hint-empty-combo-placeholder.md)). Exposing plugin shortcuts is follow-up wiring (the PluginManager is App-owned and unreachable from the draw path).
- **ChordHead keycaps**: `combo_keycap_parts` (`modifier_hint_overlay.rs`) draws axes whose `GeneralSettings::{alt,option,shift}_display_style` is `"symbol"` as vector icons (`tasty_icons::{CMD_KEY,OPTION_KEY,SHIFT_KEY}`) instead of text ("⌘"/"⌥"/"⇧") — the glyphs are not in egui's font fallback chain, so as text they break into tofu boxes (see "symbol display" in [key-mapping.md](../../design/policies/key-mapping.md)). `kbd_parts` (`tasty-ui-widgets`) renders text/icon keycaps with the same background and border.
- **Hold detection**: only winit `ModifiersChanged` (real user input) is reflected — cannot be forced via IPC/CLI (principle 1). `held: Option<Combo>` holds the four currently pressed axes as-is, and when the combo changes `update_hold` **always returns dirty**, narrowing immediately. The timer (`hold_since`) starts only on the first press and is **not** reset merely by the combo changing (ADR-0035 A) — but when a registered shortcut is actually consumed on the **key input path** (excluding shared entry points such as the Command Palette — principle 1), `reset_reveal_timer_if_not_shown` restarts the timer for a hold not yet shown (ADR-0064). Cleared on window focus loss.
- **Reveal delay**: `reveal_delay_ms(held, theme)` gives `MOTION_HOLD_REVEAL_SHIFT_MS` (1200 ms) for a Shift-only hold and `MOTION_HOLD_REVEAL_MS` (500 ms) otherwise. Re-evaluated every frame, so adding a modifier while waiting on a Shift-only hold drops the delay to 500 ms and shows immediately if already elapsed. Values come only from Theme tokens (no hard-coding).
- **Geometry**: persisted in `modifier_hint.pos` / `modifier_hint.size` (`Option<(LogicalPx, LogicalPx)>`, default `None`). Default 180×400, minimum 180×240. Saved via `UpdateSettings` at the moment the user moves/resizes (the same nature as the sidebar width: globally shared + last-write-wins). If a window shrink pushes it off screen **the stored value is unchanged** and clamping is the render stage's responsibility. Geometry is overlay UI state, not accessibility meaning, so it lives in `ModifierHintSettings` (a separate root section).

## Interface

- **User**: the Settings Accessibility tab toggles (`settings.accessibility.modifier_hint*`). Showing the overlay is a real modifier hold · drag/resize/X are the user's mouse. i18n keys `modifier_hint.*` (held / hide_tooltip / role.*).
- **Agent**: nothing in release — the overlay cannot be shown or driven via IPC/CLI (principle 1, focus independence). For verification **in debug builds only** there are `debug.modifier_hint.hold` (force-state the held combo + backdate the timer) / `debug.modifier_hint.state` (dump the render state) ([debug-ipc](../../dev-guide/debug-ipc.md)). Isolated under `#[cfg(all(debug_assertions, feature = "gui"))]`, so not exposed in release.

## Related

- [settings](../settings/index.md) · [design/systems/toast](../../design/systems/toast.md) · [design/systems/theme](../../design/systems/theme.md) · [architecture/input-layer](../../architecture/input-layer.md) · [ubiquitous-language](../../concepts/ubiquitous-language.md)
