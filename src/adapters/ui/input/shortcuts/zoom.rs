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
        state: &mut crate::state::MainViewState,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
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
        state: &mut crate::state::MainViewState,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        action: ZoomAction,
    ) -> bool {
        use crate::state::FocusedSurfaceType;
        let focus = state.focused_surface_type(engine);
        // Fix the target now; the execution owner applies each queued change in order.
        let kind_zoomable = focus.kind_capability(engine, |d| d.zoomable);
        let change = match action {
            ZoomAction::In => crate::app::engine_action::ZoomChange::In,
            ZoomAction::Out => crate::app::engine_action::ZoomChange::Out,
            ZoomAction::Reset => crate::app::engine_action::ZoomChange::Reset,
        };

        // webview는 plugin_settings의 zoom을 sync_webviews에서 backend에 적용한다.
        // egui로 그리는 surface는 아래에서 font_size를 조정한다.
        if let FocusedSurfaceType::Kind(k) = &focus
            && kind_zoomable
            && crate::runtime::surface_registry::webview_kind::is_webview_kind(k)
            && let Some(plugin_id) = crate::webview::webview_settings_plugin_id(k)
        {
            state.dispatch_intent(
                crate::intent::Intent::PatchSettings(
                    crate::app::engine_action::SettingsPatch::PluginZoom {
                        plugin: plugin_id.to_owned(),
                        change,
                    },
                )
                .from_user_shortcut("zoom"),
            );
            return true;
        }

        let kind = match &focus {
            FocusedSurfaceType::Terminal => None,
            FocusedSurfaceType::Kind(kind) if kind_zoomable => Some(kind.to_string()),
            _ => return false,
        };
        state.dispatch_intent(
            crate::intent::Intent::PatchSettings(
                crate::app::engine_action::SettingsPatch::FontSize { kind, change },
            )
            .from_user_shortcut("zoom"),
        );
        true
    }
}
