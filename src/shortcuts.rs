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

/// Parse a binding string like "ctrl+shift+n" and check if it matches
/// the given key + modifiers. Returns false for empty bindings.
fn matches_binding(binding: &str, key: &Key, mods: ModifiersState) -> bool {
    if binding.is_empty() {
        return false;
    }
    // Double-tap bindings (e.g. "shift+shift") are handled separately
    if is_double_tap_binding(binding).is_some() {
        return false;
    }

    let parts: Vec<&str> = binding.split('+').collect();
    if parts.is_empty() {
        return false;
    }

    // Extract expected modifiers and the key part (last non-modifier token)
    let mut expect_ctrl = false;
    let mut expect_shift = false;
    let mut expect_alt = false;
    let mut key_part = "";

    for part in &parts {
        match part.to_lowercase().as_str() {
            "ctrl" => expect_ctrl = true,
            "shift" => expect_shift = true,
            "alt" => expect_alt = true,
            _ => key_part = part,
        }
    }

    // Check modifiers match exactly.
    // On macOS, "alt" in binding maps to Cmd (super_key) since the physical
    // position of Cmd on macOS keyboards matches Alt on Windows/Linux keyboards.
    #[cfg(target_os = "macos")]
    let alt_matches = mods.super_key() == expect_alt;
    #[cfg(not(target_os = "macos"))]
    let alt_matches = mods.alt_key() == expect_alt;

    if mods.control_key() != expect_ctrl
        || mods.shift_key() != expect_shift
        || !alt_matches
    {
        return false;
    }

    // Match the key part
    let key_lower = key_part.to_lowercase();
    match key {
        Key::Character(c) => {
            let ch = c.to_lowercase();
            if ch == key_lower {
                return true;
            }
            // Ctrl+letter may arrive as control character (0x01-0x1A).
            // Convert back to the letter for matching.
            if expect_ctrl && c.len() == 1 {
                let byte = c.as_bytes()[0];
                if byte >= 1 && byte <= 26 {
                    let letter = ((byte - 1) + b'a') as char;
                    return letter.to_string() == key_lower;
                }
            }
            false
        }
        Key::Named(named) => {
            let named_str = named_key_to_string(named);
            named_str == key_lower
        }
        _ => false,
    }
}

fn named_key_to_string(key: &NamedKey) -> String {
    match key {
        NamedKey::Tab => "tab".into(),
        NamedKey::Space => "space".into(),
        NamedKey::Enter => "enter".into(),
        NamedKey::Backspace => "backspace".into(),
        NamedKey::Delete => "delete".into(),
        NamedKey::Insert => "insert".into(),
        NamedKey::Home => "home".into(),
        NamedKey::End => "end".into(),
        NamedKey::PageUp => "pageup".into(),
        NamedKey::PageDown => "pagedown".into(),
        NamedKey::ArrowUp => "up".into(),
        NamedKey::ArrowDown => "down".into(),
        NamedKey::ArrowLeft => "left".into(),
        NamedKey::ArrowRight => "right".into(),
        NamedKey::F1 => "f1".into(),
        NamedKey::F2 => "f2".into(),
        NamedKey::F3 => "f3".into(),
        NamedKey::F4 => "f4".into(),
        NamedKey::F5 => "f5".into(),
        NamedKey::F6 => "f6".into(),
        NamedKey::F7 => "f7".into(),
        NamedKey::F8 => "f8".into(),
        NamedKey::F9 => "f9".into(),
        NamedKey::F10 => "f10".into(),
        NamedKey::F11 => "f11".into(),
        NamedKey::F12 => "f12".into(),
        NamedKey::Escape => "escape".into(),
        _ => String::new(),
    }
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

        if kb.toggle_settings == dt_str {
            let _ = self.proxy.send_event(crate::AppEvent::OpenSettings);
            return true;
        }
        if kb.toggle_notifications == dt_str {
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
        let bindings_to_check: Vec<(String, &str)> = vec![
            (kb.new_workspace.clone(), "new_workspace"),
            (kb.close_workspace.clone(), "close_workspace"),
            (kb.new_tab.clone(), "new_tab"),
            (kb.close_pane.clone(), "close_pane"),
            (kb.split_pane_vertical.clone(), "split_pane_vertical"),
            (kb.split_pane_horizontal.clone(), "split_pane_horizontal"),
            (kb.split_surface_vertical.clone(), "split_surface_vertical"),
            (kb.split_surface_horizontal.clone(), "split_surface_horizontal"),
            (kb.focus_pane_next.clone(), "focus_pane_next"),
            (kb.focus_pane_prev.clone(), "focus_pane_prev"),
            (kb.focus_surface_next.clone(), "focus_surface_next"),
            (kb.focus_surface_prev.clone(), "focus_surface_prev"),
            (kb.close_surface.clone(), "close_surface"),
            (kb.open_markdown.clone(), "open_markdown"),
            (kb.open_explorer.clone(), "open_explorer"),
            (kb.convert_surface.clone(), "convert_surface"),
            (kb.convert_to_markdown.clone(), "convert_to_markdown"),
            (kb.convert_to_explorer.clone(), "convert_to_explorer"),
        ];

        for (binding, action) in &bindings_to_check {
            if binding == dt_str {
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

        // Clipboard copy (needs &self before state borrow)
        if self.handle_copy_shortcut(key, ctrl, shift, alt) {
            return true;
        }

        let kb = self.state.engine.settings.keybindings.clone();

        // Configurable keybinding shortcuts
        if Self::handle_keybinding_shortcuts(&mut self.state, &kb, key, mods, terminal_rect, cell_w, cell_h, &self.proxy) {
            if self.state.engine.workspaces.is_empty() { self.request_close(); }
            self.base.dirty = true;
            return true;
        }

        // Hardcoded shortcuts (tab switch, Ctrl+W, number switch)
        if Self::handle_hardcoded_shortcuts(&mut self.state, &kb, key, ctrl, shift, alt, terminal_rect, cell_w, cell_h) {
            if self.state.engine.workspaces.is_empty() { self.request_close(); }
            self.base.dirty = true;
            return true;
        }

        // Clipboard paste
        if self.handle_paste_shortcut(key, ctrl, shift, alt) {
            return true;
        }

        // Zoom
        if Self::handle_zoom_shortcut(&mut self.state, key, ctrl, shift, alt) {
            self.base.dirty = true;
            return true;
        }

        false
    }

    fn handle_copy_shortcut(&mut self, key: &Key, ctrl: bool, shift: bool, alt: bool) -> bool {
        let clipboard = &self.state.engine.settings.clipboard;
        if let Key::Character(c) = key {
            let s = c.as_str().to_lowercase();
            let is_c = s == "c" || c.as_str() == "\x03";
            if is_c {
                if (ctrl && shift && clipboard.linux_style)
                    || (ctrl && !shift && !alt && clipboard.windows_style)
                    || (alt && !ctrl && !shift && clipboard.macos_style)
                {
                    if self.copy_selection_to_clipboard() {
                        self.mark_dirty();
                        return true;
                    }
                }
            }
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
        if matches_binding(&kb.new_workspace, key, mods) {
            if let Err(e) = state.add_workspace() { tracing::warn!("add_workspace failed: {e}"); }
            return true;
        }
        if matches_binding(&kb.new_tab, key, mods) {
            if let Err(e) = state.add_tab() { tracing::warn!("add_tab failed: {e}"); }
            return true;
        }
        if matches_binding(&kb.split_pane_vertical, key, mods) {
            if let Err(e) = state.split_pane(SplitDirection::Vertical) { tracing::warn!("split_pane_vertical failed: {e}"); }
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_binding(&kb.split_pane_horizontal, key, mods) {
            if let Err(e) = state.split_pane(SplitDirection::Horizontal) { tracing::warn!("split_pane_horizontal failed: {e}"); }
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_binding(&kb.split_surface_vertical, key, mods) {
            if let Err(e) = state.split_surface(SplitDirection::Vertical) { tracing::warn!("split_surface_vertical failed: {e}"); }
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_binding(&kb.split_surface_horizontal, key, mods) {
            if let Err(e) = state.split_surface(SplitDirection::Horizontal) { tracing::warn!("split_surface_horizontal failed: {e}"); }
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_binding(&kb.toggle_settings, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::OpenSettings);
            return true;
        }
        if matches_binding(&kb.toggle_notifications, key, mods) {
            state.popups.toggle("notifications");
            if state.popups.is_open("notifications") {
                state.engine.notifications.mark_all_read();
            }
            return true;
        }
        if matches_binding(&kb.close_workspace, key, mods) {
            state.close_active_workspace();
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_binding(&kb.close_pane, key, mods) {
            if !state.close_active_pane() {
                state.close_active_workspace();
            }
            if !state.engine.workspaces.is_empty() {
                state.resize_all(terminal_rect, cell_w, cell_h);
            }
            return true;
        }
        if matches_binding(&kb.close_surface, key, mods) {
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
        if matches_binding(&kb.focus_pane_next, key, mods) {
            state.move_pane_focus_forward();
            return true;
        }
        if matches_binding(&kb.focus_pane_prev, key, mods) {
            state.move_pane_focus_backward();
            return true;
        }
        if matches_binding(&kb.focus_surface_next, key, mods) {
            state.move_surface_focus_forward();
            return true;
        }
        if matches_binding(&kb.focus_surface_prev, key, mods) {
            state.move_surface_focus_backward();
            return true;
        }
        if matches_binding(&kb.toggle_sidebar, key, mods) {
            state.sidebar_visible = !state.sidebar_visible;
            return true;
        }
        if matches_binding(&kb.toggle_sidebar_collapse, key, mods) {
            state.sidebar_collapsed = !state.sidebar_collapsed;
            return true;
        }
        if matches_binding(&kb.restore_closed, key, mods) {
            state.restore_closed_item();
            state.resize_all(terminal_rect, cell_w, cell_h);
            return true;
        }
        if matches_binding(&kb.quit_immediate, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::Shutdown);
            return true;
        }
        if matches_binding(&kb.quit_minimize, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::Minimize);
            return true;
        }
        if matches_binding(&kb.quit, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::QuitRequested);
            return true;
        }
        if matches_binding(&kb.open_markdown, key, mods) {
            let pane_id = state.active_workspace().focused_pane;
            state.dialogs.file_open_pane_id = Some(pane_id);
            state.dialogs.markdown_open_buffer.clear();
            state.popups.open_centered_focused("markdown_open");
            return true;
        }
        if matches_binding(&kb.open_explorer, key, mods) {
            let home = directories::BaseDirs::new()
                            .map(|d| d.home_dir().to_string_lossy().to_string())
                            .unwrap_or_else(|| ".".to_string());
            let _ = state.add_explorer_tab(home);
            return true;
        }
        if matches_binding(&kb.convert_surface, key, mods) {
            if let Some(sid) = state.focused_surface_id() {
                state.dialogs.convert_popup = Some(sid);
                state.dialogs.convert_popup_selected = None;
                state.popups.open_with_scope("convert_surface", crate::ui::popup::PopupScope::Surface(sid));
            }
            return true;
        }
        if matches_binding(&kb.convert_to_markdown, key, mods) {
            if let Some(sid) = state.focused_surface_id() {
                let pane_id = state.active_workspace().focused_pane;
                state.dialogs.markdown_convert_surface_id = Some(sid);
                state.dialogs.file_open_pane_id = Some(pane_id);
                state.dialogs.markdown_open_buffer.clear();
                state.popups.open_with_scope("markdown_open", crate::ui::popup::PopupScope::Surface(sid));
            }
            return true;
        }
        if matches_binding(&kb.convert_to_explorer, key, mods) {
            if let Some(sid) = state.focused_surface_id() {
                state.convert_surface_to_explorer(sid);
            }
            return true;
        }
        if matches_binding(&kb.new_window, key, mods) {
            let _ = proxy.send_event(crate::AppEvent::CreateWindow);
            return true;
        }
        false
    }

    fn handle_hardcoded_shortcuts(
        state: &mut crate::state::AppState,
        kb: &crate::settings::KeybindingSettings,
        key: &Key,
        ctrl: bool,
        shift: bool,
        alt: bool,
        terminal_rect: crate::model::Rect,
        cell_w: f32,
        cell_h: f32,
    ) -> bool {
        // Ctrl+Shift+Tab: previous tab
        if ctrl && shift {
            if let Key::Named(NamedKey::Tab) = key {
                state.prev_tab_in_pane();
                return true;
            }
        }

        // Ctrl+W: close tab → pane → workspace
        if ctrl && !shift && !alt {
            if let Key::Character(c) = key {
                let s = c.as_str();
                if s == "w" || s == "W" || s == "\u{17}" {
                    if !state.close_active_tab() {
                        if !state.close_active_pane() {
                            state.close_active_workspace();
                        }
                        if !state.engine.workspaces.is_empty() {
                            state.resize_all(terminal_rect, cell_w, cell_h);
                        }
                    }
                    return true;
                }
            }
        }

        // Ctrl+Tab: next tab
        if ctrl && !shift && !alt {
            if let Key::Named(NamedKey::Tab) = key {
                state.next_tab_in_pane();
                return true;
            }
        }

        // Number key tab/workspace switching
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

    fn handle_paste_shortcut(&mut self, key: &Key, ctrl: bool, shift: bool, alt: bool) -> bool {
        let clipboard = &self.state.engine.settings.clipboard;
        if let Key::Character(c) = key {
            let s = c.as_str().to_lowercase();
            let is_v = s == "v" || c.as_str() == "\u{16}";
            if is_v {
                if (ctrl && shift && clipboard.linux_style)
                    || (ctrl && !shift && !alt && clipboard.windows_style)
                    || (alt && !ctrl && !shift && clipboard.macos_style)
                {
                    self.paste_to_terminal();
                    self.mark_dirty();
                    return true;
                }
            }
        }
        false
    }

    fn handle_zoom_shortcut(
        state: &mut crate::state::AppState,
        key: &Key,
        ctrl: bool,
        shift: bool,
        alt: bool,
    ) -> bool {
        let zoom_ctrl = ctrl && !alt && state.engine.settings.zoom.ctrl_style;
        let zoom_alt = alt && !ctrl && state.engine.settings.zoom.alt_style;
        if !(zoom_ctrl || zoom_alt) {
            return false;
        }

        if let Key::Character(c) = key {
            match c.as_str() {
                "=" | "+" => {
                    let current = state.engine.settings.appearance.font_size;
                    state.engine.settings.appearance.font_size = (current + 1.0).min(72.0);
                    return true;
                }
                "-" => {
                    let current = state.engine.settings.appearance.font_size;
                    state.engine.settings.appearance.font_size = (current - 1.0).max(6.0);
                    return true;
                }
                "0" if !shift => {
                    state.engine.settings.appearance.font_size = 14.0;
                    return true;
                }
                _ => {}
            }
        }
        false
    }
}
