//! 바인딩 문자열을 macOS NSMenuItem 의 key equivalent 와 수식키로 바꾼다.
//!
//! 수식키는 [키 매핑 정책](../../../docs/design/policies/key-mapping.md)의 macOS 매핑을 따른다:
//! `ctrl` → Control, `shift` → Shift, `option` → Option, `alt` → Command(위치 기반 추상화).
//! 키 이름은 `tasty-key-match` 가 받는 이름 집합과 같고, 이름 키는 AppKit 기능 키 문자
//! (`NSF1FunctionKey` 등)나 제어 문자로 바꾼다. 메뉴로 나타낼 수 없는 키는 빈 문자열이라
//! 단축키 없는 메뉴 항목이 된다.

use tasty_settings::keybindings::parse::parse_binding;

/// NSMenuItem 에 넣을 key equivalent 와 수식키. AppKit 형식으로 옮기는 일은 호출자가 한다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MenuKeyEquivalent {
    /// `setKeyEquivalent` 문자열. 빈 문자열이면 단축키를 표시하지 않는다.
    pub key: String,
    pub control: bool,
    pub shift: bool,
    pub option: bool,
    pub command: bool,
}

/// AppKit `NSF1FunctionKey`. F1~F24 는 이 값부터 차례로 이어진다(`NSF24FunctionKey` = 0xF71B).
const NS_F1_FUNCTION_KEY: u32 = 0xF704;

/// 바인딩 문자열을 메뉴 key equivalent 로 바꾼다. 파싱할 수 없거나 메뉴 문자로 나타낼 수 없는 키면
/// 수식키까지 비운다.
pub fn menu_key_equivalent(binding: &str) -> MenuKeyEquivalent {
    let Some(parsed) = parse_binding(binding) else {
        return MenuKeyEquivalent::default();
    };
    let Some(key) = key_equivalent_for_token(&parsed.key.to_ascii_lowercase()) else {
        return MenuKeyEquivalent::default();
    };
    MenuKeyEquivalent {
        key,
        control: parsed.ctrl,
        shift: parsed.shift,
        option: parsed.option,
        command: parsed.alt,
    }
}

/// 키 토큰 하나를 key equivalent 문자로 바꾼다. 문자 하나는 그대로 둔다(대문자 대신 Shift 수식키로
/// 표현한다).
fn key_equivalent_for_token(token: &str) -> Option<String> {
    if token.chars().count() == 1 {
        return Some(token.to_string());
    }
    let ch = match token {
        "plus" => '+',
        "minus" => '-',
        "equals" => '=',
        "tab" => '\u{09}',
        "space" => ' ',
        "enter" => '\u{0D}',
        // NSMenuItem 문서: 메뉴의 Backspace(⌫)는 NSBackspaceCharacter(0x08), 앞 지우기(⌦)는
        // NSDeleteCharacter(0x7F)로 지정한다. 키 이벤트의 NSDeleteFunctionKey 가 아니다.
        "backspace" => '\u{08}',
        "delete" => '\u{7F}',
        "insert" => '\u{F727}',
        "home" => '\u{F729}',
        "end" => '\u{F72B}',
        "pageup" => '\u{F72C}',
        "pagedown" => '\u{F72D}',
        "up" => '\u{F700}',
        "down" => '\u{F701}',
        "left" => '\u{F702}',
        "right" => '\u{F703}',
        "escape" => '\u{1B}',
        _ => return function_key(token).map(String::from),
    };
    Some(ch.to_string())
}

/// `f1`~`f24` 를 AppKit 기능 키 문자로 바꾼다.
fn function_key(token: &str) -> Option<char> {
    let n: u32 = token.strip_prefix('f')?.parse().ok()?;
    if !(1..=24).contains(&n) || token != format!("f{n}") {
        return None;
    }
    char::from_u32(NS_F1_FUNCTION_KEY + n - 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(binding: &str) -> String {
        menu_key_equivalent(binding).key
    }

    #[test]
    fn modifiers_follow_the_macos_position_mapping() {
        assert_eq!(
            menu_key_equivalent("alt+shift+n"),
            MenuKeyEquivalent {
                key: "n".into(),
                shift: true,
                command: true,
                ..Default::default()
            }
        );
        assert_eq!(
            menu_key_equivalent("Ctrl+Option+Q"),
            MenuKeyEquivalent {
                key: "q".into(),
                control: true,
                option: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn function_keys_become_appkit_function_key_characters() {
        assert_eq!(key("f1"), "\u{F704}"); // NSF1FunctionKey
        assert_eq!(key("alt+f12"), "\u{F70F}"); // NSF12FunctionKey
        assert_eq!(key("f13"), "\u{F710}"); // NSF13FunctionKey
        assert_eq!(key("ctrl+F21"), "\u{F718}"); // NSF21FunctionKey
        assert_eq!(key("f24"), "\u{F71B}"); // NSF24FunctionKey
        for n in 1..=24u32 {
            assert_eq!(
                key(&format!("f{n}")).chars().collect::<Vec<_>>(),
                vec![char::from_u32(0xF704 + n - 1).unwrap()],
                "f{n}"
            );
        }
    }

    #[test]
    fn named_keys_become_appkit_characters() {
        let cases = [
            ("alt+tab", "\u{09}"),
            ("alt+space", " "),
            ("alt+enter", "\u{0D}"),
            ("alt+backspace", "\u{08}"),
            ("alt+delete", "\u{7F}"),
            ("alt+insert", "\u{F727}"),
            ("alt+home", "\u{F729}"),
            ("alt+end", "\u{F72B}"),
            ("alt+pageup", "\u{F72C}"),
            ("alt+pagedown", "\u{F72D}"),
            ("alt+up", "\u{F700}"),
            ("alt+down", "\u{F701}"),
            ("alt+left", "\u{F702}"),
            ("alt+right", "\u{F703}"),
            ("escape", "\u{1B}"),
            ("alt+plus", "+"),
            ("alt++", "+"),
            ("alt+minus", "-"),
            ("alt+equals", "="),
        ];
        for (binding, expected) in cases {
            assert_eq!(key(binding), expected, "{binding}");
        }
    }

    /// 매칭 규칙이 받는 이름 키는 모두 메뉴 문자가 있고, 문자 하나가 아니다(이름이 그대로 들어가지 않는다).
    #[test]
    fn every_key_match_name_has_a_menu_character() {
        let mut n = 0;
        for name in tasty_key_match::named_key_tokens() {
            let k = key(name);
            assert_eq!(k.chars().count(), 1, "{name} → {k:?}");
            assert_ne!(k, name, "{name}");
            n += 1;
        }
        assert!(n >= 39, "이름 키 {n}개만 돌았다");
    }

    /// 매칭할 수 없는 키·빈 값·수식키만 있는 값은 단축키를 비운다(수식키도 남기지 않는다).
    #[test]
    fn unrepresentable_bindings_leave_the_item_without_a_shortcut() {
        for binding in [
            "",
            "alt+",
            "ctrl",
            "alt+f25",
            "alt+f0",
            "alt+f013",
            "ctrl+shft+h",
            "alt+hello",
        ] {
            assert_eq!(
                menu_key_equivalent(binding),
                MenuKeyEquivalent::default(),
                "{binding}"
            );
        }
    }
}
