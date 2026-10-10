//! 텍스트로 넣은 바인딩(설정 파일·단축키 번들·plugin override)에 붙이는 OS 안내.
//!
//! - OS 가 먼저 받는 조합: Tasty 에 오지 않을 수 있는 조합의 짧은 목록이다. 데스크톱마다 달라
//!   완전한 목록이 될 수 없으므로 경고 문구는 "그럴 수 있다" 로만 말하고 저장을 막지 않는다.
//!   녹화한 바인딩은 OS 가 가로챈 키가 녹화기에 오지 않으므로 이 검사가 필요 없다.
//! - OS 키 이름: `cmd`·`super`·`win`·`meta` 는 저장 토큰이 아니다. `super` 가 macOS 에서는 Command
//!   위치라 OS 마다 뜻이 달라지므로 정규화하지 않고, 어떻게 적는지 안내한다.
//!
//! 표시 위치와 문구는 docs/design/policies/key-mapping.md 를 따른다.

use super::parse::parse_binding;

/// 예약 목록을 고를 OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReservedOs {
    Windows,
    /// Linux 데스크톱 전체(GNOME·KDE 등). macOS 는 목록이 없다.
    Linux,
}

impl ReservedOs {
    /// 이 빌드의 OS. macOS 는 `None` — option 이 Option 키라 OS 가 가로채는 조합 목록을 두지 않는다.
    pub fn current() -> Option<Self> {
        if cfg!(target_os = "windows") {
            Some(Self::Windows)
        } else if cfg!(target_os = "macos") {
            None
        } else {
            Some(Self::Linux)
        }
    }
}

/// Windows 에서 Win 과 함께 누르면 OS 가 먼저 받는 키(Win 하나만 더한 조합).
const WINDOWS_WIN_KEYS: [&str; 6] = ["l", "d", "e", "r", "i", "tab"];

/// `binding` 이 `os` 가 먼저 받을 수 있는 조합이면 true.
/// option 단독(Win·Super 만 누름)과, Windows 의 Win + [`WINDOWS_WIN_KEYS`] 하나가 해당한다.
pub fn is_os_reserved(binding: &str, os: ReservedOs) -> bool {
    let binding = binding.trim();
    if binding.eq_ignore_ascii_case("option") {
        return true;
    }
    match os {
        ReservedOs::Linux => false,
        ReservedOs::Windows => parse_binding(binding).is_some_and(|p| {
            p.option
                && !p.ctrl
                && !p.alt
                && !p.shift
                && WINDOWS_WIN_KEYS
                    .iter()
                    .any(|k| p.key.eq_ignore_ascii_case(k))
        }),
    }
}

/// 저장 토큰 대신 적힌 OS 키 이름.
const OS_KEY_NAMES: [&str; 4] = ["cmd", "super", "win", "meta"];

/// `binding` 의 `+` 로 나눈 조각 중 OS 키 이름(`cmd`·`super`·`win`·`meta`)이 있으면 true.
pub fn uses_os_key_name(binding: &str) -> bool {
    binding.split('+').any(|part| {
        let part = part.trim();
        OS_KEY_NAMES.iter().any(|n| part.eq_ignore_ascii_case(n))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn win_with_one_listed_key_is_reserved_on_windows_only() {
        for b in [
            "option+l",
            "Option+D",
            "option+e",
            "option+r",
            "option+i",
            "option+tab",
        ] {
            assert!(is_os_reserved(b, ReservedOs::Windows), "{b}");
            assert!(!is_os_reserved(b, ReservedOs::Linux), "{b}");
        }
    }

    #[test]
    fn another_modifier_or_another_key_is_not_on_the_list() {
        for b in [
            "ctrl+option+l",
            "option+shift+d",
            "option+k",
            "ctrl+l",
            "alt+tab",
        ] {
            assert!(!is_os_reserved(b, ReservedOs::Windows), "{b}");
        }
    }

    #[test]
    fn option_alone_is_reserved_on_both() {
        assert!(is_os_reserved("option", ReservedOs::Windows));
        assert!(is_os_reserved(" Option ", ReservedOs::Linux));
    }

    #[test]
    fn os_key_names_are_found_in_any_part() {
        for b in ["cmd+k", "Super+K", "ctrl+win+k", "meta+shift+1", "ctrl+cmd"] {
            assert!(uses_os_key_name(b), "{b}");
        }
        for b in ["option+k", "ctrl+alt+k", "ctrl+m", "ctrl+plus", "ctrl++"] {
            assert!(!uses_os_key_name(b), "{b}");
        }
    }
}
