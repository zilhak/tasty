//! explorer 타입어헤드 — 영숫자를 치면 그 글자로 시작하는 항목을 고른다.
//!
//! 파일 탐색기의 관례 동작이다. `c` 를 누르면 `c` 로 시작하는 첫 항목이 선택되고 그
//! 항목이 보이도록 스크롤되며, 같은 글자를 이어 누르면 다음 후보로 순환한다. 서로 다른
//! 글자를 이어 치면 접두사 검색이 된다.
//!
//! **이 모듈은 egui 도 파일시스템도 모른다.** 입력은 이름 목록과 현재 선택 인덱스뿐이라
//! GUI 없이 유닛 테스트가 돈다 — 규칙(순환 시작점 · 접두사 · 되돌리기)이 실제로 틀리는
//! 자리가 전부 여기 있고, 그리는 쪽은 그 결과를 선택과 스크롤로 옮기기만 한다.
//!
//! **단축키가 아니다.** 키 조합 → 액션 바인딩이 아니라 문자 입력 스트림 소비라
//! `KeybindingSettings` 에 필드를 만들지 않는다. 대신 사용자가 수식 없는 영숫자 바인딩을
//! 직접 만들었을 때 둘이 함께 발화하는 것을 [`unmodified_binding_chars`] 가 막는다.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use tasty_settings::keybindings::KeybindingSettings;

/// 마지막 입력으로부터 이만큼 지나면 버퍼를 비운다. 그 뒤의 같은 글자는 접두사를 잇는
/// 것이 아니라 새 검색으로 읽는다.
pub const TYPE_AHEAD_TIMEOUT: Duration = Duration::from_millis(1000);

/// 타입어헤드 입력 상태. 버퍼는 **소문자 ASCII 영숫자만** 담는다.
#[derive(Default)]
pub struct TypeAhead {
    buffer: String,
    last_input: Option<Instant>,
}

impl TypeAhead {
    /// 글자 하나를 먹이고, 골라야 할 항목의 인덱스를 돌려준다.
    ///
    /// 매칭이 없으면 `None` 을 돌려주고 **버퍼를 이 입력 직전 상태로 되돌린다** — 오타
    /// 한 글자가 그 뒤 입력을 전부 막지 않게 하는 것이다(되돌리지 않으면 `cz` 가 버퍼에
    /// 남아 이어지는 `o` 가 `czo` 를 찾는다).
    pub fn feed(
        &mut self,
        ch: char,
        now: Instant,
        names: &[String],
        selected: Option<usize>,
    ) -> Option<usize> {
        if self
            .last_input
            .is_none_or(|t| now.duration_since(t) >= TYPE_AHEAD_TIMEOUT)
        {
            self.buffer.clear();
        }
        // 타임아웃 정리 **뒤**의 상태를 되돌림 지점으로 잡는다. 정리 전으로 되돌리면
        // 이미 만료된 버퍼가 되살아난다.
        let before = self.buffer.clone();
        self.buffer.push(ch.to_ascii_lowercase());
        self.last_input = Some(now);

        let (query, start) = query_and_start(&self.buffer, selected);
        match find_match(names, &query, start) {
            Some(i) => Some(i),
            None => {
                self.buffer = before;
                None
            }
        }
    }

    /// 버퍼를 비운다. 디렉토리·정렬이 바뀌거나 내부 탭이 바뀌면 이전 입력이 새 목록에
    /// 이어지면 안 된다.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.last_input = None;
    }
}

/// 버퍼에서 (검색어, 검색 시작 인덱스)를 정한다.
///
/// 같은 글자 연타는 **순환**이고 서로 다른 글자는 **접두사 확장**이다. 둘을 가르지
/// 않으면 `cc` 가 `cc` 로 시작하는 이름을 찾게 되어 연타가 순환이 아니라 매칭 실패가 된다.
/// 접두사 확장은 지금 선택된 항목부터 보기 때문에, 이미 새 접두사에 맞는 항목이 선택돼
/// 있으면 제자리에 머문다.
fn query_and_start(buffer: &str, selected: Option<usize>) -> (String, usize) {
    let mut chars = buffer.chars();
    let Some(first) = chars.next() else {
        return (String::new(), 0);
    };
    let repeated_single = chars.clone().count() == 0 || buffer.chars().all(|c| c == first);
    if repeated_single {
        (first.to_string(), selected.map_or(0, |i| i + 1))
    } else {
        (buffer.to_string(), selected.unwrap_or(0))
    }
}

/// `start` 부터 끝까지 본 뒤 앞으로 돌아와 한 바퀴 돌며 첫 매칭 인덱스를 찾는다.
/// 비교는 대소문자를 무시한다.
fn find_match(names: &[String], query: &str, start: usize) -> Option<usize> {
    if names.is_empty() || query.is_empty() {
        return None;
    }
    let n = names.len();
    (0..n)
        .map(|k| (start + k) % n)
        .find(|&i| names[i].to_lowercase().starts_with(query))
}

/// 타입어헤드가 소비해도 되는 문자인가 — **단일 ASCII 영숫자**만이다.
///
/// `Event::Paste` 는 애초에 다른 이벤트라 여기 오지 않고, Ctrl/Cmd 조합은 egui-winit 이
/// `Event::Text` 를 만들지 않는다. macOS Option 조합이 만드는 `å` 같은 비-ASCII 는 이
/// 필터가 거른다.
pub fn type_ahead_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c),
        _ => None,
    }
}

/// **수식 없이** 바인딩된 영숫자 글자들. 이 글자는 타입어헤드가 소비하지 않는다.
///
/// 한 키 입력이 둘 다 발화하기 때문이다 — `handle_event` 는 먼저 `Event::Text` 를 egui
/// 큐에 넣고 그 다음 단축키를 소비하는데, 단축키가 소비돼도 **이미 큐에 들어간
/// `Event::Text` 를 빼는 경로가 없다.** 기본 프리셋에는 수식 없는 영숫자 바인딩이 하나도
/// 없지만 사용자가 직접 만들 수 있으므로, 그때는 단축키 쪽에 양보한다.
///
/// `shift` 만 붙은 바인딩도 막는다. shift 조합은 `Event::Text` 를 만들기 때문에
/// (대문자) 수식 없는 것과 똑같이 이중 발화한다 — `binding_has_modifier` 가 shift 를
/// 수식으로 세지 않는 것이 여기서는 맞는 판정이다.
pub fn unmodified_binding_chars(keybindings: &KeybindingSettings) -> HashSet<char> {
    let mut chars = HashSet::new();
    for (field_id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        let Some(bindings) = keybindings.get_bindings(field_id) else {
            continue;
        };
        for binding in bindings {
            if tasty_key_match::binding_has_modifier(binding) {
                continue;
            }
            let Some(parsed) = tasty_key_match::parse_binding(binding) else {
                continue;
            };
            let mut key = parsed.key.chars();
            if let (Some(c), None) = (key.next(), key.next())
                && c.is_ascii_alphanumeric()
            {
                chars.insert(c.to_ascii_lowercase());
            }
        }
    }
    chars
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 표시 순서를 그대로 흉내 낸다 — 디렉토리 우선 정렬 결과다.
    fn names() -> Vec<String> {
        [
            "docs",
            "src",
            "Cargo.toml",
            "cache.rs",
            "config.rs",
            "main.rs",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// 선택이 없으면 목록 처음부터 찾고, 대소문자를 무시한다.
    #[test]
    fn a_first_keystroke_finds_the_first_match_ignoring_case() {
        let mut ta = TypeAhead::default();
        assert_eq!(ta.feed('c', Instant::now(), &names(), None), Some(2));
    }

    /// 같은 글자 연타는 후보를 순환한다 — 마지막 후보 다음은 처음으로 돌아온다.
    #[test]
    fn repeating_the_same_letter_cycles_through_the_candidates() {
        let mut ta = TypeAhead::default();
        let now = Instant::now();
        assert_eq!(ta.feed('c', now, &names(), None), Some(2));
        assert_eq!(ta.feed('c', now, &names(), Some(2)), Some(3));
        assert_eq!(ta.feed('c', now, &names(), Some(3)), Some(4));
        assert_eq!(ta.feed('c', now, &names(), Some(4)), Some(2));
    }

    /// 서로 다른 글자를 이으면 접두사 검색이다 — 연타 순환과 다르게 동작한다.
    #[test]
    fn different_letters_extend_the_prefix_instead_of_cycling() {
        let mut ta = TypeAhead::default();
        let now = Instant::now();
        assert_eq!(ta.feed('c', now, &names(), Some(2)), Some(3));
        assert_eq!(ta.feed('o', now, &names(), Some(3)), Some(4));
    }

    /// 매칭이 없으면 선택도 버퍼도 그대로다 — 다음 글자가 오타에 묶이지 않는다.
    #[test]
    fn a_letter_with_no_match_leaves_the_buffer_as_it_was() {
        let mut ta = TypeAhead::default();
        let now = Instant::now();
        assert_eq!(ta.feed('z', now, &names(), None), None);
        // 되돌리지 않았다면 이 입력은 `zc` 를 찾아 실패한다.
        assert_eq!(ta.feed('c', now, &names(), None), Some(2));
    }

    /// 타임아웃이 지나면 버퍼가 비워져 한 글자 검색으로 돌아간다.
    #[test]
    fn the_buffer_expires_after_the_timeout() {
        let mut ta = TypeAhead::default();
        let now = Instant::now();
        assert_eq!(ta.feed('c', now, &names(), None), Some(2));
        // 버퍼가 살아 있었다면 `co` 접두사 검색이 됐을 자리다.
        let later = now + TYPE_AHEAD_TIMEOUT;
        assert_eq!(ta.feed('o', later, &names(), Some(2)), None);
    }

    /// 소비 대상은 단일 ASCII 영숫자뿐이다.
    #[test]
    fn only_single_ascii_alphanumerics_are_consumed() {
        assert_eq!(type_ahead_char("c"), Some('c'));
        assert_eq!(type_ahead_char("7"), Some('7'));
        assert_eq!(type_ahead_char("C"), Some('C'));
        assert_eq!(type_ahead_char("."), None);
        assert_eq!(type_ahead_char("/"), None);
        assert_eq!(type_ahead_char("한"), None);
        assert_eq!(type_ahead_char("ab"), None);
        assert_eq!(type_ahead_char(""), None);
    }

    /// 기본 프리셋에는 수식 없는 영숫자 바인딩이 없다 — 어떤 글자도 막히지 않는다.
    #[test]
    fn the_default_preset_blocks_no_letter() {
        assert!(unmodified_binding_chars(&KeybindingSettings::default()).is_empty());
    }

    /// 사용자가 수식 없이 글자를 바인딩하면 그 글자는 단축키 쪽에 양보한다.
    #[test]
    fn a_letter_bound_without_a_modifier_is_left_to_the_shortcut() {
        let kb = KeybindingSettings {
            explorer_refresh: vec!["r".to_string()],
            ..Default::default()
        };
        assert!(unmodified_binding_chars(&kb).contains(&'r'));
    }

    /// 수식이 붙은 바인딩은 `Event::Text` 를 만들지 않으므로 충돌이 아니다.
    #[test]
    fn a_letter_bound_with_a_modifier_stays_available() {
        let kb = KeybindingSettings {
            explorer_refresh: vec!["ctrl+r".to_string()],
            ..Default::default()
        };
        assert!(!unmodified_binding_chars(&kb).contains(&'r'));
    }
}
