//! Commands 절 키캡. 매니페스트 조합을 플랫폼과 사용자 수식키 표시 스타일에 따라 키캡 조각으로 바꾼다.
//!
//! Windows·Linux 는 매니페스트 순서대로 낱말 키캡이다. macOS 는 키바인딩 매핑(`alt` → Command,
//! `option` → Option, `ctrl` → Control)을 거친 뒤 Apple 순서(⌃ ⌥ ⇧ ⌘)로 수식키를 놓고, 각 수식키는
//! 설정 › 일반의 표시 스타일을 따른다. 설정 키바인딩 행과 수식키 안내가 같은 규칙이다. Ctrl 은 표시
//! 스타일과 글리프가 없어 늘 낱말 `Ctrl` 이다.

use crate::chip::{KbdKey, split_keys};

/// `alt` 토큰의 표기. 설정 값 `"alt"` · `"cmd"` · `"symbol"` 에 대응한다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PluginKeycapAltStyle {
    #[default]
    Alt,
    Cmd,
    Symbol,
}

/// `option`·`shift` 토큰의 표기. 설정 값 `"symbol"` 이면 기호, 그 밖은 낱말이다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PluginKeycapWordStyle {
    #[default]
    Word,
    Symbol,
}

/// 키캡을 그릴 플랫폼과 수식키 표기. 기본값은 비-macOS 낱말 키캡이다.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PluginKeycapStyle {
    pub macos: bool,
    pub alt: PluginKeycapAltStyle,
    pub option: PluginKeycapWordStyle,
    pub shift: PluginKeycapWordStyle,
}

impl PluginKeycapStyle {
    /// 설정 문자열(`GeneralSettings::{alt,option,shift}_display_style`)로 만든다. 모르는 값은
    /// `KeybindingSettings::format_display_parts` 처럼 낱말로 본다.
    pub fn from_setting_names(macos: bool, alt: &str, option: &str, shift: &str) -> Self {
        let word = |name: &str| match name {
            "symbol" => PluginKeycapWordStyle::Symbol,
            _ => PluginKeycapWordStyle::Word,
        };
        Self {
            macos,
            alt: match alt {
                "cmd" => PluginKeycapAltStyle::Cmd,
                "symbol" => PluginKeycapAltStyle::Symbol,
                _ => PluginKeycapAltStyle::Alt,
            },
            option: word(option),
            shift: word(shift),
        }
    }
}

/// 키캡 하나. 낱말 또는 수식키 기호 아이콘이다. 기호는 폰트에 없을 수 있어 수식키 안내처럼 아이콘으로
/// 그린다.
#[derive(Debug, Clone)]
pub enum PluginKeycap {
    Text(String),
    Icon(tasty_icons::Icon),
}

/// 아이콘은 캐시 키(`uri`)가 같으면 같은 글리프다.
impl PartialEq for PluginKeycap {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a == b,
            (Self::Icon(a), Self::Icon(b)) => a.uri == b.uri,
            _ => false,
        }
    }
}

impl PluginKeycap {
    /// [`crate::kbd_parts`] 가 받는 조각으로 빌린다.
    pub fn as_kbd_key(&self) -> KbdKey<'_> {
        match self {
            Self::Text(text) => KbdKey::Text(text),
            Self::Icon(icon) => KbdKey::Icon(*icon),
        }
    }
}

/// 매니페스트 조합을 키캡 조각으로 바꾼다. 빈 조각은 버린다.
pub fn plugin_keycap_parts(chord: &str, style: &PluginKeycapStyle) -> Vec<PluginKeycap> {
    let keys: Vec<&str> = split_keys(chord)
        .into_iter()
        .map(str::trim)
        .filter(|k| !k.is_empty())
        .collect();
    if !style.macos {
        return keys
            .into_iter()
            .map(|k| PluginKeycap::Text(title_case(k)))
            .collect();
    }
    let (mut ctrl, mut option, mut shift, mut alt) = (false, false, false, false);
    let mut rest = Vec::new();
    for key in keys {
        match key.to_ascii_lowercase().as_str() {
            "ctrl" => ctrl = true,
            "option" => option = true,
            "shift" => shift = true,
            "alt" => alt = true,
            _ => rest.push(PluginKeycap::Text(title_case(key))),
        }
    }
    let word = |style: PluginKeycapWordStyle, text: &str, icon: tasty_icons::Icon| match style {
        PluginKeycapWordStyle::Word => PluginKeycap::Text(text.to_owned()),
        PluginKeycapWordStyle::Symbol => PluginKeycap::Icon(icon),
    };
    let mut out = Vec::with_capacity(rest.len() + 4);
    if ctrl {
        out.push(PluginKeycap::Text("Ctrl".to_owned()));
    }
    if option {
        out.push(word(style.option, "Option", tasty_icons::OPTION_KEY));
    }
    if shift {
        out.push(word(style.shift, "Shift", tasty_icons::SHIFT_KEY));
    }
    if alt {
        out.push(match style.alt {
            PluginKeycapAltStyle::Alt => PluginKeycap::Text("Alt".to_owned()),
            PluginKeycapAltStyle::Cmd => PluginKeycap::Text("Cmd".to_owned()),
            PluginKeycapAltStyle::Symbol => PluginKeycap::Icon(tasty_icons::CMD_KEY),
        });
    }
    out.extend(rest);
    out
}

/// 한 글자 키는 대문자, 나머지는 첫 글자만 대문자로 쓴다(`h` → `H`, `shift` → `Shift`).
pub(super) fn title_case(key: &str) -> String {
    let mut chars = key.chars();
    match chars.next() {
        Some(c) if chars.as_str().is_empty() => c.to_uppercase().collect(),
        Some(c) => c
            .to_uppercase()
            .chain(chars.as_str().to_lowercase().chars())
            .collect(),
        None => String::new(),
    }
}
