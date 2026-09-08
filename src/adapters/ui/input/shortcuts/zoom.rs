//! Zoom in / out / reset 단축키 — focused surface 의 font_size override 갱신.

use winit::keyboard::{Key, ModifiersState};

use super::binding::matches_any_binding;
use crate::view::main::MainView;

/// 줌 세 동작. **판별과 실행을 가르는 것이 이 타입의 존재 이유다** — 단발 키 경로는
/// 키로 이것을 정하고, 명령 팔레트는 `action_id` 로 정한다. 실행부는 하나다.
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
        // Pick which surface override the shortcut targets based on focus.
        let focus = state.focused_surface_type(engine);
        // 어느 kind 가 줌 가능한지는 registry 의 zoomable capability 로 판정(kind
        // 하드코딩 없음). appearance 가변 대여 전에 미리 계산한다(registry 는 engine 의
        // 다른 필드라 동시 대여 회피).
        let kind_zoomable = focus.kind_capability(engine, |d| d.zoomable);

        // webview(rendering="webview") kind 는 font_size override 가 아니라
        // `PlatformWebView::set_zoom` 경로(host_api/webview.rs)를 탄다 — HtmlWebViewSettings
        // 가 이미 매 프레임 plugin_settings 를 polling 해 backend 에 적용하므로(sync_webviews),
        // 여기서는 그 설정 슬롯만 갱신하면 된다(신규 host API 불필요). egui-mesh kind 는
        // 아래 기존 font_size 분기로 그대로 진행(unregressed).
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
            // Other surfaces don't expose a font_size shortcut.
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
