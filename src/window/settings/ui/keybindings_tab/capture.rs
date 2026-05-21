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
    if let Key::Named(n) = &event.logical_key {
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
            return KeyCapture::None;
        }
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
