use winit::event::ElementState;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::MainView;
use crate::core::intent::{DomainIntent, SendPayload};
use crate::state::{FocusedSurfaceType, PendingKeyEvent};
use crate::view::ui::View;

/// `decide_key_to_terminal` 의 입력 — 현재 focused terminal 의 read-only 상태.
/// UI 가 sequence 결정에 필요한 정보만 추출. terminal mut borrow 불필요.
struct KeyboardReadState {
    app_cursor: bool,
    is_alt_screen: bool,
    scroll_offset: usize,
    rows: usize,
}

/// 키 처리 후 UI 가 직접 수행해야 하는 *터미널 자체 mutate* 동작. PTY input 과
/// 무관 (옛 코드의 `terminal.scroll_*` / `scroll_to_bottom` 호출 분리).
enum KeyboardScrollAction {
    None,
    ScrollUp(usize),
    ScrollDown(usize),
    ScrollToBottom,
}

/// `decide_key_to_terminal` 의 반환. PTY input 은 `payloads` 로 Intent 큐잉,
/// 터미널 자체 mutate 는 `scroll_action` 으로 호출자가 처리.
struct KeyboardSendOutcome {
    payloads: Vec<SendPayload>,
    scroll_action: KeyboardScrollAction,
    dirty: bool,
    sent: bool,
}

impl MainView {
    pub(super) fn handle_keyboard_input(
        &mut self,
        event: &winit::event::KeyEvent,
        egui_consumed: bool,
    ) {
        // Feed all key events (Press + Release) to the double-tap detector
        self.double_tap
            .on_key_event(&event.logical_key, event.state == ElementState::Pressed);

        if event.state != ElementState::Pressed {
            return;
        }

        // Check for double-tap modifier shortcut (e.g. Shift+Shift)
        if let Some(dt) = self.double_tap.take() {
            if self.state.settings_open {
                // When settings are open, pass to keybinding recorder
                self.state.captured_double_tap = Some(dt.binding_str().to_string());
                self.mark_dirty();
                return;
            } else if self.handle_double_tap_shortcut(dt) {
                self.mark_dirty();
                return;
            }
        }

        if event.logical_key == Key::Named(NamedKey::Escape) {
            if self.state.settings_open {
                self.state.settings_open = false;
                self.state.settings_ui_state = crate::settings_ui::SettingsUiState::new();
                self.mark_dirty();
                return;
            }
            if self.state.popups.is_open("notifications") {
                self.state.dispatch_intent(
                    crate::intent::UiIntent::ClosePopup {
                        id: "notifications",
                    }
                    .from_user_shortcut("escape_close_notifications"),
                );
                self.mark_dirty();
                return;
            }
        }

        // Modals, dialogs, and focused popups block keyboard input to the terminal.
        let overlay_open = self.state.settings_open
            || self.state.has_input_dialog_open()
            || self.state.popups.has_focused();

        if !overlay_open {
            // On macOS, IME composition (e.g. Korean) can replace the logical key
            // with the composed character. When modifier keys are held, use the
            // physical key code to determine the intended key for shortcut matching.
            let shortcut_key = if self.base.modifiers.control_key()
                || self.base.modifiers.super_key()
                || self.base.modifiers.alt_key()
            {
                crate::shortcuts::physical_key_to_logical(&event.physical_key)
                    .unwrap_or_else(|| event.logical_key.clone())
            } else {
                event.logical_key.clone()
            };
            if self.handle_shortcut(&shortcut_key, self.base.modifiers) {
                if self.ime_preedit.is_some() {
                    // 단축키로 팝업/오버레이가 열렸으면 조합 중 문자를 PTY로 보내지 않고 버린다.
                    // 그 외 단축키(split, close 등)는 조합 문자를 확정 전송한다.
                    if self.state.popups.has_focused() {
                        self.clear_ime_preedit();
                    } else {
                        self.flush_ime_preedit();
                    }
                }
                // enter_copy_mode 같은 단축키가 신호한 deferred 작업 처리.
                self.try_enter_vi_copy_mode();
                self.mark_dirty();
                return;
            }
        }

        // vi-style 키보드 복사 모드가 활성이면 키를 가로채 PTY 로 보내지 않는다.
        if self.vi_copy.is_some() {
            let vi_key = if self.base.modifiers.control_key() {
                crate::shortcuts::physical_key_to_logical(&event.physical_key)
                    .unwrap_or_else(|| event.logical_key.clone())
            } else {
                event.logical_key.clone()
            };
            if self.try_handle_vi_key(&vi_key, self.base.modifiers) {
                self.mark_dirty();
                return;
            }
        }
        if overlay_open {
            return;
        }

        // ── Central keyboard dispatch: route to exactly one surface ──
        let surface_type = self.state.focused_surface_type(&self.core_state);
        let typing_surface_id = self.state.focused_surface_id(&self.core_state);

        match surface_type {
            FocusedSurfaceType::Terminal => {
                // Forward to terminal.
                // When IME is active, suppress non-ASCII text (Korean/Chinese/Japanese
                // composition — Ime::Commit will handle it). ASCII text (numbers,
                // punctuation like 1234567890,./) passes through IME unchanged and
                // won't generate Ime::Commit, so we must send it here.
                let text_for_terminal = if self.ime_active {
                    match &event.text {
                        Some(t) if t.as_str().is_ascii() => &event.text,
                        _ => &None,
                    }
                } else {
                    &event.text
                };
                // When modifiers are held, prefer the physical key for Ctrl+letter
                // handling so that IME composition (e.g. Korean 'ㅊ' for 'c') doesn't
                // prevent control characters from being sent.
                let terminal_key = if self.base.modifiers.control_key()
                    || self.base.modifiers.super_key()
                    || self.base.modifiers.alt_key()
                {
                    crate::shortcuts::physical_key_to_logical(&event.physical_key)
                        .unwrap_or_else(|| event.logical_key.clone())
                } else {
                    event.logical_key.clone()
                };

                let read_state =
                    self.state
                        .focused_terminal(&self.core_state)
                        .map(|t| KeyboardReadState {
                            app_cursor: t.application_cursor_keys(),
                            is_alt_screen: t.is_alternate_screen(),
                            scroll_offset: t.scroll_offset(),
                            rows: t.rows(),
                        });
                let surface_id = self.state.focused_surface_id(&self.core_state);

                if let (Some(rs), Some(sid)) = (read_state, surface_id) {
                    let outcome = Self::decide_key_to_terminal(
                        rs,
                        &terminal_key,
                        text_for_terminal,
                        self.base.modifiers,
                    );

                    for payload in outcome.payloads {
                        self.state.dispatch_intent(
                            DomainIntent::SendToSurface {
                                surface_id: sid,
                                payload,
                            }
                            .from_user_shortcut("keyboard_input"),
                        );
                    }

                    match outcome.scroll_action {
                        KeyboardScrollAction::None => {}
                        KeyboardScrollAction::ScrollUp(n) => {
                            if let Some(terminal) =
                                self.state.focused_terminal_mut(&mut self.core_state)
                            {
                                terminal.scroll_up(n);
                            }
                        }
                        KeyboardScrollAction::ScrollDown(n) => {
                            if let Some(terminal) =
                                self.state.focused_terminal_mut(&mut self.core_state)
                            {
                                terminal.scroll_down(n);
                            }
                        }
                        KeyboardScrollAction::ScrollToBottom => {
                            if let Some(terminal) =
                                self.state.focused_terminal_mut(&mut self.core_state)
                            {
                                terminal.scroll_to_bottom();
                            }
                        }
                    }

                    if outcome.dirty {
                        self.base.dirty = true;
                    }

                    if outcome.sent {
                        self.ime_cursor_advance = 0;
                        if self.text_selection.is_some() {
                            self.text_selection = None;
                            self.base.dirty = true;
                        }
                    }
                }
            }
            FocusedSurfaceType::Kind(ref kind) if kind == "markdown" || kind == "image" => {
                // If egui consumed the event (e.g. TextEdit has focus), skip
                // the PendingKeyEvent queue to avoid double-handling.
                if !egui_consumed {
                    self.state.pending_surface_keys.push(PendingKeyEvent {
                        key: event.logical_key.clone(),
                    });
                }
                self.mark_dirty();
            }
            _ => {
                // html, empty, None — no keyboard handling needed here
            }
        }

        if let Some(sid) = typing_surface_id {
            self.core_state.record_typing(sid);
        }
    }

    /// 키 입력을 PTY payload + scroll action 으로 변환. terminal mutate 0 —
    /// 호출자가 `KeyboardSendOutcome` 으로 받아 dispatch_intent + scroll_action
    /// 분리 처리. application_cursor_keys / is_alternate_screen / scroll_offset
    /// / rows 는 호출자가 미리 read 해서 `KeyboardReadState` 로 전달.
    fn decide_key_to_terminal(
        state: KeyboardReadState,
        key: &Key,
        text: &Option<winit::keyboard::SmolStr>,
        modifiers: ModifiersState,
    ) -> KeyboardSendOutcome {
        let mut payloads: Vec<SendPayload> = Vec::new();
        let mut scroll_action = KeyboardScrollAction::None;
        let mut dirty = false;
        let mut sent = false;

        let is_scrollback_key = !state.is_alt_screen
            && matches!(
                key.as_ref(),
                Key::Named(NamedKey::PageUp) | Key::Named(NamedKey::PageDown)
            );

        let push_bytes = |payloads: &mut Vec<SendPayload>, bytes: &[u8]| {
            payloads.push(SendPayload::Bytes(bytes.to_vec()));
        };

        match key.as_ref() {
            Key::Named(NamedKey::Enter) => {
                if modifiers.shift_key() {
                    // Kitty keyboard protocol: CSI 13 ; 2 u (Shift+Enter)
                    push_bytes(&mut payloads, b"\x1b[13;2u");
                } else {
                    push_bytes(&mut payloads, b"\r");
                }
                sent = true;
            }
            Key::Named(NamedKey::Backspace) => {
                push_bytes(&mut payloads, b"\x7f");
                sent = true;
            }
            Key::Named(NamedKey::Tab) => {
                if modifiers.shift_key() {
                    push_bytes(&mut payloads, b"\x1b[Z");
                } else {
                    push_bytes(&mut payloads, b"\t");
                }
                sent = true;
            }
            Key::Named(NamedKey::Escape) => {
                push_bytes(&mut payloads, b"\x1b");
                sent = true;
            }
            Key::Named(NamedKey::ArrowUp) => {
                push_bytes(
                    &mut payloads,
                    if state.app_cursor {
                        b"\x1bOA"
                    } else {
                        b"\x1b[A"
                    },
                );
                sent = true;
            }
            Key::Named(NamedKey::ArrowDown) => {
                push_bytes(
                    &mut payloads,
                    if state.app_cursor {
                        b"\x1bOB"
                    } else {
                        b"\x1b[B"
                    },
                );
                sent = true;
            }
            Key::Named(NamedKey::ArrowRight) => {
                push_bytes(
                    &mut payloads,
                    if state.app_cursor {
                        b"\x1bOC"
                    } else {
                        b"\x1b[C"
                    },
                );
                sent = true;
            }
            Key::Named(NamedKey::ArrowLeft) => {
                push_bytes(
                    &mut payloads,
                    if state.app_cursor {
                        b"\x1bOD"
                    } else {
                        b"\x1b[D"
                    },
                );
                sent = true;
            }
            Key::Named(NamedKey::Home) => {
                push_bytes(&mut payloads, b"\x1b[H");
                sent = true;
            }
            Key::Named(NamedKey::End) => {
                push_bytes(&mut payloads, b"\x1b[F");
                sent = true;
            }
            Key::Named(NamedKey::PageUp) => {
                if state.is_alt_screen {
                    push_bytes(&mut payloads, b"\x1b[5~");
                    sent = true;
                } else {
                    scroll_action = KeyboardScrollAction::ScrollUp(state.rows);
                    dirty = true;
                }
            }
            Key::Named(NamedKey::PageDown) => {
                if state.is_alt_screen {
                    push_bytes(&mut payloads, b"\x1b[6~");
                    sent = true;
                } else {
                    scroll_action = KeyboardScrollAction::ScrollDown(state.rows);
                    dirty = true;
                }
            }
            Key::Named(NamedKey::Insert) => {
                push_bytes(&mut payloads, b"\x1b[2~");
                sent = true;
            }
            Key::Named(NamedKey::Delete) => {
                push_bytes(&mut payloads, b"\x1b[3~");
                sent = true;
            }
            Key::Named(NamedKey::F1) => {
                push_bytes(&mut payloads, b"\x1bOP");
                sent = true;
            }
            Key::Named(NamedKey::F2) => {
                push_bytes(&mut payloads, b"\x1bOQ");
                sent = true;
            }
            Key::Named(NamedKey::F3) => {
                push_bytes(&mut payloads, b"\x1bOR");
                sent = true;
            }
            Key::Named(NamedKey::F4) => {
                push_bytes(&mut payloads, b"\x1bOS");
                sent = true;
            }
            Key::Named(NamedKey::F5) => {
                push_bytes(&mut payloads, b"\x1b[15~");
                sent = true;
            }
            Key::Named(NamedKey::F6) => {
                push_bytes(&mut payloads, b"\x1b[17~");
                sent = true;
            }
            Key::Named(NamedKey::F7) => {
                push_bytes(&mut payloads, b"\x1b[18~");
                sent = true;
            }
            Key::Named(NamedKey::F8) => {
                push_bytes(&mut payloads, b"\x1b[19~");
                sent = true;
            }
            Key::Named(NamedKey::F9) => {
                push_bytes(&mut payloads, b"\x1b[20~");
                sent = true;
            }
            Key::Named(NamedKey::F10) => {
                push_bytes(&mut payloads, b"\x1b[21~");
                sent = true;
            }
            Key::Named(NamedKey::F11) => {
                push_bytes(&mut payloads, b"\x1b[23~");
                sent = true;
            }
            Key::Named(NamedKey::F12) => {
                push_bytes(&mut payloads, b"\x1b[24~");
                sent = true;
            }
            _ => {
                // Ctrl+letter → send control character (0x01-0x1A)
                if modifiers.control_key()
                    && !modifiers.alt_key()
                    && let Key::Character(c) = key
                    && let Some(ch) = c.chars().next()
                    && ch.is_ascii_alphabetic()
                {
                    let ctrl_char = (ch.to_ascii_lowercase() as u8) - b'a' + 1;
                    push_bytes(&mut payloads, &[ctrl_char]);
                    sent = true;
                    // 옛 코드의 early return (scroll_to_bottom 분기 우회).
                    return KeyboardSendOutcome {
                        payloads,
                        scroll_action,
                        dirty,
                        sent,
                    };
                }
                if let Some(text) = text {
                    let s = text.as_str();
                    if !s.is_empty() {
                        payloads.push(SendPayload::Text(s.to_string()));
                        sent = true;
                    }
                }
            }
        }
        // Scroll to bottom only when actual content was sent to the terminal,
        // not on modifier-only keypresses (Ctrl, Cmd, Shift, Alt).
        // PageUp/PageDown (scrollback) 의 scroll_action 을 덮어쓰지 않도록 None 일 때만.
        if sent
            && !is_scrollback_key
            && state.scroll_offset > 0
            && matches!(scroll_action, KeyboardScrollAction::None)
        {
            scroll_action = KeyboardScrollAction::ScrollToBottom;
            dirty = true;
        }

        KeyboardSendOutcome {
            payloads,
            scroll_action,
            dirty,
            sent,
        }
    }

    pub(super) fn handle_ime(&mut self, ime_event: winit::event::Ime, egui_consumed: bool) {
        super::ime::handle_event(self, ime_event, egui_consumed);
    }
}
