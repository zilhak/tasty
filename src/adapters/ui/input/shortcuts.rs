mod copy_paste;
mod dispatch;
mod double_tap;
mod keybinding;
/// modifier-hint에 표시할 조합 모델.
pub(crate) mod modifier_hint;
mod numeric;
#[cfg(test)]
mod tests;
mod webview_claims;
mod zoom;

pub(crate) use webview_claims::webview_shortcut_policy;

pub(crate) use tasty_key_match::{
    any_binding_pressed_egui, consume_binding_egui, matches_any_binding, physical_key_to_logical,
    shortcut_lookup_key,
};
use winit::event_loop::EventLoopProxy;

/// 이벤트 루프 종료 뒤 입력이 도착할 수 있으므로 전송 실패는 trace로 남긴다.
pub(crate) fn send_app_event(proxy: &EventLoopProxy<crate::AppEvent>, event: crate::AppEvent) {
    if let Err(e) = proxy.send_event(event) {
        tracing::trace!("AppEvent send dropped (event loop closing): {e}");
    }
}

/// 새 workspace는 활성 대상의 카테고리를 상속한다. 빈 engine이면 None으로 normal을 사용한다.
pub(crate) fn focused_workspace_category(
    state: &crate::state::MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> Option<crate::model::WorkspaceCategoryId> {
    if engine.workspaces().is_empty() {
        return None;
    }
    Some(state.active_workspace(engine).category)
}

fn focused_explorer_panel<'a>(
    state: &crate::state::MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'a>,
) -> Option<&'a crate::model::ExplorerPanel> {
    let pane = state.focused_pane(engine)?;
    let tab = pane.tabs.get(state.navigation.tab_index(pane))?;
    let focused = state.navigation.surface_id(tab).unwrap_or(0);
    engine.find_surface_by_id(focused)?.explorer()
}

fn focused_explorer_surface_id(
    state: &crate::state::MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> Option<u32> {
    focused_explorer_panel(state, engine).map(|p| p.id)
}

/// 미리보기 토글·Properties 단축키. 포커스된 탐색기만 바꾼다.
fn explorer_view_shortcut(
    state: &mut crate::state::MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    action: &str,
) {
    let Some(panel) = focused_explorer_panel(state, engine) else {
        return;
    };
    let (sid, cwd) = (panel.id, panel.current_root().to_path_buf());
    if action == "explorer_toggle_preview" {
        if let Some(view) = state.explorer_views.get_mut(sid) {
            view.preview.toggle();
        }
        return;
    }
    crate::adapters::ui::popup::explorer_properties::open_for_shortcut(
        state,
        engine.is_mirror_surface(sid),
        sid,
        &cwd,
    );
}

/// 키보드 붙여넣기는 포커스된 탐색기의 현재 폴더를 대상으로 한다.
fn focused_explorer_cwd(
    state: &crate::state::MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> Option<std::path::PathBuf> {
    focused_explorer_panel(state, engine).map(|p| p.current_root().to_path_buf())
}
