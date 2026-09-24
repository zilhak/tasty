//! 탐색기에서 영숫자를 입력하면 그 글자로 시작하는 항목을 선택한다.
//!
//! 파일 탐색기에서 흔히 쓰는 동작이다. `c`를 누르면 `c`로 시작하는 첫 항목이 선택되고
//! 그 항목이 보이도록 스크롤한다. 같은 글자를 이어서 누르면 다음 후보로 넘어가고, 다른
//! 글자를 이어서 입력하면 그 글자를 합친 접두사로 찾는다.
//!
//! 이 모듈은 egui와 파일시스템에 의존하지 않는다. 입력이 이름 목록과 현재 선택 인덱스뿐
//! 이라 GUI 없이 테스트할 수 있다. 순환 시작 위치, 접두사 확장, 입력 되돌리기처럼 실제로
//! 틀리기 쉬운 규칙이 모두 이 안에 있고, 화면을 그리는 쪽은 결과를 선택과 스크롤로 옮기는
//! 일만 한다.
//!
//! 이 동작은 단축키가 아니다. 키 조합에 액션을 연결하는 것이 아니라 문자 입력을 그대로
//! 소비하므로 `KeybindingSettings`에 항목을 두지 않는다. 다만 사용자가 수식 키 없는
//! 영숫자 단축키를 직접 만들면 한 번의 입력으로 둘이 함께 동작하는데, 그 경우는
//! [`unmodified_binding_chars`]가 걸러낸다.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use tasty_settings::keybindings::KeybindingSettings;

/// 마지막 입력 후 이 시간이 지나면 버퍼를 비운다. 그 뒤에 같은 글자를 누르면 접두사를
/// 잇는 것이 아니라 새로운 검색으로 처리한다.
pub const TYPE_AHEAD_TIMEOUT: Duration = Duration::from_millis(1000);

/// 타입어헤드 입력 상태. 버퍼에는 소문자 ASCII 영숫자만 담는다.
#[derive(Default)]
pub struct TypeAhead {
    buffer: String,
    last_input: Option<Instant>,
}

impl TypeAhead {
    /// 글자 하나를 받아 선택해야 할 항목의 인덱스를 돌려준다.
    ///
    /// 일치하는 항목이 없으면 `None`을 돌려주고 버퍼를 입력 직전 상태로 되돌린다. 오타
    /// 한 글자가 그 뒤의 입력을 계속 막지 않게 하기 위해서다. 되돌리지 않으면 `cz`가
    /// 버퍼에 남아 다음에 누른 `o`가 `czo`를 찾게 된다.
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
        // 만료 처리를 끝낸 뒤의 상태를 되돌림 지점으로 잡는다. 그 전 상태로 되돌리면
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

    /// 버퍼를 비운다. 폴더나 정렬이 바뀌거나 내부 탭을 옮기면 이전 입력이 새 목록으로
    /// 이어지면 안 되기 때문이다.
    pub fn reset(&mut self) {
        self.buffer.clear();
        self.last_input = None;
    }
}

/// 버퍼에서 검색어와 검색을 시작할 인덱스를 정한다.
///
/// 같은 글자를 반복해서 누르면 후보를 순환하고, 다른 글자를 이으면 접두사를 확장한다.
/// 둘을 구분하지 않으면 `cc`가 `cc`로 시작하는 이름을 찾게 되어 반복 입력이 순환이 아니라
/// 검색 실패가 된다. 접두사 확장은 현재 선택된 항목부터 살펴보므로, 이미 새 접두사에
/// 맞는 항목이 선택돼 있으면 선택이 그대로 유지된다.
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

/// `start`부터 목록 끝까지 살펴본 뒤 처음으로 돌아와 한 바퀴 돌면서 처음 일치하는
/// 인덱스를 찾는다. 비교할 때 대소문자는 구분하지 않는다.
fn find_match(names: &[String], query: &str, start: usize) -> Option<usize> {
    if names.is_empty() || query.is_empty() {
        return None;
    }
    let n = names.len();
    (0..n)
        .map(|k| (start + k) % n)
        .find(|&i| names[i].to_lowercase().starts_with(query))
}

/// 타입어헤드가 소비해도 되는 문자인지 판단한다. ASCII 영숫자 한 글자만 허용한다.
///
/// 붙여넣기는 다른 이벤트라 여기로 오지 않고, Ctrl·Cmd 조합은 egui-winit이 텍스트
/// 이벤트를 만들지 않는다. macOS에서 Option 조합으로 입력되는 `å` 같은 문자는 이
/// 조건에서 걸러진다.
pub fn type_ahead_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() => Some(c),
        _ => None,
    }
}

/// 수식 키 없이 단축키로 등록된 영숫자를 모은다. 이 글자들은 타입어헤드가 소비하지
/// 않는다.
///
/// 한 번의 키 입력으로 둘이 함께 동작하기 때문이다. `handle_event`는 텍스트 이벤트를
/// 먼저 egui 큐에 넣고 그 다음에 단축키를 처리하는데, 단축키가 처리되더라도 이미 큐에
/// 들어간 텍스트 이벤트를 되돌릴 방법이 없다. 기본 프리셋에는 수식 키 없는 영숫자
/// 단축키가 없지만 사용자가 직접 만들 수 있으므로, 그럴 때는 단축키를 우선한다.
///
/// `shift`만 붙은 단축키도 같이 제외한다. shift 조합은 대문자 텍스트 이벤트를 만들어
/// 수식 키가 없을 때와 똑같이 두 번 동작한다. `binding_has_modifier`가 shift를 수식 키로
/// 세지 않는 것이 여기서는 오히려 맞는 판단이다.
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

    /// 화면에 보이는 순서를 그대로 흉내 낸다. 폴더를 먼저 정렬한 결과다.
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

    /// 선택된 항목이 없으면 목록 처음부터 찾고, 대소문자는 구분하지 않는다.
    #[test]
    fn a_first_keystroke_finds_the_first_match_ignoring_case() {
        let mut ta = TypeAhead::default();
        assert_eq!(ta.feed('c', Instant::now(), &names(), None), Some(2));
    }

    /// 같은 글자를 반복해서 누르면 후보를 순환하고, 마지막 후보 다음에는 처음으로
    /// 돌아온다.
    #[test]
    fn repeating_the_same_letter_cycles_through_the_candidates() {
        let mut ta = TypeAhead::default();
        let now = Instant::now();
        assert_eq!(ta.feed('c', now, &names(), None), Some(2));
        assert_eq!(ta.feed('c', now, &names(), Some(2)), Some(3));
        assert_eq!(ta.feed('c', now, &names(), Some(3)), Some(4));
        assert_eq!(ta.feed('c', now, &names(), Some(4)), Some(2));
    }

    /// 다른 글자를 이어서 입력하면 접두사 검색이 되며, 반복 입력과는 다르게 동작한다.
    #[test]
    fn different_letters_extend_the_prefix_instead_of_cycling() {
        let mut ta = TypeAhead::default();
        let now = Instant::now();
        assert_eq!(ta.feed('c', now, &names(), Some(2)), Some(3));
        assert_eq!(ta.feed('o', now, &names(), Some(3)), Some(4));
    }

    /// 일치하는 항목이 없으면 선택과 버퍼가 그대로 유지되어, 다음 글자가 오타에 영향을
    /// 받지 않는다.
    #[test]
    fn a_letter_with_no_match_leaves_the_buffer_as_it_was() {
        let mut ta = TypeAhead::default();
        let now = Instant::now();
        assert_eq!(ta.feed('z', now, &names(), None), None);
        // 버퍼를 되돌리지 않았다면 이 입력은 `zc`를 찾아 실패한다.
        assert_eq!(ta.feed('c', now, &names(), None), Some(2));
    }

    /// 타임아웃이 지나면 버퍼가 비워져 한 글자 검색으로 돌아간다.
    #[test]
    fn the_buffer_expires_after_the_timeout() {
        let mut ta = TypeAhead::default();
        let now = Instant::now();
        assert_eq!(ta.feed('c', now, &names(), None), Some(2));
        // 버퍼가 남아 있었다면 `co` 접두사로 검색했을 입력이다.
        let later = now + TYPE_AHEAD_TIMEOUT;
        assert_eq!(ta.feed('o', later, &names(), Some(2)), None);
    }

    /// ASCII 영숫자 한 글자만 소비한다.
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

    /// 기본 프리셋에는 수식 키 없는 영숫자 단축키가 없어 막히는 글자가 없다.
    #[test]
    fn the_default_preset_blocks_no_letter() {
        assert!(unmodified_binding_chars(&KeybindingSettings::default()).is_empty());
    }

    /// 사용자가 수식 키 없이 글자를 단축키로 등록하면 그 글자는 단축키가 가져간다.
    #[test]
    fn a_letter_bound_without_a_modifier_is_left_to_the_shortcut() {
        let kb = KeybindingSettings {
            explorer_refresh: vec!["r".to_string()],
            ..Default::default()
        };
        assert!(unmodified_binding_chars(&kb).contains(&'r'));
    }

    /// 수식 키가 붙은 단축키는 텍스트 이벤트를 만들지 않으므로 충돌하지 않는다.
    #[test]
    fn a_letter_bound_with_a_modifier_stays_available() {
        let kb = KeybindingSettings {
            explorer_refresh: vec!["ctrl+r".to_string()],
            ..Default::default()
        };
        assert!(!unmodified_binding_chars(&kb).contains(&'r'));
    }
}
