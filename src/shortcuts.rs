use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use crate::window::main::MainWindow;
use crate::window::Window as _;
use crate::model::SplitDirection;

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
        KeyCode::KeyA => "a", KeyCode::KeyB => "b", KeyCode::KeyC => "c",
        KeyCode::KeyD => "d", KeyCode::KeyE => "e", KeyCode::KeyF => "f",
        KeyCode::KeyG => "g", KeyCode::KeyH => "h", KeyCode::KeyI => "i",
        KeyCode::KeyJ => "j", KeyCode::KeyK => "k", KeyCode::KeyL => "l",
        KeyCode::KeyM => "m", KeyCode::KeyN => "n", KeyCode::KeyO => "o",
        KeyCode::KeyP => "p", KeyCode::KeyQ => "q", KeyCode::KeyR => "r",
        KeyCode::KeyS => "s", KeyCode::KeyT => "t", KeyCode::KeyU => "u",
        KeyCode::KeyV => "v", KeyCode::KeyW => "w", KeyCode::KeyX => "x",
        KeyCode::KeyY => "y", KeyCode::KeyZ => "z",
        KeyCode::Digit0 => "0", KeyCode::Digit1 => "1", KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3", KeyCode::Digit4 => "4", KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6", KeyCode::Digit7 => "7", KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        KeyCode::Minus => "-", KeyCode::Equal => "=",
        KeyCode::BracketLeft => "[", KeyCode::BracketRight => "]",
        KeyCode::Semicolon => ";", KeyCode::Quote => "'",
        KeyCode::Backquote => "`", KeyCode::Backslash => "\\",
        KeyCode::Comma => ",", KeyCode::Period => ".", KeyCode::Slash => "/",
        _ => return None,
    };
    Some(Key::Character(ch.into()))
}

/// 바인딩 목록 중 하나라도 매칭되면 true.
fn matches_any_binding(bindings: &[String], key: &Key, mods: ModifiersState) -> bool {
    bindings.iter().any(|b| matches_binding(b, key, mods))
}

/// Parsed binding: expected modifier state + the literal key token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedBinding<'a> {
    ctrl: bool,
    shift: bool,
    alt: bool,
    /// 키 토큰 (문자 "+", "-", "a" 또는 네임 "plus", "f1", "tab" 등). 공백/모디파이어 키워드는 거부되어 여기 오지 않는다.
    key: &'a str,
}

/// 왼쪽부터 `ctrl+`/`shift+`/`alt+` 프리픽스를 순차적으로 떼어낸다.
///
/// `split('+')`을 쓰지 않는 이유: `"ctrl++"`의 두 번째 `+`처럼 키 이름과 구분자가
/// 충돌하는 경우를 다루기 위함. 프리픽스를 하나씩 벗겨내면 남은 부분이 통째로 키가
/// 되므로 구분자 충돌 문제가 사라진다.
fn parse_binding(binding: &str) -> Option<ParsedBinding<'_>> {
    if binding.is_empty() {
        return None;
    }
    // Double-tap bindings (e.g. "shift+shift") are handled separately
    if is_double_tap_binding(binding).is_some() {
        return None;
    }

    let mut ctrl = false;
    let mut shift = false;
    let mut alt = false;
    let mut rest = binding;

    loop {
        let lower = rest.to_ascii_lowercase();
        if !ctrl && lower.starts_with("ctrl+") {
            ctrl = true;
            rest = &rest[5..];
        } else if !shift && lower.starts_with("shift+") {
            shift = true;
            rest = &rest[6..];
        } else if !alt && lower.starts_with("alt+") {
            alt = true;
            rest = &rest[4..];
        } else {
            break;
        }
    }

    // 키 파트가 비어있거나(`"ctrl+"`) 모디파이어 키워드 그대로(`"ctrl"` 단독)인 경우
    // 매칭이 불가능하므로 거부.
    if rest.is_empty() {
        return None;
    }
    let rest_lower = rest.to_ascii_lowercase();
    if matches!(rest_lower.as_str(), "ctrl" | "shift" | "alt") {
        return None;
    }

    Some(ParsedBinding { ctrl, shift, alt, key: rest })
}

/// Parse a binding string like "ctrl+shift+n" and check if it matches
/// the given key + modifiers. Returns false for empty bindings.
fn matches_binding(binding: &str, key: &Key, mods: ModifiersState) -> bool {
    let Some(parsed) = parse_binding(binding) else { return false; };

    // Modifier-only key presses must never trigger any shortcut, regardless of
    // how the binding is spelled. This is the structural guard that prevents
    // "Ctrl alone" from ever matching.
    if let Key::Named(n) = key {
        if matches!(
            n,
            NamedKey::Control
                | NamedKey::Shift
                | NamedKey::Alt
                | NamedKey::Super
                | NamedKey::Meta
                | NamedKey::Hyper
                | NamedKey::Fn
                | NamedKey::FnLock
                | NamedKey::CapsLock
                | NamedKey::NumLock
                | NamedKey::ScrollLock
                | NamedKey::Symbol
                | NamedKey::SymbolLock
        ) {
            return false;
        }
    }

    // Check modifiers match exactly.
    // On macOS, "alt" in binding maps to Cmd (super_key) since the physical
    // position of Cmd on macOS keyboards matches Alt on Windows/Linux keyboards.
    #[cfg(target_os = "macos")]
    let alt_matches = mods.super_key() == parsed.alt;
    #[cfg(not(target_os = "macos"))]
    let alt_matches = mods.alt_key() == parsed.alt;

    if mods.control_key() != parsed.ctrl
        || mods.shift_key() != parsed.shift
        || !alt_matches
    {
        return false;
    }

    let key_lower = parsed.key.to_ascii_lowercase();
    match key {
        Key::Character(c) => {
            let ch = c.to_lowercase();
            if key_matches_token(&ch, &key_lower) {
                return true;
            }
            // Ctrl+letter may arrive as control character (0x01-0x1A).
            // Convert back to the letter for matching.
            if parsed.ctrl && c.len() == 1 {
                let byte = c.as_bytes()[0];
                if byte >= 1 && byte <= 26 {
                    let letter = ((byte - 1) + b'a') as char;
                    return letter.to_string() == key_lower;
                }
            }
            false
        }
        Key::Named(named) => match named_key_to_string(named) {
            Some(named_str) => named_str == key_lower,
            None => false,
        },
        _ => false,
    }
}

/// 입력 문자(`character`, 이미 lowercase)가 바인딩 키 토큰(`token`)과 동일한 키를
/// 의미하는지 판정. `"plus"↔"+"`, `"minus"↔"-"`, `"equals"↔"="` 등 심볼 이름을
/// 양쪽 모두에서 받도록 별칭 매칭을 수행한다.
fn key_matches_token(character: &str, token: &str) -> bool {
    if character == token {
        return true;
    }
    match (character, token) {
        ("+", "plus") | ("plus", "+") => true,
        ("-", "minus") | ("minus", "-") => true,
        ("=", "equals") | ("equals", "=") => true,
        _ => false,
    }
}

fn named_key_to_string(key: &NamedKey) -> Option<&'static str> {
    Some(match key {
        NamedKey::Tab => "tab",
        NamedKey::Space => "space",
        NamedKey::Enter => "enter",
        NamedKey::Backspace => "backspace",
        NamedKey::Delete => "delete",
        NamedKey::Insert => "insert",
        NamedKey::Home => "home",
        NamedKey::End => "end",
        NamedKey::PageUp => "pageup",
        NamedKey::PageDown => "pagedown",
        NamedKey::ArrowUp => "up",
        NamedKey::ArrowDown => "down",
        NamedKey::ArrowLeft => "left",
        NamedKey::ArrowRight => "right",
        NamedKey::F1 => "f1",
        NamedKey::F2 => "f2",
        NamedKey::F3 => "f3",
        NamedKey::F4 => "f4",
        NamedKey::F5 => "f5",
        NamedKey::F6 => "f6",
        NamedKey::F7 => "f7",
        NamedKey::F8 => "f8",
        NamedKey::F9 => "f9",
        NamedKey::F10 => "f10",
        NamedKey::F11 => "f11",
        NamedKey::F12 => "f12",
        NamedKey::Escape => "escape",
        _ => return None,
    })
}

/// Check if a binding string represents a double-tap modifier (e.g. "shift+shift").
fn is_double_tap_binding(binding: &str) -> Option<crate::double_tap::DoubleTapKey> {
    match binding.to_lowercase().as_str() {
        "shift+shift" => Some(crate::double_tap::DoubleTapKey::Shift),
        "ctrl+ctrl" => Some(crate::double_tap::DoubleTapKey::Ctrl),
        "alt+alt" => Some(crate::double_tap::DoubleTapKey::Alt),
        _ => None,
    }
}

impl MainWindow {
    /// Handle double-tap modifier shortcuts. Returns true if consumed.
    pub(crate) fn handle_double_tap_shortcut(&mut self, dt: crate::double_tap::DoubleTapKey) -> bool {
        let kb = self.state.engine.settings.keybindings.clone();
        let dt_str = dt.binding_str();

        let has_dt = |bindings: &[String]| bindings.iter().any(|b| b == dt_str);
        if has_dt(&kb.toggle_settings) {
            let _ = self.proxy.send_event(crate::AppEvent::OpenSettings);
            return true;
        }
        if has_dt(&kb.toggle_notifications) {
            self.state.popups.toggle("notifications");
            if self.state.popups.is_open("notifications") {
                self.state.engine.notifications.mark_all_read();
            }
            return true;
        }

        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();

        // Check all configurable bindings for double-tap matches
        let bindings_to_check: Vec<(&[String], &str)> = vec![
            (&kb.new_workspace, "new_workspace"),
            (&kb.close_workspace, "close_workspace"),
            (&kb.new_tab, "new_tab"),
            (&kb.close_pane, "close_pane"),
            (&kb.split_pane_vertical, "split_pane_vertical"),
            (&kb.split_pane_horizontal, "split_pane_horizontal"),
            (&kb.split_surface_vertical, "split_surface_vertical"),
            (&kb.split_surface_horizontal, "split_surface_horizontal"),
            (&kb.focus_pane_next, "focus_pane_next"),
            (&kb.focus_pane_prev, "focus_pane_prev"),
            (&kb.focus_surface_next, "focus_surface_next"),
            (&kb.focus_surface_prev, "focus_surface_prev"),
            (&kb.close_surface, "close_surface"),
            (&kb.open_markdown, "open_markdown"),
            (&kb.open_explorer, "open_explorer"),
            (&kb.convert_surface, "convert_surface"),
            (&kb.convert_to_markdown, "convert_to_markdown"),
            (&kb.convert_to_explorer, "convert_to_explorer"),
            (&kb.close_active, "close_active"),
            (&kb.next_tab, "next_tab"),
            (&kb.prev_tab, "prev_tab"),
        ];

        for (bindings, action) in &bindings_to_check {
            if has_dt(bindings) {
                match *action {
                    "new_workspace" => { if let Err(e) = self.state.add_workspace() { tracing::warn!("add_workspace failed: {e}"); } self.state.resize_all(terminal_rect, cell_w, cell_h); }
                    "close_workspace" => {
                        self.state.close_active_workspace();
                        if self.state.engine.workspaces.is_empty() {
                            self.request_close();
                        } else {
                            self.state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    "new_tab" => { if let Err(e) = self.state.add_tab() { tracing::warn!("add_tab failed: {e}"); } self.state.resize_all(terminal_rect, cell_w, cell_h); }
                    "close_pane" => {
                        if !self.state.close_active_pane() { self.state.close_active_workspace(); }
                        if self.state.engine.workspaces.is_empty() {
                            self.request_close();
                        } else {
                            self.state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    "split_pane_vertical" => { if let Err(e) = self.state.split_pane(SplitDirection::Vertical) { tracing::warn!("split_pane_vertical failed: {e}"); } self.state.resize_all(terminal_rect, cell_w, cell_h); }
                    "split_pane_horizontal" => { if let Err(e) = self.state.split_pane(SplitDirection::Horizontal) { tracing::warn!("split_pane_horizontal failed: {e}"); } self.state.resize_all(terminal_rect, cell_w, cell_h); }
                    "split_surface_vertical" => { if let Err(e) = self.state.split_surface(SplitDirection::Vertical) { tracing::warn!("split_surface_vertical failed: {e}"); } self.state.resize_all(terminal_rect, cell_w, cell_h); }
                    "split_surface_horizontal" => { if let Err(e) = self.state.split_surface(SplitDirection::Horizontal) { tracing::warn!("split_surface_horizontal failed: {e}"); } self.state.resize_all(terminal_rect, cell_w, cell_h); }
                    "focus_pane_next" => { self.state.move_pane_focus_forward(); }
                    "focus_pane_prev" => { self.state.move_pane_focus_backward(); }
                    "focus_surface_next" => { self.state.move_surface_focus_forward(); }
                    "focus_surface_prev" => { self.state.move_surface_focus_backward(); }
                    "close_surface" => {
                        if !self.state.close_active_surface() { if !self.state.close_active_pane() { self.state.close_active_workspace(); } }
                        if self.state.engine.workspaces.is_empty() {
                            self.request_close();
                        } else {
                            self.state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    "restore_closed" => {
                        self.state.restore_closed_item();
                        self.state.resize_all(terminal_rect, cell_w, cell_h);
                    }
                    "quit" => { let _ = self.proxy.send_event(crate::AppEvent::QuitRequested); }
                    "quit_immediate" => { let _ = self.proxy.send_event(crate::AppEvent::Shutdown); }
                    "quit_minimize" => { let _ = self.proxy.send_event(crate::AppEvent::Minimize); }
                    "open_markdown" => {
                        let pane_id = self.state.active_workspace().focused_pane;
                        self.state.dialogs.file_open_pane_id = Some(pane_id);
                        self.state.dialogs.markdown_open_buffer.clear();
                        self.state.popups.open_centered_focused("markdown_open");
                    }
                    "open_explorer" => {
                        let home = directories::BaseDirs::new()
                            .map(|d| d.home_dir().to_string_lossy().to_string())
                            .unwrap_or_else(|| ".".to_string());
                        let _ = self.state.add_explorer_tab(home);
                    }
                    "convert_surface" => {
                        if let Some(sid) = self.state.focused_surface_id() {
                            self.state.dialogs.convert_popup = Some(sid);
                            self.state.dialogs.convert_popup_selected = None;
                            self.state.popups.open_with_scope("convert_surface", crate::ui::popup::PopupScope::Surface(sid));
                        }
                    }
                    "convert_to_markdown" => {
                        if let Some(sid) = self.state.focused_surface_id() {
                            let pane_id = self.state.active_workspace().focused_pane;
                            self.state.dialogs.markdown_convert_surface_id = Some(sid);
                            self.state.dialogs.file_open_pane_id = Some(pane_id);
                            self.state.dialogs.markdown_open_buffer.clear();
                            self.state.popups.open_with_scope("markdown_open", crate::ui::popup::PopupScope::Surface(sid));
                        }
                    }
                    "convert_to_explorer" => {
                        if let Some(sid) = self.state.focused_surface_id() {
                            self.state.convert_surface_to_explorer(sid);
                        }
                    }
                    "close_active" => {
                        if !self.state.close_active_tab() {
                            if !self.state.close_active_pane() {
                                self.state.close_active_workspace();
                            }
                        }
                        if self.state.engine.workspaces.is_empty() {
                            self.request_close();
                        } else {
                            self.state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    "next_tab" => { self.state.next_tab_in_pane(); }
                    "prev_tab" => { self.state.prev_tab_in_pane(); }
                    _ => {}
                }
                return true;
            }
        }

        false
    }

    /// Handle keyboard shortcuts. Returns true if the event was consumed by a shortcut.
    pub(crate) fn handle_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let ctrl = mods.control_key();
        let shift = mods.shift_key();
        #[cfg(target_os = "macos")]
        let alt = mods.super_key();
        #[cfg(not(target_os = "macos"))]
        let alt = mods.alt_key();

        let terminal_rect = self.compute_terminal_rect();
        let cell_w = self.base.gpu.cell_width();
        let cell_h = self.base.gpu.cell_height();

        // Copy path (Explorer only) — must check before generic copy
        if self.handle_copy_path_shortcut(key, mods) {
            return true;
        }

        // Clipboard copy (needs &self before state borrow)
        if self.handle_copy_shortcut(key, mods) {
            return true;
        }

        let kb = self.state.engine.settings.keybindings.clone();

        // Configurable keybinding shortcuts
        if Self::handle_keybinding_shortcuts(&mut self.state, &kb, key, mods, terminal_rect, cell_w, cell_h, &self.proxy) {
            if self.state.engine.workspaces.is_empty() { self.request_close(); }
            self.base.dirty = true;
            return true;
        }

        // Numeric tab/workspace switching (Ctrl+1..9 / Alt+1..9)
        if Self::handle_numeric_switch_shortcuts(&mut self.state, &kb, key, ctrl, shift, alt) {
            if self.state.engine.workspaces.is_empty() { self.request_close(); }
            self.base.dirty = true;
            return true;
        }

        // Clipboard paste
        if self.handle_paste_shortcut(key, mods) {
            return true;
        }

        // Zoom
        if Self::handle_zoom_shortcut(&mut self.state, key, mods) {
            self.base.dirty = true;
            return true;
        }

        false
    }

    fn handle_copy_path_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.state.engine.settings.keybindings.copy_path.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        // Explorer에서 선택된 파일 경로를 텍스트로 클립보드에 복사
        let st = self.state.focused_surface_type();
        if !matches!(st, crate::state::FocusedSurfaceType::Explorer) {
            return false;
        }
        let text = self.state.focused_explorer_selected_paths();
        if text.is_empty() {
            return false;
        }
        self.base.gpu.egui_state.set_clipboard_text(text);
        let scope = crate::ui::ToastScope::Surface(
            self.state.focused_surface_id().unwrap_or(0)
        );
        self.state.toasts.push_info(crate::i18n::t("toast.copied_path"), scope);
        self.mark_dirty();
        true
    }

    fn handle_copy_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.state.engine.settings.keybindings.copy.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        if self.copy_selection_to_clipboard() {
            self.mark_dirty();
            return true;
        }
        // For non-terminal surfaces (Explorer/Markdown), inject egui Copy
        // event so egui's label text selection can copy to clipboard.
        let st = self.state.focused_surface_type();
        if matches!(st, crate::state::FocusedSurfaceType::Explorer | crate::state::FocusedSurfaceType::Markdown) {
            self.base.gpu.egui_ctx.input_mut(|i| i.events.push(egui::Event::Copy));
            self.mark_dirty();
            return true;
        }
        false
    }

    fn handle_keybinding_shortcuts(
        state: &mut crate::state::AppState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        mods: ModifiersState,
        terminal_rect: crate::model::Rect,
        cell_w: f32,
        cell_h: f32,
        proxy: &winit::event_loop::EventLoopProxy<crate::AppEvent>,
    ) -> bool {
        if matches_any_binding(&kb.new_workspace, key, mods) {
            if let Err(e) = state.add_workspace() { tracing::warn!("add_workspace failed: {e}"); }
            return true;
        }
        if matches_any_binding(&kb.new_tab, key, mods) {
            if let Err(e) = state.add_tab() { tracing::warn!("add_tab failed: {e}"); }
            return true;
        }
        if matches_any_binding(&kb.split_pane_vertical, key, mods) {
            if let Err(e) = state.split_pane(SplitDirection::Vertical) { tracing::warn!("split_pane_vertical failed: {e}"); }
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_pane_horizontal, key, mods) {
            if let Err(e) = state.split_pane(SplitDirection::Horizontal) { tracing::warn!("split_pane_horizontal failed: {e}"); }
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_surface_vertical, key, mods) {
            if let Err(e) = state.split_surface(SplitDirection::Vertical) { tracing::warn!("split_surface_vertical failed: {e}"); }
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.split_surface_horizontal, key, mods) {
            if let Err(e) = state.split_surface(SplitDirection::Horizontal) { tracing::warn!("split_surface_horizontal failed: {e}"); }
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.toggle_settings, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::OpenSettings);
            return true;
        }
        if matches_any_binding(&kb.toggle_notifications, key, mods) {
            state.popups.toggle("notifications");
            if state.popups.is_open("notifications") {
                state.engine.notifications.mark_all_read();
            }
            return true;
        }
        if matches_any_binding(&kb.toggle_clipboard_viewer, key, mods) {
            if state.popups.is_open("clipboard_viewer") {
                state.popups.close("clipboard_viewer");
            } else {
                crate::clipboard_viewer_ui::open_clipboard_viewer_popup(state);
            }
            return true;
        }
        if matches_any_binding(&kb.close_workspace, key, mods) {
            state.close_active_workspace();
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.close_pane, key, mods) {
            if !state.close_active_pane() {
                state.close_active_workspace();
            }
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.close_surface, key, mods) {
            if !state.close_active_surface() {
                if !state.close_active_pane() {
                    state.close_active_workspace();
                }
            }
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.focus_pane_next, key, mods) {
            state.move_pane_focus_forward();
            return true;
        }
        if matches_any_binding(&kb.focus_pane_prev, key, mods) {
            state.move_pane_focus_backward();
            return true;
        }
        if matches_any_binding(&kb.focus_surface_next, key, mods) {
            state.move_surface_focus_forward();
            return true;
        }
        if matches_any_binding(&kb.focus_surface_prev, key, mods) {
            state.move_surface_focus_backward();
            return true;
        }
        if matches_any_binding(&kb.toggle_sidebar, key, mods) {
            state.sidebar_visible = !state.sidebar_visible;
            return true;
        }
        if matches_any_binding(&kb.toggle_sidebar_collapse, key, mods) {
            state.sidebar_collapsed = !state.sidebar_collapsed;
            return true;
        }
        if matches_any_binding(&kb.restore_closed, key, mods) {
            state.restore_closed_item();
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_any_binding(&kb.quit_immediate, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::Shutdown);
            return true;
        }
        if matches_any_binding(&kb.quit_minimize, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::Minimize);
            return true;
        }
        if matches_any_binding(&kb.quit, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::QuitRequested);
            return true;
        }
        if matches_any_binding(&kb.open_markdown, key, mods) {
            let pane_id = state.active_workspace().focused_pane;
            state.dialogs.file_open_pane_id = Some(pane_id);
            state.dialogs.markdown_open_buffer.clear();
            state.popups.open_centered_focused("markdown_open");
            return true;
        }
        if matches_any_binding(&kb.open_explorer, key, mods) {
            let home = directories::BaseDirs::new()
                            .map(|d| d.home_dir().to_string_lossy().to_string())
                            .unwrap_or_else(|| ".".to_string());
            let _ = state.add_explorer_tab(home);
            return true;
        }
        if matches_any_binding(&kb.convert_surface, key, mods) {
            if let Some(sid) = state.focused_surface_id() {
                state.dialogs.convert_popup = Some(sid);
                state.dialogs.convert_popup_selected = None;
                state.popups.open_with_scope("convert_surface", crate::ui::popup::PopupScope::Surface(sid));
            }
            return true;
        }
        if matches_any_binding(&kb.convert_to_markdown, key, mods) {
            if let Some(sid) = state.focused_surface_id() {
                let pane_id = state.active_workspace().focused_pane;
                state.dialogs.markdown_convert_surface_id = Some(sid);
                state.dialogs.file_open_pane_id = Some(pane_id);
                state.dialogs.markdown_open_buffer.clear();
                state.popups.open_with_scope("markdown_open", crate::ui::popup::PopupScope::Surface(sid));
            }
            return true;
        }
        if matches_any_binding(&kb.convert_to_explorer, key, mods) {
            if let Some(sid) = state.focused_surface_id() {
                state.convert_surface_to_explorer(sid);
            }
            return true;
        }
        if matches_any_binding(&kb.new_window, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::CreateWindow);
            return true;
        }
        if matches_any_binding(&kb.close_active, key, mods) {
            if !state.close_active_tab() {
                if !state.close_active_pane() {
                    state.close_active_workspace();
                }
            }
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_any_binding(&kb.next_tab, key, mods) {
            state.next_tab_in_pane();
            return true;
        }
        if matches_any_binding(&kb.prev_tab, key, mods) {
            state.prev_tab_in_pane();
            return true;
        }
        false
    }

    /// Number-key tab/workspace switching (Ctrl+1..9, Alt+1..9).
    /// Not exposed to the keybinding UI because it is a bank of 9 slots
    /// governed by the `tab_switch_modifier` / `workspace_switch_modifier`
    /// settings, not a single bindable combo.
    fn handle_numeric_switch_shortcuts(
        state: &mut crate::state::AppState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        if let Key::Character(c) = key {
            let ch = c.chars().next().unwrap_or('\0');
            if ch.is_ascii_digit() {
                let tab_mod = kb.tab_switch_modifier.to_lowercase();
                let tab_mod_matches = match tab_mod.as_str() {
                    "alt" => alt && !ctrl && !shift,
                    _ => ctrl && !shift && !alt,
                };
                if tab_mod_matches {
                    let index = if ch == '0' { 9 } else { (ch as usize) - ('1' as usize) };
                    state.goto_tab_in_pane(index);
                    return true;
                }

                let ws_mod = kb.workspace_switch_modifier.to_lowercase();
                let ws_mod_matches = match ws_mod.as_str() {
                    "ctrl" => ctrl && !shift && !alt,
                    _ => alt && !ctrl && !shift,
                };
                if ws_mod_matches {
                    if let Some(digit) = ch.to_digit(10) {
                        if digit >= 1 && digit <= 9 {
                            state.switch_workspace((digit - 1) as usize);
                            return true;
                        }
                    }
                }
            }
        }

        false
    }

    fn handle_paste_shortcut(&mut self, key: &Key, mods: ModifiersState) -> bool {
        let bindings = self.state.engine.settings.keybindings.paste.clone();
        if !matches_any_binding(&bindings, key, mods) {
            return false;
        }
        self.paste_to_terminal();
        self.mark_dirty();
        true
    }

    fn handle_zoom_shortcut(
        state: &mut crate::state::AppState,
        key: &Key,
        mods: ModifiersState,
    ) -> bool {
        let kb = &state.engine.settings.keybindings;
        if matches_any_binding(&kb.zoom_in, key, mods) {
            let current = state.engine.settings.appearance.font_size;
            state.engine.settings.appearance.font_size = (current + 1.0).min(72.0);
            return true;
        }
        if matches_any_binding(&kb.zoom_out, key, mods) {
            let current = state.engine.settings.appearance.font_size;
            state.engine.settings.appearance.font_size = (current - 1.0).max(6.0);
            return true;
        }
        if matches_any_binding(&kb.zoom_reset, key, mods) {
            state.engine.settings.appearance.font_size = 14.0;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::keyboard::SmolStr;

    fn mods_ctrl() -> ModifiersState {
        ModifiersState::CONTROL
    }
    fn mods_ctrl_shift() -> ModifiersState {
        ModifiersState::CONTROL | ModifiersState::SHIFT
    }
    fn mods_none() -> ModifiersState {
        ModifiersState::empty()
    }
    fn k_char(s: &str) -> Key {
        Key::Character(SmolStr::new(s))
    }
    fn k_named(n: NamedKey) -> Key {
        Key::Named(n)
    }

    // ── parse_binding 동작 ────────────────────────────────────────────

    #[test]
    fn parse_simple_modifier_plus_key() {
        let p = parse_binding("ctrl+a").unwrap();
        assert!(p.ctrl && !p.shift && !p.alt);
        assert_eq!(p.key, "a");
    }

    #[test]
    fn parse_double_plus_is_plus_key() {
        // "ctrl++" = Ctrl + `+` 키.
        let p = parse_binding("ctrl++").unwrap();
        assert!(p.ctrl && !p.shift && !p.alt);
        assert_eq!(p.key, "+");
    }

    #[test]
    fn parse_minus_and_equals() {
        assert_eq!(parse_binding("ctrl+-").unwrap().key, "-");
        assert_eq!(parse_binding("ctrl+=").unwrap().key, "=");
    }

    #[test]
    fn parse_plus_alias_is_canonical() {
        let p = parse_binding("ctrl+plus").unwrap();
        assert_eq!(p.key, "plus");
    }

    #[test]
    fn parse_empty_is_rejected() {
        assert!(parse_binding("").is_none());
    }

    #[test]
    fn parse_trailing_plus_is_rejected() {
        // "ctrl+"처럼 키가 없는 경우.
        assert!(parse_binding("ctrl+").is_none());
    }

    #[test]
    fn parse_modifier_only_is_rejected() {
        assert!(parse_binding("ctrl").is_none());
        assert!(parse_binding("shift").is_none());
        assert!(parse_binding("alt").is_none());
    }

    #[test]
    fn parse_accepts_any_modifier_order() {
        let p1 = parse_binding("ctrl+shift+a").unwrap();
        let p2 = parse_binding("shift+ctrl+a").unwrap();
        assert_eq!((p1.ctrl, p1.shift, p1.key), (true, true, "a"));
        assert_eq!((p2.ctrl, p2.shift, p2.key), (true, true, "a"));
    }

    #[test]
    fn parse_is_case_insensitive_for_modifiers() {
        let p = parse_binding("CTRL+A").unwrap();
        assert!(p.ctrl);
        assert_eq!(p.key, "A");
    }

    // ── matches_binding: 모디파이어 단독 방어 ─────────────────────────

    #[test]
    fn ctrl_alone_does_not_match_any_binding() {
        let key = k_named(NamedKey::Control);
        // 어떤 바인딩과도 Ctrl 단독은 매칭되지 않아야 한다.
        for binding in ["ctrl++", "ctrl+=", "ctrl+plus", "ctrl+a", "ctrl+shift+="] {
            assert!(
                !matches_binding(binding, &key, mods_ctrl()),
                "binding {binding:?}가 Ctrl 단독에 매칭되면 안 된다"
            );
        }
    }

    #[test]
    fn shift_alone_does_not_match_any_binding() {
        let key = k_named(NamedKey::Shift);
        assert!(!matches_binding("shift+a", &key, ModifiersState::SHIFT));
    }

    #[test]
    fn alt_alone_does_not_match_any_binding() {
        let key = k_named(NamedKey::Alt);
        assert!(!matches_binding("alt+a", &key, ModifiersState::ALT));
    }

    // ── matches_binding: 정상 매칭 경로 ───────────────────────────────

    #[test]
    fn plus_key_matches_ctrl_plus_binding() {
        let key = k_char("+");
        assert!(matches_binding("ctrl++", &key, mods_ctrl()));
    }

    #[test]
    fn plus_alias_matches_plus_character() {
        let key = k_char("+");
        assert!(matches_binding("ctrl+plus", &key, mods_ctrl()));
    }

    #[test]
    fn plus_character_matches_plus_alias_and_literal() {
        let key = k_char("+");
        assert!(matches_binding("ctrl+plus", &key, mods_ctrl()));
        assert!(matches_binding("ctrl++", &key, mods_ctrl()));
    }

    #[test]
    fn equals_key_matches_ctrl_equals_binding() {
        let key = k_char("=");
        assert!(matches_binding("ctrl+=", &key, mods_ctrl()));
        assert!(matches_binding("ctrl+equals", &key, mods_ctrl()));
    }

    #[test]
    fn minus_key_matches_ctrl_minus_binding() {
        let key = k_char("-");
        assert!(matches_binding("ctrl+-", &key, mods_ctrl()));
        assert!(matches_binding("ctrl+minus", &key, mods_ctrl()));
    }

    #[test]
    fn shift_requirement_is_enforced() {
        // "ctrl++"는 Shift를 기대하지 않으므로 Ctrl+Shift+<+키>는 매칭 안 됨.
        let key = k_char("+");
        assert!(!matches_binding("ctrl++", &key, mods_ctrl_shift()));
        // 반대로 "ctrl+shift+="는 shift를 요구.
        let eq = k_char("=");
        assert!(matches_binding("ctrl+shift+=", &eq, mods_ctrl_shift()));
        assert!(!matches_binding("ctrl+shift+=", &eq, mods_ctrl()));
    }

    #[test]
    fn letter_matches_both_char_and_control_char() {
        // Ctrl+letter가 0x01-0x1A로 도착해도 매칭.
        let ctrl_a = k_char("\u{1}"); // Ctrl+A = 0x01
        assert!(matches_binding("ctrl+a", &ctrl_a, mods_ctrl()));
        let plain_a = k_char("a");
        assert!(matches_binding("ctrl+a", &plain_a, mods_ctrl()));
    }

    #[test]
    fn no_modifier_binding_does_not_match_when_ctrl_held() {
        // 가상의 "a" 단독 바인딩 (파서는 허용하지만 의미상 수정자 요구 안 함).
        // Ctrl을 누르고 a를 눌렀는데 바인딩이 "a"뿐이라면 매칭되면 안 됨.
        let key = k_char("a");
        assert!(matches_binding("a", &key, mods_none()));
        assert!(!matches_binding("a", &key, mods_ctrl()));
    }

    #[test]
    fn empty_binding_never_matches() {
        let key = k_char("a");
        assert!(!matches_binding("", &key, mods_none()));
    }

    #[test]
    fn named_key_without_mapping_never_matches_empty() {
        // NamedKey::Control 같이 매핑이 없는 키는 매칭되지 않아야 한다.
        // 과거에는 named_str이 "" 를 반환해서 빈 key_part와 매칭되는 버그가 있었다.
        let key = k_named(NamedKey::Control);
        assert!(!matches_binding("ctrl+a", &key, mods_ctrl()));
    }
}
