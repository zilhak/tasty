//! 바인딩 문자열 파싱/매칭 — `"ctrl+shift+n"` 같은 문자열을 `(key, mods)` 와 매칭.

use winit::keyboard::{Key, ModifiersState, NamedKey};

pub(crate) fn matches_any_binding(bindings: &[String], key: &Key, mods: ModifiersState) -> bool {
    bindings.iter().any(|b| matches_binding(b, key, mods))
}

/// egui 입력(`InputState`) 기준으로 바인딩 목록 중 하나라도 이번 프레임에 눌렸는지
/// 판정한다. winit 단축키 경로가 닿지 않는 egui 위젯(검색 바 등) 안에서
/// `KeybindingSettings` 바인딩을 그대로 매칭하기 위한 진입점.
pub(crate) fn any_binding_pressed_egui(bindings: &[String], input: &egui::InputState) -> bool {
    bindings.iter().any(|b| binding_pressed_egui(b, input))
}

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

/// Parsed binding: expected modifier state + the literal key token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ParsedBinding<'a> {
    pub(super) ctrl: bool,
    pub(super) shift: bool,
    pub(super) alt: bool,
    /// macOS 전용: Option 키. Windows/Linux에서는 항상 false.
    pub(super) option: bool,
    /// 키 토큰 (문자 "+", "-", "a" 또는 네임 "plus", "f1", "tab" 등). 공백/모디파이어 키워드는 거부되어 여기 오지 않는다.
    pub(super) key: &'a str,
}

/// 왼쪽부터 `ctrl+`/`shift+`/`alt+` 프리픽스를 순차적으로 떼어낸다.
///
/// `split('+')`을 쓰지 않는 이유: `"ctrl++"`의 두 번째 `+`처럼 키 이름과 구분자가
/// 충돌하는 경우를 다루기 위함. 프리픽스를 하나씩 벗겨내면 남은 부분이 통째로 키가
/// 되므로 구분자 충돌 문제가 사라진다.
pub(super) fn parse_binding(binding: &str) -> Option<ParsedBinding<'_>> {
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
    let mut option = false;
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
        } else if !option && lower.starts_with("option+") {
            option = true;
            rest = &rest[7..];
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
    if matches!(rest_lower.as_str(), "ctrl" | "shift" | "alt" | "option") {
        return None;
    }

    Some(ParsedBinding {
        ctrl,
        shift,
        alt,
        option,
        key: rest,
    })
}

/// Parse a binding string like "ctrl+shift+n" and check if it matches
/// the given key + modifiers. Returns false for empty bindings.
pub(super) fn matches_binding(binding: &str, key: &Key, mods: ModifiersState) -> bool {
    let Some(parsed) = parse_binding(binding) else {
        return false;
    };

    // Modifier-only key presses must never trigger any shortcut, regardless of
    // how the binding is spelled. This is the structural guard that prevents
    // "Ctrl alone" from ever matching.
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

    // macOS: "option" modifier maps to Option key (alt_key in winit).
    // On non-macOS, "option" bindings never match (option is always false).
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

/// Check if a binding string represents a double-tap modifier (e.g. "shift+shift").
fn is_double_tap_binding(binding: &str) -> Option<crate::double_tap::DoubleTapKey> {
    match binding.to_lowercase().as_str() {
        "shift+shift" => Some(crate::double_tap::DoubleTapKey::Shift),
        "ctrl+ctrl" => Some(crate::double_tap::DoubleTapKey::Ctrl),
        "alt+alt" => Some(crate::double_tap::DoubleTapKey::Alt),
        _ => None,
    }
}
