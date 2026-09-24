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
    any_binding_pressed_egui, matches_any_binding, physical_key_to_logical,
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
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
) -> Option<crate::model::WorkspaceCategoryId> {
    if engine.workspaces.is_empty() {
        return None;
    }
    Some(state.active_workspace(engine).category)
}

fn focused_explorer_panel<'a>(
    state: &crate::state::AppState,
    engine: &'a crate::core::CoreState,
) -> Option<&'a crate::model::ExplorerPanel> {
    let pane = state.focused_pane(engine)?;
    let tab = pane.tabs.get(pane.active_tab)?;
    let focused = tab.focused_surface;
    let surface = tab.layout().find_surface(focused)?;
    surface
        .as_any()
        .downcast_ref::<crate::model::ExplorerPanel>()
}

fn focused_explorer_surface_id(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
) -> Option<u32> {
    focused_explorer_panel(state, engine).map(|p| p.id)
}

/// 키보드 붙여넣기는 포커스된 탐색기의 현재 폴더를 대상으로 한다.
fn focused_explorer_cwd(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
) -> Option<std::path::PathBuf> {
    focused_explorer_panel(state, engine).map(|p| p.current_root().to_path_buf())
}
