//! Zoom in / out / reset 단축키 — focused surface 의 font_size override 갱신.

use winit::keyboard::{Key, ModifiersState};

use super::binding::matches_any_binding;
use crate::view::main::MainView;

impl MainView {
    pub(super) fn handle_zoom_shortcut(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
        use crate::state::FocusedSurfaceType;
        let kb = &engine.settings.keybindings;
        let is_zoom_in = matches_any_binding(&kb.zoom_in, key, mods);
        let is_zoom_out = matches_any_binding(&kb.zoom_out, key, mods);
        let is_zoom_reset = matches_any_binding(&kb.zoom_reset, key, mods);
        if !(is_zoom_in || is_zoom_out || is_zoom_reset) {
            return false;
        }

        // Pick which surface override the shortcut targets based on focus.
        let focus = state.focused_surface_type(engine);
        // 어느 kind 가 줌 가능한지는 registry 의 zoomable capability 로 판정(kind
        // 하드코딩 없음). appearance 가변 대여 전에 미리 계산한다(registry 는 engine 의
        // 다른 필드라 동시 대여 회피).
        let kind_zoomable = focus.kind_capability(engine, |d| d.zoomable);
        let appearance = &mut engine.settings.appearance;
        let (override_ref, current_effective_size) = match &focus {
            FocusedSurfaceType::Terminal => {
                let size = appearance
                    .default_font
                    .apply_override(&appearance.terminal_font)
                    .font_size;
                (&mut appearance.terminal_font, size)
            }
            FocusedSurfaceType::Kind(k) if kind_zoomable => {
                let size = appearance.effective_font_for_kind(k).font_size;
                let ov = appearance
                    .plugin_font_overrides
                    .entry(k.to_string())
                    .or_default();
                (ov, size)
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
