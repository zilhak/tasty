//! Zoom in / out / reset 단축키 — focused surface 의 font_size override 갱신.

use winit::keyboard::{Key, ModifiersState};

use crate::view::main::MainView;
use tasty_key_match::matches_any_binding;

/// 키 입력과 명령 팔레트가 공유하는 줌 동작.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ZoomAction {
    In,
    Out,
    Reset,
}

impl MainView {
    pub(super) fn handle_zoom_shortcut(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
        let kb = &engine.settings.keybindings;
        let action = if matches_any_binding(&kb.zoom_in, key, mods) {
            ZoomAction::In
        } else if matches_any_binding(&kb.zoom_out, key, mods) {
            ZoomAction::Out
        } else if matches_any_binding(&kb.zoom_reset, key, mods) {
            ZoomAction::Reset
        } else {
            return false;
        };
        Self::apply_zoom(state, engine, action)
    }

    /// 줌 실행. 포커스된 surface 가 줌 대상이 아니면 `false`.
    pub(crate) fn apply_zoom(
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        action: ZoomAction,
    ) -> bool {
        use crate::state::FocusedSurfaceType;
        let focus = state.focused_surface_type(engine);
        // appearance를 가변 대여하기 전에 registry의 zoomable 값을 읽는다.
        let kind_zoomable = focus.kind_capability(engine, |d| d.zoomable);

        // webview는 plugin_settings의 zoom을 sync_webviews에서 backend에 적용한다.
        // egui로 그리는 surface는 아래에서 font_size를 조정한다.
        if let FocusedSurfaceType::Kind(k) = &focus
            && kind_zoomable
            && crate::core::surface_registry::webview_kind::is_webview_kind(k)
            && let Some(plugin_id) = crate::webview::webview_settings_plugin_id(k)
        {
            use crate::settings::PluginSettingValue;
            let current = match engine.settings.plugin_setting(plugin_id, "zoom") {
                Some(PluginSettingValue::Number(n)) => *n,
                _ => 100.0,
            };
            let next = match action {
                ZoomAction::Reset => 100.0,
                ZoomAction::In => (current + 10.0).min(500.0),
                ZoomAction::Out => (current - 10.0).max(25.0),
            };
            engine
                .settings
                .set_plugin_setting(plugin_id, "zoom", PluginSettingValue::Number(next));
            return true;
        }

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
            _ => return false,
        };

        match action {
            ZoomAction::Reset => override_ref.font_size = None,
            ZoomAction::In => {
                override_ref.font_size = Some((current_effective_size + 1.0).min(72.0));
            }
            ZoomAction::Out => {
                override_ref.font_size = Some((current_effective_size - 1.0).max(6.0));
            }
        }
        true
    }
}
