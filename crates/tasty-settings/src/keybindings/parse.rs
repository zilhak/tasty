//! 바인딩 문자열과 modifier 조합 파싱. 실제 키 이벤트 매칭은 tasty-key-match가 맡는다.
//! UI와 단축키 이식 검사도 이 파서를 공유한다.

/// 파싱된 바인딩 — 기대 modifier 상태 + 키 토큰.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedBinding<'a> {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// 하단 수정자 열의 가운데 키. macOS 는 Option, Windows·Linux 는 같은 위치의 Win·Super 다
    /// (`crates/tasty-key-match/src/lib.rs` 의 매칭 규칙).
    pub option: bool,
    /// 키 토큰 (문자 `"+"`, `"-"`, `"a"` 또는 네임 `"plus"`, `"f1"`, `"tab"` 등).
    /// 공백/모디파이어 키워드는 거부되어 여기 오지 않는다.
    pub key: &'a str,
}

impl ParsedBinding<'_> {
    /// 이 바인딩이 요구하는 modifier 축만 떼어낸 조합.
    pub fn combo(&self) -> Combo {
        Combo {
            ctrl: self.ctrl,
            alt: self.alt,
            option: self.option,
            shift: self.shift,
        }
    }
}

/// modifier 접두사를 차례로 읽고 남은 문자열을 키로 쓴다. ctrl++처럼 + 자체가 키인 경우를 보존한다.
/// modifier만 남는 더블탭 표기는 여기서 거절하며 별도 입력 경로가 처리한다.
pub fn parse_binding(binding: &str) -> Option<ParsedBinding<'_>> {
    if binding.is_empty() {
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

/// 대소문자와 modifier 순서를 무시해 조합을 비교한다. 파싱에 실패하면 원문을 대소문자 무시로 비교한다.
pub fn bindings_equivalent(a: &str, b: &str) -> bool {
    match (parse_binding(a), parse_binding(b)) {
        (Some(pa), Some(pb)) => {
            pa.ctrl == pb.ctrl
                && pa.shift == pb.shift
                && pa.alt == pb.alt
                && pa.option == pb.option
                && pa.key.eq_ignore_ascii_case(pb.key)
        }
        _ => a.eq_ignore_ascii_case(b),
    }
}

/// 네 modifier 상태. option 축은 모든 OS 에 있다(macOS Option · Windows Win · Linux Super).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub option: bool,
    pub shift: bool,
}

impl Combo {
    /// 눌린 축 개수(조합 크기).
    pub fn size(&self) -> usize {
        [self.ctrl, self.alt, self.option, self.shift]
            .into_iter()
            .filter(|&b| b)
            .count()
    }

    /// 이 조합이 눌린 셋 `other` 를 부분집합으로 포함하는지(눌린 셋 ⊆ self).
    ///
    /// 각 축에 대해 "`other` 에서 눌리지 않았거나, 눌렸다면 self 도 눌림" 을 요구한다.
    /// 예) `{ctrl,shift}.contains_all({ctrl})` = true, `{ctrl}.contains_all({ctrl,shift})` = false.
    pub fn contains_all(&self, other: Combo) -> bool {
        (!other.ctrl || self.ctrl)
            && (!other.alt || self.alt)
            && (!other.option || self.option)
            && (!other.shift || self.shift)
    }

    /// 정렬 키: `(크기, 눌린 축의 우선순위 오름차순 배열)`.
    ///
    /// 우선순위 `Ctrl(0) < Alt(1) < Option(2) < Shift(3)`. 크기를 1차 키로 두어
    /// "크기 오름차순" 을 보장하고, 같은 크기 안에서는 축 우선순위 배열의 사전식
    /// 비교로 순서가 정해진다. 축을 우선순위 순서로 순회하므로 배열은 이미 오름차순.
    fn sort_key(&self) -> (usize, [u8; 4]) {
        let mut prios = [u8::MAX; 4];
        let mut i = 0;
        for (present, prio) in [
            (self.ctrl, 0u8),
            (self.alt, 1),
            (self.option, 2),
            (self.shift, 3),
        ] {
            if present {
                prios[i] = prio;
                i += 1;
            }
        }
        (self.size(), prios)
    }

    /// modifier만 있는 문자열을 파싱한다. 알려진 네 토큰 외 이름이나 빈 값은 None이다.
    /// 키 문자 +를 다루지 않으므로 여기서는 split('+')를 사용한다.
    pub fn parse_modifiers(s: &str) -> Option<Combo> {
        let mut c = Combo::default();
        for part in s.split('+') {
            match part.trim().to_ascii_lowercase().as_str() {
                "ctrl" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" => c.alt = true,
                "option" => c.option = true,
                _ => return None,
            }
        }
        if c.size() == 0 { None } else { Some(c) }
    }

    /// 조합 이름 — 우선순위 순서로 `+` 연결. 예) `{ctrl,shift}` → `"ctrl+shift"`.
    /// 저장용 정규 문자열이다(UI 표시 문자열은 `KeybindingSettings::format_display`).
    pub fn name(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.ctrl {
            parts.push("ctrl");
        }
        if self.alt {
            parts.push("alt");
        }
        if self.option {
            parts.push("option");
        }
        if self.shift {
            parts.push("shift");
        }
        parts.join("+")
    }
}

/// 사용 가능한 축 전체에 대한 비어있지 않은 조합 목록(정렬 전).
fn all_axis_combos() -> Vec<Combo> {
    let mut out = Vec::new();
    for ctrl in [false, true] {
        for alt in [false, true] {
            for option in [false, true] {
                for shift in [false, true] {
                    let c = Combo {
                        ctrl,
                        alt,
                        option,
                        shift,
                    };
                    if c.size() > 0 {
                        out.push(c);
                    }
                }
            }
        }
    }
    out
}

/// 비어 있지 않은 modifier 조합 15개를 정렬한다. 네 축이 모든 OS 에 있으므로 OS 와 무관하다.
pub fn all_modifier_combos() -> Vec<Combo> {
    let mut combos = all_axis_combos();
    combos.sort_by_key(|c| c.sort_key());
    combos
}

/// 눌린 조합 `held` 를 부분집합으로 포함하는 모든 조합을 정렬해 반환.
///
/// `held` 가 단일 축이면 그 축을 포함하는 조합 전체(8개), 다축이면
/// 그 축들을 **모두** 포함하는 조합으로 좁혀진다. 정렬은 `Combo::sort_key` 규칙 —
/// 첫 원소는 항상 `held` 자신(가장 작은 크기)이므로 헤더와 첫 섹션이 일치한다.
pub fn combos_containing_all(held: Combo) -> Vec<Combo> {
    let mut combos: Vec<Combo> = all_axis_combos()
        .into_iter()
        .filter(|c| c.contains_all(held))
        .collect();
    combos.sort_by_key(|c| c.sort_key());
    combos
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 더블탭 표기는 일반 바인딩으로 파싱하지 않는다.
    #[test]
    fn double_tap_spellings_are_rejected() {
        for b in ["shift+shift", "ctrl+ctrl", "alt+alt", "Shift+Shift"] {
            assert!(parse_binding(b).is_none(), "{b} 가 파싱됐다");
        }
    }

    #[test]
    fn parses_modifier_prefixes_in_any_order() {
        let a = parse_binding("ctrl+shift+a").unwrap();
        let b = parse_binding("shift+ctrl+a").unwrap();
        assert_eq!(a, b);
        assert!(a.ctrl && a.shift && !a.alt && !a.option);
        assert_eq!(a.key, "a");
    }

    #[test]
    fn keeps_plus_as_key_token() {
        let p = parse_binding("ctrl++").unwrap();
        assert!(p.ctrl);
        assert_eq!(p.key, "+");
    }

    #[test]
    fn rejects_empty_and_modifier_only() {
        assert!(parse_binding("").is_none());
        assert!(parse_binding("ctrl+").is_none());
        for m in ["ctrl", "shift", "alt", "option"] {
            assert!(parse_binding(m).is_none(), "{m}");
        }
    }

    #[test]
    fn option_axis_survives_parsing_on_every_platform() {
        // 파싱은 플랫폼 무관하다. option 은 비-macOS 에서 Win·Super 로 매칭된다.
        let p = parse_binding("alt+option+t").unwrap();
        assert!(p.alt && p.option);
        assert_eq!(p.key, "t");
        assert!(Combo::parse_modifiers("option+shift").unwrap().option);
    }

    #[test]
    fn parsed_combo_drops_the_key_token() {
        let p = parse_binding("ctrl+option+1").unwrap();
        assert_eq!(
            p.combo(),
            Combo {
                ctrl: true,
                alt: false,
                option: true,
                shift: false,
            }
        );
    }

    #[test]
    fn modifier_combo_names_round_trip() {
        for c in all_modifier_combos() {
            assert_eq!(Combo::parse_modifiers(&c.name()), Some(c), "{}", c.name());
        }
    }

    #[test]
    fn individual_sentinel_is_not_a_modifier_combo() {
        assert!(
            Combo::parse_modifiers(crate::KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER).is_none()
        );
    }

    #[test]
    fn combo_enumeration_matches_the_option_axis() {
        assert_eq!(all_modifier_combos().len(), 15);
        assert!(all_modifier_combos().iter().all(|c| c.size() > 0));
    }

    #[test]
    fn combos_containing_all_starts_with_held() {
        let held = Combo {
            ctrl: true,
            ..Combo::default()
        };
        let list = combos_containing_all(held);
        assert_eq!(list.first(), Some(&held));
        assert!(list.iter().all(|c| c.contains_all(held)));
    }
}
