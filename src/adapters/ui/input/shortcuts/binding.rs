//! 바인딩 **매칭** — `"ctrl+shift+n"` 같은 문자열을 winit/egui 의 `(key, mods)` 와 대조한다.
//!
//! 문자열을 축과 키 토큰으로 쪼개는 **파싱**은 그 문자열을 소유한 크레이트에 있다
//! (`tasty_settings::keybindings::parse`) — 단축키 이식 번들의 `option` 판정이 같은
//! 규칙을 써야 하는데 이 모듈은 `gui` feature 뒤라 그쪽에서 안 보이기 때문이다
//! (`docs/adr/0256-the-binding-parser-lives-with-the-setting-it-parses.md`).
//! 여기 남은 것은 파싱 결과를 실제 키 이벤트와 맞추는 플랫폼 규칙이다.

use winit::keyboard::{Key, ModifiersState, NamedKey};

pub(crate) use tasty_settings::keybindings::parse::bindings_equivalent;
pub(super) use tasty_settings::keybindings::parse::parse_binding;

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

/// 바인딩 문자열이 `ctrl`/`alt`/`option` 중 하나 이상을 요구하는지 판정한다.
/// `shift` 단독과 수식 없는 키는 `false`.
///
/// webview 키 포워딩 정책(`host_api/webview/keys.rs`)이 "페이지에 남길 키" 와
/// "host 가 가져갈 키" 를 가르는 기준으로 쓴다 — 파싱 규칙을 그쪽에 복제하지
/// 않도록 여기서 한 번만 판정한다.
pub(crate) fn binding_has_modifier(binding: &str) -> bool {
    match parse_binding(binding) {
        Some(p) => p.ctrl || p.alt || p.option,
        None => false,
    }
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
