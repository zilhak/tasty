//! Zoom in / out / reset 단축키 — focused surface 의 font_size override 갱신.

use winit::keyboard::{Key, ModifiersState};

use super::binding::matches_any_binding;
use crate::window::main::MainWindow;

impl MainWindow {
    pub(super) fn handle_zoom_shortcut(
        state: &mut crate::state::AppState,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
        use crate::state::FocusedSurfaceType;
        let kb = &state.engine.settings.keybindings;
        let is_zoom_in = matches_any_binding(&kb.zoom_in, key, mods);
        let is_zoom_out = matches_any_binding(&kb.zoom_out, key, mods);
        let is_zoom_reset = matches_any_binding(&kb.zoom_reset, key, mods);
        if !(is_zoom_in || is_zoom_out || is_zoom_reset) {
            return false;
        }

        // Pick which surface override the shortcut targets based on focus.
        let focus = state.focused_surface_type();
        let appearance = &mut state.engine.settings.appearance;
        let (override_ref, current_effective_size) = match &focus {
            FocusedSurfaceType::Terminal => {
                let size = appearance
                    .default_font
                    .apply_override(&appearance.terminal_font)
                    .font_size;
                (&mut appearance.terminal_font, size)
            }
            FocusedSurfaceType::Kind(k) if k == "markdown" => {
                let size = appearance
                    .default_font
                    .apply_override(&appearance.markdown_font)
                    .font_size;
                (&mut appearance.markdown_font, size)
            }
            // Other surfaces don't expose a font_size shortcut.
            _ => return false,
        };

        if is_zoom_reset {
            override_ref.font_size = None;
        } else if is_zoom_in {
            override_ref.font_size = Some((current_effective_size + 1.0).min(72.0));
        } else if is_zoom_out {
            override_ref.font_size = Some((current_effective_size - 1.0).max(6.0));
        }
        true
    }
}
