mod binding;
mod copy_paste;
mod dispatch;
mod double_tap;
mod keybinding;
/// modifier-hint 조합 콘텐츠 모델(순수 로직). 소비처(modifier-hint-03 wiring)는 미연결.
pub(crate) mod modifier_hint;
mod numeric;
#[cfg(test)]
mod tests;
mod zoom;

pub(crate) use binding::{any_binding_pressed_egui, binding_has_modifier, matches_any_binding};
use winit::event_loop::EventLoopProxy;
use winit::keyboard::{Key, KeyCode, PhysicalKey};

/// Best-effort `EventLoopProxy::send_event` dispatch.
///
/// `send_event`는 event loop가 이미 종료된 뒤에만 Err를 돌려준다. quit/shutdown
/// 단축키 직후의 자투리 입력 race에서만 발생하며, 이미 종료 중인 상황이라 무해
/// — 다만 디버깅에 도움 되도록 trace 레벨로 흔적은 남긴다.
pub(crate) fn send_app_event(proxy: &EventLoopProxy<crate::AppEvent>, event: crate::AppEvent) {
    if let Err(e) = proxy.send_event(event) {
        tracing::trace!("AppEvent send dropped (event loop closing): {e}");
    }
}

/// Convert a physical key code to a Key::Character for shortcut matching.
/// On macOS, when IME is composing (e.g. Korean), logical_key may contain
/// the composed character (e.g. "ㅇ" instead of "d"). This function extracts
/// the intended key from the physical key code.
pub(crate) fn physical_key_to_logical(physical: &PhysicalKey) -> Option<Key> {
    let code = match physical {
        PhysicalKey::Code(c) => c,
        _ => return None,
    };
    let ch: &str = match code {
        KeyCode::KeyA => "a",
        KeyCode::KeyB => "b",
        KeyCode::KeyC => "c",
        KeyCode::KeyD => "d",
        KeyCode::KeyE => "e",
        KeyCode::KeyF => "f",
        KeyCode::KeyG => "g",
        KeyCode::KeyH => "h",
        KeyCode::KeyI => "i",
        KeyCode::KeyJ => "j",
        KeyCode::KeyK => "k",
        KeyCode::KeyL => "l",
        KeyCode::KeyM => "m",
        KeyCode::KeyN => "n",
        KeyCode::KeyO => "o",
        KeyCode::KeyP => "p",
        KeyCode::KeyQ => "q",
        KeyCode::KeyR => "r",
        KeyCode::KeyS => "s",
        KeyCode::KeyT => "t",
        KeyCode::KeyU => "u",
        KeyCode::KeyV => "v",
        KeyCode::KeyW => "w",
        KeyCode::KeyX => "x",
        KeyCode::KeyY => "y",
        KeyCode::KeyZ => "z",
        KeyCode::Digit0 => "0",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3",
        KeyCode::Digit4 => "4",
        KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6",
        KeyCode::Digit7 => "7",
        KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        KeyCode::Minus => "-",
        KeyCode::Equal => "=",
        KeyCode::BracketLeft => "[",
        KeyCode::BracketRight => "]",
        KeyCode::Semicolon => ";",
        KeyCode::Quote => "'",
        KeyCode::Backquote => "`",
        KeyCode::Backslash => "\\",
        KeyCode::Comma => ",",
        KeyCode::Period => ".",
        KeyCode::Slash => "/",
        _ => return None,
    };
    Some(Key::Character(ch.into()))
}

/// 단축키/Command Palette 로 새 워크스페이스를 생성할 때 계승할 카테고리 — 현재
/// 활성 워크스페이스의 소속. `active_workspace` 는 `engine.workspaces` 가 비어있지
/// 않아야 하는 invariant 가 있으므로(parked 상태, 워크스페이스 0개), 그 경우엔
/// `None`(생성 시 normal 로 fallback)을 반환해 패닉을 피한다.
pub(crate) fn focused_workspace_category(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
) -> Option<crate::model::WorkspaceCategoryId> {
    if engine.workspaces.is_empty() {
        return None;
    }
    Some(state.active_workspace(engine).category)
}

/// 포커스된 surface 가 `ExplorerPanel` 이면 그 참조를 반환한다.
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

/// Returns the surface ID of the focused Explorer surface, if any.
fn focused_explorer_surface_id(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
) -> Option<u32> {
    focused_explorer_panel(state, engine).map(|p| p.id)
}

/// 포커스된 Explorer surface 의 현재 디렉토리(`current_root`). 키보드 붙여넣기의
/// 대상 디렉토리로 쓰인다(컨텍스트 메뉴의 빈 영역 대상 붙여넣기와 동일 정책).
fn focused_explorer_cwd(
    state: &crate::state::AppState,
    engine: &crate::core::CoreState,
) -> Option<std::path::PathBuf> {
    focused_explorer_panel(state, engine).map(|p| p.current_root().to_path_buf())
}
