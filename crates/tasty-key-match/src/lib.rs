//! 바인딩 문자열을 winit/egui 키 이벤트와 대조한다. 단축키와 webview 키 전달이 같은 규칙을 쓴다.
//! 문자열 파싱은 tasty_settings::keybindings::parse가 맡고, 이 크레이트는 플랫폼별 매칭을 담당한다.

use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

pub use tasty_settings::keybindings::parse::bindings_equivalent;
pub use tasty_settings::keybindings::parse::parse_binding;

pub fn matches_any_binding(bindings: &[String], key: &Key, mods: ModifiersState) -> bool {
    bindings.iter().any(|b| matches_binding(b, key, mods))
}

#[cfg(feature = "egui-input")]
/// egui 입력(`InputState`) 기준으로 바인딩 목록 중 하나라도 이번 프레임에 눌렸는지
/// 판정한다. winit 단축키 경로가 닿지 않는 egui 위젯(검색 바 등) 안에서
/// `KeybindingSettings` 바인딩을 그대로 매칭하기 위한 진입점.
pub fn any_binding_pressed_egui(bindings: &[String], input: &egui::InputState) -> bool {
    bindings.iter().any(|b| binding_pressed_egui(b, input))
}

#[cfg(feature = "egui-input")]
/// 단일 바인딩 문자열이 egui 입력에서 이번 프레임에 눌렸는지 판정.
fn binding_pressed_egui(binding: &str, input: &egui::InputState) -> bool {
    let Some(parsed) = parse_binding(binding) else {
        return false;
    };
    let mods = &input.modifiers;

    // modifier 매핑은 winit 경로(`matches_binding`)와 동일한 플랫폼 규칙을 따른다.
    // macOS: 바인딩 "alt" → Cmd(mac_cmd), "option" → Option(alt). 그 외: "alt" → alt.
    #[cfg(target_os = "macos")]
    let (alt_matches, option_matches) = (mods.mac_cmd == parsed.alt, mods.alt == parsed.option);
    #[cfg(not(target_os = "macos"))]
    let (alt_matches, option_matches) = (mods.alt == parsed.alt, !parsed.option);

    if mods.ctrl != parsed.ctrl || mods.shift != parsed.shift || !alt_matches || !option_matches {
        return false;
    }

    match token_to_egui_key(&parsed.key.to_ascii_lowercase()) {
        Some(key) => input.key_pressed(key),
        None => false,
    }
}

/// 바인딩 키 토큰(소문자)을 egui `Key` 로 변환. named/function 토큰은 명시 매핑하고,
/// 글자·숫자·기호는 egui `Key::from_name` 에 위임한다 (대문자 폴백 포함).
#[cfg(feature = "egui-input")]
fn token_to_egui_key(token: &str) -> Option<egui::Key> {
    use egui::Key;
    Some(match token {
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "escape" => Key::Escape,
        "tab" => Key::Tab,
        "space" => Key::Space,
        "enter" => Key::Enter,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        _ => {
            return Key::from_name(token).or_else(|| Key::from_name(&token.to_ascii_uppercase()));
        }
    })
}

/// ctrl/alt/option 중 하나 이상을 요구하는지 확인한다. shift 단독은 false다.
/// webview가 페이지에 남길 키와 호스트로 전달할 키를 구분할 때 사용한다.
pub fn binding_has_modifier(binding: &str) -> bool {
    match parse_binding(binding) {
        Some(p) => p.ctrl || p.alt || p.option,
        None => false,
    }
}

/// Parse a binding string like "ctrl+shift+n" and check if it matches
/// the given key + modifiers. Returns false for empty bindings.
pub fn matches_binding(binding: &str, key: &Key, mods: ModifiersState) -> bool {
    let Some(parsed) = parse_binding(binding) else {
        return false;
    };

    // modifier 자체를 누른 이벤트는 단축키를 실행하지 않는다.
    if let Key::Named(n) = key
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
        return false;
    }

    // Check modifiers match exactly.
    // On macOS, "alt" in binding maps to Cmd (super_key) since the physical
    // position of Cmd on macOS keyboards matches Alt on Windows/Linux keyboards.
    // "option" maps to the macOS Option key (alt_key in winit).
    #[cfg(target_os = "macos")]
    let alt_matches = mods.super_key() == parsed.alt;
    #[cfg(not(target_os = "macos"))]
    let alt_matches = mods.alt_key() == parsed.alt;

    // option 바인딩은 macOS에서만 매칭한다.
    #[cfg(target_os = "macos")]
    let option_matches = mods.alt_key() == parsed.option;
    #[cfg(not(target_os = "macos"))]
    let option_matches = !parsed.option; // option binding은 non-macOS에서 항상 불일치

    if mods.control_key() != parsed.ctrl
        || mods.shift_key() != parsed.shift
        || !alt_matches
        || !option_matches
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
                if (1..=26).contains(&byte) {
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
    matches!(
        (character, token),
        ("+", "plus")
            | ("plus", "+")
            | ("-", "minus")
            | ("minus", "-")
            | ("=", "equals")
            | ("equals", "=")
    )
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

/// Convert a physical key code to a Key::Character for shortcut matching.
/// On macOS, when IME is composing (e.g. Korean), logical_key may contain
/// the composed character (e.g. "ㅇ" instead of "d"). This function extracts
/// the intended key from the physical key code.
pub fn physical_key_to_logical(physical: &PhysicalKey) -> Option<Key> {
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
