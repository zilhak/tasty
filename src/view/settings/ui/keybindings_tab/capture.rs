use super::KeyCapture;

pub fn capture_winit_key_combo(
    event: &winit::event::KeyEvent,
    modifiers: winit::keyboard::ModifiersState,
) -> KeyCapture {
    use winit::event::ElementState;
    use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};

    if event.state != ElementState::Pressed {
        return KeyCapture::None;
    }

    // Escape → clear
    if event.logical_key == Key::Named(NamedKey::Escape) {
        return KeyCapture::Clear;
    }

    // modifier-only 키는 무시
    if let Key::Named(n) = &event.logical_key
        && matches!(
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
        )
    {
        return KeyCapture::None;
    }

    // 물리 키에서 키 이름 결정 (IME/Option 변환에 영향받지 않도록)
    let key_name =
        physical_key_to_name(&event.physical_key).or_else(|| named_key_to_name(&event.logical_key));
    let Some(key_name) = key_name else {
        return KeyCapture::None;
    };

    // modifier 조합
    let mut parts = Vec::new();
    if modifiers.control_key() {
        parts.push("ctrl");
    }
    // macOS: Cmd(⌘) = "alt" (물리적 위치가 Win/Linux Alt와 동일)
    #[cfg(target_os = "macos")]
    if modifiers.super_key() {
        parts.push("alt");
    }
    #[cfg(not(target_os = "macos"))]
    if modifiers.alt_key() {
        parts.push("alt");
    }
    // macOS: Option 키 = "option"
    #[cfg(target_os = "macos")]
    if modifiers.alt_key() {
        parts.push("option");
    }
    if modifiers.shift_key() {
        parts.push("shift");
    }

    // modifier 없는 타이핑 키는 단축키로 등록 불가
    let is_typing_key = matches!(
        event.physical_key,
        PhysicalKey::Code(
            KeyCode::KeyA
                | KeyCode::KeyB
                | KeyCode::KeyC
                | KeyCode::KeyD
                | KeyCode::KeyE
                | KeyCode::KeyF
                | KeyCode::KeyG
                | KeyCode::KeyH
                | KeyCode::KeyI
                | KeyCode::KeyJ
                | KeyCode::KeyK
                | KeyCode::KeyL
                | KeyCode::KeyM
                | KeyCode::KeyN
                | KeyCode::KeyO
                | KeyCode::KeyP
                | KeyCode::KeyQ
                | KeyCode::KeyR
                | KeyCode::KeyS
                | KeyCode::KeyT
                | KeyCode::KeyU
                | KeyCode::KeyV
                | KeyCode::KeyW
                | KeyCode::KeyX
                | KeyCode::KeyY
                | KeyCode::KeyZ
                | KeyCode::Digit0
                | KeyCode::Digit1
                | KeyCode::Digit2
                | KeyCode::Digit3
                | KeyCode::Digit4
                | KeyCode::Digit5
                | KeyCode::Digit6
                | KeyCode::Digit7
                | KeyCode::Digit8
                | KeyCode::Digit9
                | KeyCode::Space
                | KeyCode::Minus
                | KeyCode::Equal
        )
    );
    if is_typing_key && parts.is_empty() {
        return KeyCapture::None;
    }

    parts.push(key_name);
    KeyCapture::Combo(parts.join("+"))
}

/// quick-switch 슬롯 전용 캡처 — **modifier 가 하나라도 눌려 있으면 무효**다.
///
/// 일반 콤보 캡처([`capture_winit_key_combo`])와 정반대 규칙: 슬롯 키는 dispatch
/// 시점에 `tab_switch_modifier`/`workspace_switch_modifier` 와 조합되므로, 사용자는
/// modifier 없이 **키 하나만** 눌러야 한다. 실수로 `Ctrl+Q` 를 누르면 조용히 `Q` 로
/// 해석하지 않고 무효 처리(대기 유지)해 다시 누르게 한다.
///
/// - `state != Pressed` → `None`
/// - Escape → `Clear`(슬롯 비우기)
/// - modifier-only 키(Ctrl/Shift/Alt/Super …) 단독 → `None`
/// - modifier 가 하나라도 눌린 채의 일반 키 → `None`(무효)
/// - modifier 없는 일반 키 → `Combo(키이름)`(콤보 접두사 없이 raw 키만)
pub fn capture_bare_key(
    event: &winit::event::KeyEvent,
    modifiers: winit::keyboard::ModifiersState,
) -> KeyCapture {
    use winit::event::ElementState;
    use winit::keyboard::{Key, NamedKey};

    if event.state != ElementState::Pressed {
        return KeyCapture::None;
    }

    let is_escape = event.logical_key == Key::Named(NamedKey::Escape);
    let is_modifier_only = matches!(
        &event.logical_key,
        Key::Named(
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
        )
    );
    let key_name =
        physical_key_to_name(&event.physical_key).or_else(|| named_key_to_name(&event.logical_key));

    bare_key_decision(is_escape, is_modifier_only, modifiers, key_name)
}

/// [`capture_bare_key`] 의 순수 판정부 — `winit::event::KeyEvent` 는 외부에서 생성할 수
/// 없어 단위 테스트가 불가능하므로, 이벤트에서 뽑아낸 값만으로 결정을 내리는 부분을
/// 분리해 테스트 가능하게 한다.
fn bare_key_decision(
    is_escape: bool,
    is_modifier_only: bool,
    modifiers: winit::keyboard::ModifiersState,
    key_name: Option<&'static str>,
) -> KeyCapture {
    if is_escape {
        return KeyCapture::Clear;
    }
    if is_modifier_only {
        return KeyCapture::None;
    }
    // modifier 가 하나라도 눌려 있으면 무효 — 순수 키 입력만 유효.
    if modifiers.control_key()
        || modifiers.alt_key()
        || modifiers.shift_key()
        || modifiers.super_key()
    {
        return KeyCapture::None;
    }
    match key_name {
        Some(name) => KeyCapture::Combo(name.to_string()),
        None => KeyCapture::None,
    }
}

fn physical_key_to_name(physical: &winit::keyboard::PhysicalKey) -> Option<&'static str> {
    use winit::keyboard::{KeyCode, PhysicalKey};
    let code = match physical {
        PhysicalKey::Code(c) => c,
        _ => return None,
    };
    Some(match code {
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
        KeyCode::Tab => "tab",
        KeyCode::Space => "space",
        KeyCode::Enter => "enter",
        KeyCode::Backspace => "backspace",
        KeyCode::Delete => "delete",
        KeyCode::Insert => "insert",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::PageUp => "pageup",
        KeyCode::PageDown => "pagedown",
        KeyCode::ArrowUp => "up",
        KeyCode::ArrowDown => "down",
        KeyCode::ArrowLeft => "left",
        KeyCode::ArrowRight => "right",
        KeyCode::F1 => "f1",
        KeyCode::F2 => "f2",
        KeyCode::F3 => "f3",
        KeyCode::F4 => "f4",
        KeyCode::F5 => "f5",
        KeyCode::F6 => "f6",
        KeyCode::F7 => "f7",
        KeyCode::F8 => "f8",
        KeyCode::F9 => "f9",
        KeyCode::F10 => "f10",
        KeyCode::F11 => "f11",
        KeyCode::F12 => "f12",
        KeyCode::Minus => "minus",
        KeyCode::Equal => "=",
        KeyCode::Comma => ",",
        KeyCode::Period => ".",
        KeyCode::Semicolon => ";",
        KeyCode::BracketLeft => "[",
        KeyCode::BracketRight => "]",
        KeyCode::Backslash => "\\",
        KeyCode::Backquote => "`",
        KeyCode::Slash => "/",
        _ => return None,
    })
}

fn named_key_to_name(key: &winit::keyboard::Key) -> Option<&'static str> {
    use winit::keyboard::NamedKey;
    if let winit::keyboard::Key::Named(n) = key {
        Some(match n {
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
            _ => return None,
        })
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyCapture, bare_key_decision};
    use winit::keyboard::ModifiersState;

    #[test]
    fn capture_bare_key_rejects_when_modifier_held() {
        // ctrl 이 눌린 상태에서 'q' 키 입력은 무효(대기 유지) — 순수 키만 유효.
        let result = bare_key_decision(false, false, ModifiersState::CONTROL, Some("q"));
        assert_eq!(result, KeyCapture::None);

        // alt / shift / super 도 동일하게 거부.
        assert_eq!(
            bare_key_decision(false, false, ModifiersState::ALT, Some("q")),
            KeyCapture::None
        );
        assert_eq!(
            bare_key_decision(false, false, ModifiersState::SHIFT, Some("q")),
            KeyCapture::None
        );
        assert_eq!(
            bare_key_decision(false, false, ModifiersState::SUPER, Some("q")),
            KeyCapture::None
        );
    }

    #[test]
    fn capture_bare_key_accepts_plain_key() {
        // modifier 없이 'q' 입력 → raw 키 이름만(콤보 접두사 없음).
        let result = bare_key_decision(false, false, ModifiersState::empty(), Some("q"));
        assert_eq!(result, KeyCapture::Combo("q".to_string()));
    }

    #[test]
    fn capture_bare_key_escape_clears() {
        assert_eq!(
            bare_key_decision(true, false, ModifiersState::empty(), None),
            KeyCapture::Clear
        );
    }

    #[test]
    fn capture_bare_key_modifier_only_waits() {
        // modifier-only 키(Ctrl 단독) 는 무효 — 대기 유지.
        assert_eq!(
            bare_key_decision(false, true, ModifiersState::CONTROL, None),
            KeyCapture::None
        );
    }
}
