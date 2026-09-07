use winit::event::ElementState;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::MainView;
use crate::core::intent::{DomainIntent, SendPayload};
use crate::state::FocusedSurfaceType;
use crate::view::ui::View;

/// `decide_key_to_terminal` 의 입력 — 현재 focused terminal 의 read-only 상태.
/// UI 가 sequence 결정에 필요한 정보만 추출. terminal mut borrow 불필요.
struct KeyboardReadState {
    app_cursor: bool,
    is_alt_screen: bool,
    scroll_offset: usize,
    rows: usize,
    /// macOS "Option as Meta" 설정 값. 호출부가 cfg 분기로 채운다(비-macOS=false).
    /// 켜져 있으면 Option(Alt)+문자가 `ESC` + base 문자 Meta 시퀀스로 인코딩된다.
    option_as_meta: bool,
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
        _egui_consumed: bool,
    ) {
        // Feed all key events (Press + Release) to the double-tap detector
        self.double_tap
            .on_key_event(&event.logical_key, event.state == ElementState::Pressed);

        if event.state != ElementState::Pressed {
            return;
        }

        // 0단계: 전체화면 무대가 떠 있으면 여기서 전부 끝난다(1~9단계로 내려가지 않는다).
        if self.try_consume_fullscreen_stage_key(event) {
            return;
        }

        if self.try_consume_double_tap_key() {
            return;
        }

        if self.try_consume_escape_key(event) {
            return;
        }

        // Modals, dialogs, focused host popups, plugin egui-mesh popups block
        // keyboard input to the terminal. 판정은 egui feed 게이트(`view::main`)와
        // 같은 `AppState::keyboard_overlay_open` 을 쓴다.
        let overlay_open = self.state.keyboard_overlay_open();

        if !overlay_open && self.try_consume_shortcut_key(event) {
            return;
        }

        // vi-style 키보드 복사 모드가 활성이면 키를 가로채 PTY 로 보내지 않는다.
        if self.vi_copy.is_some() && self.try_consume_vi_key(event) {
            return;
        }

        if overlay_open {
            return;
        }

        // ── Central keyboard dispatch: route to exactly one surface ──
        let surface_type = self.state.focused_surface_type(&self.core_state);
        let typing_surface_id = self.state.focused_surface_id(&self.core_state);

        match surface_type {
            FocusedSurfaceType::Terminal => self.forward_key_to_terminal(event),
            _ => {
                // markdown/image 등 egui-mesh surface 면 plugin 으로 Key/Text forward.
                // html/empty/None 등 비-mesh surface 는 여전히 no-op.
                if let Some(sid) = self.focused_egui_mesh_surface_id() {
                    self.forward_key_to_egui_mesh(sid, event);
                } else if let Some(sid) = self.focused_attach_mesh_surface_id() {
                    // attach mesh mirror surface — 위와 동형이되 목적지가 원격.
                    self.forward_key_to_attach_mesh(sid, event);
                }
            }
        }

        if let Some(sid) = typing_surface_id {
            self.core_state.record_typing(sid);
        }
    }

    /// Ctrl/Cmd/Alt 가 눌린 동안 IME 조합(예: Korean)이 logical_key 를 조합문자로
    /// 덮어써도 physical key code 로 US 레이아웃 base 문자를 복원한다. 6단계 단축키
    /// 매칭과 9단계 터미널 포워딩이 동일 로직이라 공용 헬퍼로 통합.
    fn shortcut_lookup_key(&self, event: &winit::event::KeyEvent) -> Key {
        if self.base.modifiers.control_key()
            || self.base.modifiers.super_key()
            || self.base.modifiers.alt_key()
        {
            crate::shortcuts::physical_key_to_logical(&event.physical_key)
                .unwrap_or_else(|| event.logical_key.clone())
        } else {
            event.logical_key.clone()
        }
    }

    /// 0단계: 전체화면 무대가 활성이면 **모든 키를 여기서 소비**한다. 소비 시 true.
    ///
    /// 1~3단계(double-tap)보다 앞에 있어야 한다 — 무대는 뒤 세계와 로직상 무관하므로
    /// 뒤 세계의 어떤 단축키도(double-tap 포함) 무대 중에 발화하면 안 된다. 4단계
    /// (`try_consume_escape_key`)보다 앞인 것이 ESC 비전파의 실체다: 무대 중 ESC 는
    /// 여기서 무대만 닫고 `return` 하므로 settings/notifications 닫기 경로에 도달하지
    /// 않는다. 무대가 없으면 no-op 이라 기존 파이프라인은 그대로다.
    ///
    /// 키가 **무대 콘텐츠로 전달되는 경로는 여기가 아니다** — 무대는 egui 프레임 안에
    /// 그려지므로 `MainView::handle_event` 가 무대 중 키/IME 를 egui 입력 시스템으로
    /// 먹인다(오버레이와 같은 취급). 이 함수는 그 뒤에 남은 잔여 입력이 뒤로 새지
    /// 않게 막는 쪽만 담당한다.
    fn try_consume_fullscreen_stage_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        // 조회 키는 나머지 파이프라인과 같은 규칙(`shortcut_lookup_key`)으로 고른다 —
        // 사용자가 종료 키를 modifier 조합으로 바꾸면 그때부터 물리 키 기준 매칭이
        // 필요해지고, 기본값 ESC 는 modifier 가 없어 어느 규칙이든 같은 값이다.
        let key = self.shortcut_lookup_key(event);
        let decision = stage_key_decision(
            self.state.fullscreen_stage_active(),
            &self.core_state.settings.keybindings.fullscreen_stage_exit,
            &key,
            self.base.modifiers,
        );
        match decision {
            StageKeyDecision::PassThrough => false,
            StageKeyDecision::ExitStage => {
                self.discard_pending_double_tap();
                self.state.close_fullscreen_stage();
                self.mark_dirty();
                true
            }
            StageKeyDecision::ConsumeForStage => {
                self.discard_pending_double_tap();
                true
            }
        }
    }

    /// 무대 중 완성된 double-tap 결과를 버린다. 검출기 자체에는 press/release 를 계속
    /// 먹인다(물리 상태를 놓치면 무대를 나온 뒤 판정이 어긋난다) — 대신 완성된 결과만
    /// 여기서 소진해 무대 중에도, 무대를 나온 직후에도 발화하지 않게 한다.
    fn discard_pending_double_tap(&mut self) {
        let _discarded = self.double_tap.take();
    }

    /// 1~3단계: double-tap modifier 단축키(예: Shift+Shift) 소비. 소비 시 true.
    fn try_consume_double_tap_key(&mut self) -> bool {
        // Check for double-tap modifier shortcut (e.g. Shift+Shift)
        if let Some(dt) = self.double_tap.take() {
            if self.state.settings_open {
                // When settings are open, pass to keybinding recorder
                self.state.captured_double_tap = Some(dt.binding_str().to_string());
                self.mark_dirty();
                return true;
            } else if self.handle_double_tap_shortcut(dt) {
                self.reset_modifier_hint_reveal_timer();
                self.mark_dirty();
                return true;
            }
        }
        false
    }

    /// 등록된 단축키가 실제로 소비된 시점에 modifier-hint 표시 지연 타이머를 리셋한다
    /// — 홀드를 유지한 채 단축키를 계속 쓰는 동안에는 도움말 오버레이가 뜨지 않게
    /// 하기 위함. **키 입력 경로에서만** 호출해야 한다(원칙1) — Command Palette 가
    /// 공유하는 `dispatch_action_by_id`(`shortcuts/dispatch.rs`) 안에는 이 호출을
    /// 넣지 않는다. 홀드 중이 아니면 no-op(`ModifierHintRuntime::reset_reveal_timer_if_not_shown`).
    fn reset_modifier_hint_reveal_timer(&mut self) {
        let theme = crate::theme::theme();
        self.state
            .modifier_hint
            .reset_reveal_timer_if_not_shown(&theme);
    }

    /// 4단계: Escape 로 settings / notifications 팝업 닫기 소비. 소비 시 true.
    fn try_consume_escape_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        if event.logical_key == Key::Named(NamedKey::Escape) {
            if self.state.settings_open {
                self.state.settings_open = false;
                self.state.settings_ui_state = crate::settings_ui::SettingsUiState::new();
                self.mark_dirty();
                return true;
            }
            if self.state.popups.is_open("notifications") {
                self.state.dispatch_intent(
                    crate::intent::UiIntent::ClosePopup {
                        id: "notifications",
                    }
                    .from_user_shortcut("escape_close_notifications"),
                );
                self.mark_dirty();
                return true;
            }
            // 위 둘 다 아니면 **포커스된 host popup** 을 푼다. 이것이 키보드의 탈출구다.
            //
            // 없으면 어떻게 되는가: 포커스된 popup 은 `keyboard_overlay_open` 을 참으로
            // 만들고, 그러면 단축키 **테이블 전체**가 아래 6단계에 진입조차 못 한다. 그
            // 상태를 푸는 길이 지금까지 마우스뿐이었다 — 바깥을 클릭하면 non-sticky popup
            // 의 포커스가 풀린다(`adapters/ui/popup/draw.rs`). 키보드만 쓰는 사용자에게는
            // 그 길이 없었다.
            //
            // **새 정책이 아니라 이미 있는 정책의 두 번째 입구다.** 바깥 클릭과 같은
            // 의미로 푼다 — 포커스를 놓고, `close_on_outside_click` 인 것만 닫는다.
            // 그래서 "Escape 는 무엇을 닫는가" 를 popup 마다 새로 판단할 필요가 없다.
            //
            // 범위만 좁힌다: 포커스된 **하나**만 본다. 바깥 클릭은 좌표를 가지므로 그
            // 점을 안 담은 popup 전부를 가리킬 수 있지만 Escape 에는 좌표가 없다. 좌표
            // 없는 키를 같은 범위로 쓰면 사용자가 가리킨 적 없는 popup 까지 닫힌다.
            //
            // 순서: settings → notifications → 이것. 앞의 둘은 포커스와 무관하게(열려만
            // 있으면) 먹으므로 뒤로 밀면 동작이 바뀐다. 이 순서는
            // `tests/escape_dismisses_the_focused_popup.rs` 가 고정한다.
            if let Some((id, closes)) = self.state.popups.focused_dismissal_target() {
                self.state.popups.set_focused(id, false);
                if closes {
                    self.state.dispatch_intent(
                        crate::intent::UiIntent::ClosePopup { id }
                            .from_user_shortcut("escape_dismiss_focused_popup"),
                    );
                }
                self.mark_dirty();
                return true;
            }
        }
        false
    }

    /// 6단계: 단축키 소비 + IME 조합 중이면 flush/clear 분기(★불가침 — 조건·순서 불변).
    /// 호출부가 `!overlay_open` 을 이미 확인한 뒤 진입한다(단락평가로 원본과 순서 동일).
    fn try_consume_shortcut_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        let shortcut_key = self.shortcut_lookup_key(event);
        if self.handle_shortcut(&shortcut_key, self.base.modifiers) {
            self.after_shortcut_consumed();
            return true;
        }
        false
    }

    /// `handle_shortcut` 이 키를 소비한 직후의 후처리(★불가침 — 조건·순서 불변).
    /// winit 키 경로(`try_consume_shortcut_key`)와 native webview 포워딩 경로
    /// (`app::webview_keys`)가 **같은 후처리를 쓰도록** 여기 한 곳에 모은다 — 한쪽만
    /// 빠뜨리면 `enter_copy_mode` 같은 deferred 단축키가 webview 위에서만 죽는다.
    pub(crate) fn after_shortcut_consumed(&mut self) {
        self.reset_modifier_hint_reveal_timer();
        if self.ime_preedit.is_some() {
            // 단축키로 팝업/오버레이가 열렸으면 조합 중 문자를 PTY로 보내지 않고 버린다.
            // 그 외 단축키(split, close 등)는 조합 문자를 확정 전송한다.
            // plugin popup 은 이 시점에 아직 캐시에 반영되지 않았다(open 은
            // `pending_popup_opens` 를 거쳐 다음 tick 에 실행된다) — 그래서 큐가
            // 비어 있지 않은지로 "방금 이 단축키가 popup 을 요청했는가" 를 본다.
            if self.state.popups.has_focused()
                || self.state.plugin_popup_open
                || !self.state.pending_popup_opens.is_empty()
            {
                self.clear_ime_preedit();
            } else {
                self.flush_ime_preedit();
            }
        }
        // enter_copy_mode 같은 단축키가 신호한 deferred 작업 처리.
        self.try_enter_vi_copy_mode();
        self.mark_dirty();
    }

    /// 7단계: vi copy-mode 활성 시 키 가로채기. Ctrl-only 폴백이라(6·9단계와 조건이
    /// 달라) shortcut_lookup_key 로 통합하지 않고 내부에 verbatim 유지. 소비 시 true.
    fn try_consume_vi_key(&mut self, event: &winit::event::KeyEvent) -> bool {
        let vi_key = if self.base.modifiers.control_key() {
            crate::shortcuts::physical_key_to_logical(&event.physical_key)
                .unwrap_or_else(|| event.logical_key.clone())
        } else {
            event.logical_key.clone()
        };
        if self.try_handle_vi_key(&vi_key, self.base.modifiers) {
            self.mark_dirty();
            return true;
        }
        false
    }

    /// 9단계: 포커스된 터미널로 키 포워딩. IME 활성 시 non-ASCII text 억제(Commit 처리),
    /// modifier 시 physical 폴백(Ctrl+letter). scroll 은 borrow 분리를 위해 별 메서드로.
    fn forward_key_to_terminal(&mut self, event: &winit::event::KeyEvent) {
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
        let terminal_key = self.shortcut_lookup_key(event);

        // option_as_meta 필드는 macOS 전용(#[cfg(target_os = "macos")]) 이므로
        // 비-macOS 빌드가 깨지지 않도록 cfg 분기로 값을 산출해 항상 bool 을 넣는다.
        // 이렇게 하면 decide_key_to_terminal 본문은 플랫폼 무관하게 유지된다.
        #[cfg(target_os = "macos")]
        let option_as_meta = self.core_state.settings.general.option_as_meta;
        #[cfg(not(target_os = "macos"))]
        let option_as_meta = false;

        let read_state = self
            .state
            .focused_terminal(&self.core_state)
            .map(|t| KeyboardReadState {
                app_cursor: t.application_cursor_keys(),
                is_alt_screen: t.is_alternate_screen(),
                scroll_offset: t.scroll_offset(),
                rows: t.rows(),
                option_as_meta,
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

            self.apply_keyboard_scroll_action(outcome.scroll_action);

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

    /// 9단계(egui-mesh 변형): 포커스된 egui-mesh surface(markdown/image 등)로 키 누름 +
    /// 텍스트를 forward. terminal forward 와 동형이되 대상이 plugin egui `TextEdit` 이라,
    /// Key wire 이벤트 + (조건부) Text wire 이벤트를 surface 입력 큐에 누적한다.
    /// Text 는 [`should_forward_text`] 로 걸러 command modifier·제어문자·IME 조합 중
    /// non-ASCII 를 억제한다(조합 결과는 IME `Commit` 으로 별도 도착 — `ime.rs`).
    fn forward_key_to_egui_mesh(&mut self, surface_id: u32, event: &winit::event::KeyEvent) {
        self.egui_mesh_push_key(surface_id, event);

        if let Some(text) = &event.text {
            let is_cmd = self.base.modifiers.control_key() || self.base.modifiers.super_key();
            if should_forward_text(text.as_str(), is_cmd, self.ime_active) {
                self.egui_mesh_push_text(surface_id, text.as_str());
            }
        }
        self.mark_dirty();
    }

    /// [`Self::forward_key_to_egui_mesh`]의 attach mesh mirror 대응 — 목적지가
    /// 원격 plugin 이라는 점만 다르다.
    fn forward_key_to_attach_mesh(&mut self, surface_id: u32, event: &winit::event::KeyEvent) {
        self.attach_mesh_push_key(surface_id, event);

        if let Some(text) = &event.text {
            let is_cmd = self.base.modifiers.control_key() || self.base.modifiers.super_key();
            if should_forward_text(text.as_str(), is_cmd, self.ime_active) {
                self.attach_mesh_push_text(surface_id, text.as_str());
            }
        }
        self.mark_dirty();
    }

    /// 9단계 내 scroll match. `forward_key_to_terminal` 이 `focused_terminal`(read)
    /// 로 read_state 를 만든 뒤 scroll 은 `focused_terminal_mut`(write) 재차용이
    /// 필요해, read borrow 종료 후 mut borrow 하도록 별 메서드로 분리(borrow checker).
    fn apply_keyboard_scroll_action(&mut self, action: KeyboardScrollAction) {
        match action {
            KeyboardScrollAction::None => {}
            KeyboardScrollAction::ScrollUp(n) => {
                if let Some(terminal) = self.state.focused_terminal_mut(&mut self.core_state) {
                    terminal.scroll_up(n);
                }
            }
            KeyboardScrollAction::ScrollDown(n) => {
                if let Some(terminal) = self.state.focused_terminal_mut(&mut self.core_state) {
                    terminal.scroll_down(n);
                }
            }
            KeyboardScrollAction::ScrollToBottom => {
                if let Some(terminal) = self.state.focused_terminal_mut(&mut self.core_state) {
                    terminal.scroll_to_bottom();
                }
            }
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
                // macOS "Option as Meta": Option(Alt)+문자를 ESC-prefix Meta 시퀀스로
                // 인코딩한다. 물리 Option 키만(Ctrl/Cmd 동시 누름 제외) 대상이며, base
                // 문자(=key 파라미터, Alt 시 이미 physical 기반 US 레이아웃 문자)를 쓴다.
                // 합성된 특수문자(text, 예: å)가 아니라 base 문자('a')를 ESC 와 묶어야 한다.
                if state.option_as_meta
                    && modifiers.alt_key()
                    && !modifiers.control_key()
                    && !modifiers.super_key()
                    && let Key::Character(c) = key
                    && let Some(ch) = c.chars().next()
                {
                    let mut buf = vec![0x1b_u8];
                    let mut utf8 = [0u8; 4];
                    buf.extend_from_slice(ch.encode_utf8(&mut utf8).as_bytes());
                    push_bytes(&mut payloads, &buf);
                    sent = true;
                    // text 분기로 내려가 특수문자가 중복 전송되지 않도록 early return.
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

/// egui-mesh surface 로 `Text` wire 이벤트를 forward 할지 판정(egui-winit 미러 +
/// IME 억제). 억제 조건: 빈 문자열 / 제어·사설영역 문자(Delete 의 `\u{f728}` 등) /
/// command modifier 동반(Ctrl+key 는 문자 삽입 아님) / IME 조합 중 non-ASCII(조합 결과는
/// IME `Commit` 으로 도착하므로 중복 방지 — ASCII 숫자·기호는 조합을 안 거쳐 통과).
fn should_forward_text(text: &str, is_cmd: bool, ime_active: bool) -> bool {
    if text.is_empty() || is_cmd {
        return false;
    }
    // IME 조합 중 non-ASCII 는 Commit 으로 도착하므로 여기선 억제.
    if ime_active && !text.is_ascii() {
        return false;
    }
    text.chars().all(is_printable_char)
}

/// 0단계 무대 게이트의 순수 판정([`MainView::try_consume_fullscreen_stage_key`]).
/// `MainView` 는 실제 GPU/winit 컨텍스트 없이 구성할 수 없어 게이트 메서드 자체를 단위
/// 테스트로 못 돌린다 — mouse.rs 의 `cursor_moved_should_short_circuit` 과 같은 이유로
/// 판정만 순수 함수로 떼어 테스트한다.
#[derive(Debug, PartialEq, Eq)]
enum StageKeyDecision {
    /// 무대 없음 — 기존 파이프라인(1단계~)으로 그대로 흘린다.
    PassThrough,
    /// 무대 있음 + 종료 키 — 무대를 닫고 소비. 뒤로 전파하지 않는다.
    ExitStage,
    /// 무대 있음 + 그 외 키 — 무대 콘텐츠(egui)가 이미 받았다. 뒤로 전파하지 않는다.
    ConsumeForStage,
}

fn stage_key_decision(
    stage_active: bool,
    exit_bindings: &[String],
    key: &Key,
    mods: ModifiersState,
) -> StageKeyDecision {
    if !stage_active {
        return StageKeyDecision::PassThrough;
    }
    if stage_exit_key_matches(exit_bindings, key, mods) {
        StageKeyDecision::ExitStage
    } else {
        StageKeyDecision::ConsumeForStage
    }
}

/// 무대 종료 키 판정 — **바인딩 값을 읽는 단 하나의 지점**. 값 자체는
/// `KeybindingSettings::fullscreen_stage_exit`(기본 `["escape"]`)에 있고 여기서는
/// 조회만 한다(CLAUDE.md 단축키 정책: 하드코딩 금지).
///
/// 이 조회가 **무대 게이트 안에만** 있는 것이 회귀 방지의 핵심이다 — 무대가 없으면
/// `stage_key_decision` 이 여기 오기 전에 `PassThrough` 로 빠지므로, 기본값 ESC 가
/// settings/notifications 닫기나 터미널 `\x1b` 전달을 훔칠 경로가 아예 없다.
///
/// 바인딩이 빈 vec 이면 항상 false 다 — 키보드 종료 수단만 사라지고, 무대 셸이 항상
/// 그리는 종료 버튼(`adapters::ui::fullscreen::draw_fullscreen_stage`)이 남으므로
/// 탈출 불가 상태가 되지 않는다.
fn stage_exit_key_matches(exit_bindings: &[String], key: &Key, mods: ModifiersState) -> bool {
    crate::shortcuts::matches_any_binding(exit_bindings, key, mods)
}

/// egui-winit `is_printable_char` 미러 — ASCII 제어문자와 유니코드 사설 사용 영역
/// (일부 플랫폼이 Delete/기능키를 이 영역 문자로 보냄)을 비인쇄로 걸러낸다.
fn is_printable_char(chr: char) -> bool {
    let is_in_private_use_area = ('\u{e000}'..='\u{f8ff}').contains(&chr)
        || ('\u{f0000}'..='\u{ffffd}').contains(&chr)
        || ('\u{100000}'..='\u{10fffd}').contains(&chr);
    !is_in_private_use_area && !chr.is_ascii_control()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn esc() -> Key {
        Key::Named(NamedKey::Escape)
    }

    fn no_mods() -> ModifiersState {
        ModifiersState::empty()
    }

    /// 바인딩 문자열의 `alt` 토큰이 실제로 요구하는 winit modifier.
    ///
    /// modifier 매핑은 물리 위치 기반이라 macOS 에서 바인딩 `alt` 는 Command(winit
    /// `SUPER`) 에, 그 외 플랫폼에서는 Alt 에 대응한다(`matches_binding` 의 플랫폼
    /// 분기, `docs/design/policies/key-mapping.md`). 여기에 `ModifiersState::ALT` 를
    /// 직접 쓰면 macOS 에서는 Option 이 눌린 것이라 `alt` 바인딩과 매칭되지 않는다.
    #[cfg(target_os = "macos")]
    const BINDING_ALT: ModifiersState = ModifiersState::SUPER;
    #[cfg(not(target_os = "macos"))]
    const BINDING_ALT: ModifiersState = ModifiersState::ALT;

    /// 게이트가 실제로 조회하는 값과 같은 출처를 쓴다 — 기본 프리셋의
    /// `fullscreen_stage_exit`. 여기에 리터럴 `["escape"]` 를 쓰면 프리셋이 바뀌어도
    /// 테스트가 통과해 버린다.
    fn default_exit_bindings() -> Vec<String> {
        crate::settings::KeybindingSettings::default().fullscreen_stage_exit
    }

    /// 무대 중 ESC 는 0단계에서 끝난다 — 4단계(`try_consume_escape_key`, settings 모달·
    /// notifications 팝업 닫기)에 **도달하지 않는다**. 0단계가 4단계보다 앞에 있고
    /// `ExitStage` 가 즉시 `return` 으로 이어진다는 배선은
    /// `tests/fullscreen_stage_input_gate.rs` 가 구조로 고정한다.
    #[test]
    fn stage_gate_blocks_escape_from_reaching_popup_close() {
        assert_eq!(
            stage_key_decision(true, &default_exit_bindings(), &esc(), no_mods()),
            StageKeyDecision::ExitStage
        );
    }

    /// 무대가 없으면 0단계는 no-op — ESC 가 기존 4단계 경로로 그대로 내려간다.
    #[test]
    fn stage_gate_is_noop_when_no_stage() {
        assert_eq!(
            stage_key_decision(false, &default_exit_bindings(), &esc(), no_mods()),
            StageKeyDecision::PassThrough
        );
        assert_eq!(
            stage_key_decision(
                false,
                &default_exit_bindings(),
                &Key::Character("a".into()),
                no_mods()
            ),
            StageKeyDecision::PassThrough
        );
    }

    /// 종료 키가 아닌 키는 무대가 소비한다 — 단축키(6단계)·vi(7단계)·터미널
    /// forward(9단계) 어디로도 내려가지 않는다.
    #[test]
    fn stage_swallows_every_other_key() {
        for key in [
            Key::Character("a".into()),
            Key::Named(NamedKey::Enter),
            Key::Named(NamedKey::Tab),
            Key::Named(NamedKey::ArrowDown),
        ] {
            assert_eq!(
                stage_key_decision(true, &default_exit_bindings(), &key, no_mods()),
                StageKeyDecision::ConsumeForStage,
                "{key:?} 가 무대를 뚫고 내려갔다"
            );
        }
    }

    /// 종료 키 판정은 한 곳(`stage_exit_key_matches`)에만 있고, 값은
    /// `KeybindingSettings` 에서 온다 — 기본 프리셋이 ESC 다.
    #[test]
    fn stage_exit_key_is_escape_by_default() {
        let kb = default_exit_bindings();
        assert!(stage_exit_key_matches(&kb, &esc(), no_mods()));
        assert!(!stage_exit_key_matches(
            &kb,
            &Key::Named(NamedKey::Enter),
            no_mods()
        ));
    }

    /// 사용자가 바인딩을 바꾸면 그 키로 닫히고 ESC 로는 안 닫힌다 — 값이 정말
    /// 설정에서 오는지(하드코딩이 남아 있지 않은지)를 가르는 단정이다. 모디파이어는
    /// 플랫폼 규칙(`BINDING_ALT`)으로 만들어 세 플랫폼 모두에서 같은 검증을 유지한다.
    #[test]
    fn stage_exit_follows_the_configured_binding() {
        let rebound = vec!["ctrl+alt+q".to_string()];
        let ctrl_alt = ModifiersState::CONTROL | BINDING_ALT;
        assert_eq!(
            stage_key_decision(true, &rebound, &Key::Character("q".into()), ctrl_alt),
            StageKeyDecision::ExitStage
        );
        assert_eq!(
            stage_key_decision(true, &rebound, &esc(), no_mods()),
            StageKeyDecision::ConsumeForStage,
            "재바인딩 후에도 ESC 가 무대를 닫았다 — 하드코딩이 남아 있다"
        );
    }

    /// 바인딩이 비면 키보드 종료 수단만 사라진다 — 다른 키가 뒤로 새지 않는 성질은
    /// 그대로다. 탈출은 무대 셸이 항상 그리는 종료 버튼이 담당한다.
    #[test]
    fn empty_binding_leaves_the_stage_up_without_leaking_keys() {
        assert_eq!(
            stage_key_decision(true, &[], &esc(), no_mods()),
            StageKeyDecision::ConsumeForStage
        );
        assert_eq!(
            stage_key_decision(true, &[], &Key::Named(NamedKey::Enter), no_mods()),
            StageKeyDecision::ConsumeForStage
        );
    }

    fn read_state(option_as_meta: bool) -> KeyboardReadState {
        KeyboardReadState {
            app_cursor: false,
            is_alt_screen: false,
            scroll_offset: 0,
            rows: 24,
            option_as_meta,
        }
    }

    /// payload 들을 평탄화해 바이트 열로 모은다(Bytes/Text 모두 UTF-8 바이트로).
    fn collect_bytes(payloads: &[SendPayload]) -> Vec<u8> {
        let mut out = Vec::new();
        for p in payloads {
            match p {
                SendPayload::Bytes(b) => out.extend_from_slice(b),
                SendPayload::Text(s) => out.extend_from_slice(s.as_bytes()),
            }
        }
        out
    }

    // option_as_meta = true, Option(Alt)+'a' → ESC + base 'a' (합성문자 'å' 아님).
    #[test]
    fn option_as_meta_prefixes_esc() {
        let key = Key::Character("a".into());
        let text: Option<winit::keyboard::SmolStr> = Some("å".into());
        let out =
            MainView::decide_key_to_terminal(read_state(true), &key, &text, ModifiersState::ALT);
        assert_eq!(collect_bytes(&out.payloads), vec![0x1b, b'a']);
        assert!(out.sent);
    }

    // option_as_meta = false 이면 기존 동작(합성문자 'å' text 전송) 유지.
    #[test]
    fn option_as_meta_off_keeps_compose() {
        let key = Key::Character("a".into());
        let text: Option<winit::keyboard::SmolStr> = Some("å".into());
        let out =
            MainView::decide_key_to_terminal(read_state(false), &key, &text, ModifiersState::ALT);
        assert_eq!(collect_bytes(&out.payloads), "å".as_bytes());
        assert!(out.sent);
    }

    // option_as_meta = true 라도 Ctrl 이 함께 눌리면 Meta 분기로 빠지지 않는다
    // (Ctrl+letter control char 우선).
    #[test]
    fn option_as_meta_with_ctrl_is_control_char() {
        let key = Key::Character("a".into());
        let text: Option<winit::keyboard::SmolStr> = None;
        let out = MainView::decide_key_to_terminal(
            read_state(true),
            &key,
            &text,
            ModifiersState::ALT | ModifiersState::CONTROL,
        );
        // Ctrl 분기는 alt 가 눌리면 배제(`control_key() && !alt_key()`)되고, Meta
        // 분기는 control 이 눌리면 배제(`!control_key()`)되므로 둘 다 안 타고,
        // text 가 None 이라 아무것도 전송되지 않는다.
        assert!(out.payloads.is_empty());
        assert!(!out.sent);
    }

    // egui-mesh Text forward 게이트 — 일반 문자는 통과.
    #[test]
    fn forward_text_passes_plain_char() {
        assert!(should_forward_text("a", false, false));
        assert!(should_forward_text("가", false, false));
    }

    // command modifier(Ctrl/Cmd) 동반 문자는 억제(단축키·제어 삽입 방지).
    #[test]
    fn forward_text_suppresses_command_modifier() {
        assert!(!should_forward_text("a", true, false));
    }

    // 제어문자·사설영역 문자(예: macOS Delete `\u{f728}`)는 억제.
    #[test]
    fn forward_text_suppresses_control_and_private_use() {
        assert!(!should_forward_text("\u{7f}", false, false)); // DEL
        assert!(!should_forward_text("\u{f728}", false, false)); // macOS delete glyph
        assert!(!should_forward_text("", false, false)); // 빈 문자열
    }

    // IME 조합 중: non-ASCII 는 억제(조합 결과는 Commit 으로 도착), ASCII 는 통과.
    #[test]
    fn forward_text_ime_suppresses_non_ascii_only() {
        assert!(!should_forward_text("한", false, true));
        assert!(should_forward_text("1", false, true));
        assert!(should_forward_text(",", false, true));
        // IME 비활성이면 non-ASCII 도 통과(합성 문자 직접 입력).
        assert!(should_forward_text("한", false, false));
    }
}
