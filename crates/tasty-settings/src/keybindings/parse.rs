//! 바인딩 문자열 파서와 modifier 조합 — 순수 `&str` 로직, UI·winit·egui 비의존.
//!
//! `KeybindingSettings` 가 저장하는 값(`"ctrl+shift+n"` 같은 콤보, `"ctrl+shift"` 같은
//! 축 modifier 조합)의 **해석 규칙**은 그 값을 소유한 이 크레이트에 있다. 실제 키 이벤트와
//! 대조하는 매칭 레이어(winit/egui `Key` 비교)는 본체 `src/adapters/ui/input/shortcuts/`
//! 에 남아 여기의 파싱 결과를 소비한다.
//!
//! 이 자리에 있는 이유는 소비처가 매칭 레이어 하나가 아니기 때문이다 — 단축키 이식
//! 번들의 `option` 판정(`tasty-host-plugin` 의 `keybinding_bundle`)도 같은 파서를 써야
//! 하는데, 그쪽은 본체 크레이트를 볼 수 없고 본체의 매칭 레이어는 `gui` feature 뒤에
//! 있다. 파서를 복제하면 매칭 규칙과 판정 규칙이 조용히 갈린다.
//!
//! 저장 포맷(OS 독립 추상 토큰)과 표시 포맷의 분리는
//! `docs/design/policies/key-mapping.md` 가 정본이다.

/// 파싱된 바인딩 — 기대 modifier 상태 + 키 토큰.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParsedBinding<'a> {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    /// macOS 전용: Option 키. Windows/Linux 에서는 이 값이 true 인 바인딩이 절대
    /// 매칭되지 않는다(`src/adapters/ui/input/shortcuts/binding.rs` 의 매칭 규칙).
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

/// 왼쪽부터 `ctrl+`/`shift+`/`alt+`/`option+` 프리픽스를 순차적으로 떼어낸다.
///
/// `split('+')`을 쓰지 않는 이유: `"ctrl++"`의 두 번째 `+`처럼 키 이름과 구분자가
/// 충돌하는 경우를 다루기 위함. 프리픽스를 하나씩 벗겨내면 남은 부분이 통째로 키가
/// 되므로 구분자 충돌 문제가 사라진다.
///
/// 더블탭 바인딩(`"shift+shift"`·`"ctrl+ctrl"`·`"alt+alt"`)은 별도 경로가 다루며
/// 여기서는 `None` 이다 — 프리픽스를 떼면 남는 것이 모디파이어 키워드 단독이라 아래
/// 거부 규칙에 그대로 걸린다(별도 목록을 두지 않는 이유: 같은 사실이 두 자리에
/// 적히면 한쪽만 고쳐진다. 이 모듈의 `double_tap_spellings_are_rejected` 테스트가
/// 그 동치를 고정한다).
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

/// 두 바인딩 문자열이 **같은 콤보**를 가리키는지 — 대소문자와 modifier 순서를
/// 무시하고 비교한다.
///
/// 매칭을 실제로 하는 [`parse_binding`] 이 modifier 프리픽스를 순서 무관하게 벗기고
/// 키 토큰을 `to_ascii_lowercase` 로 비교하므로, "같은 콤보인가" 판정도 반드시 같은
/// 경로를 타야 한다 — 원시 문자열 비교(`"Ctrl+F"` != `"ctrl+f"`)면 매칭 규칙과 갈라진다.
/// webview 포워딩 정책이 plugin 콤보를 페이지 예약 콤보와 대조할 때, 이식 판정이
/// 대체 조합의 충돌을 볼 때 쓴다.
///
/// 어느 한쪽이 파싱 불가(빈 문자열·modifier 단독 등)면 원시 문자열의 대소문자 무시
/// 비교로 폴백한다.
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

/// `option` 축 존재 여부 — macOS 전용. 비-macOS 는 조합 공간에서 완전히 빠진다.
#[cfg(target_os = "macos")]
pub const OPTION_AXIS: bool = true;
/// `option` 축 존재 여부 — macOS 전용. 비-macOS 는 조합 공간에서 완전히 빠진다.
#[cfg(not(target_os = "macos"))]
pub const OPTION_AXIS: bool = false;

/// modifier 조합 — 4축 bool. `option` 은 macOS 전용(비-macOS 에선 항상 false 로만 등장).
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

    /// modifier-only 조합 문자열(`"ctrl"` / `"alt"` / `"ctrl+shift"` / `"option+shift"`)을
    /// [`Combo`] 로 파싱한다. quick-switch 축 modifier(`KeybindingSettings::tab_switch_modifier`
    /// 등)와 hint 역할 주입이 공유하는 **단일 소스** — `if shift` 하드코딩을 대체한다.
    ///
    /// modifier 토큰(`ctrl`/`shift`/`alt`/`option`)은 `+` 를 키로 갖지 않으므로 [`parse_binding`]
    /// 의 프리픽스-스트립 대신 `split('+')` 로 충분하다(구분자 충돌 없음). 알 수 없는 토큰이
    /// 하나라도 섞이거나(`"none"`·키 문자·`INDIVIDUAL_SWITCH_MODIFIER` sentinel) 빈 문자열이면
    /// `None`(=역할/매칭 없음).
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
    let option_states: &[bool] = if OPTION_AXIS {
        &[false, true]
    } else {
        &[false]
    };
    let mut out = Vec::new();
    for ctrl in [false, true] {
        for alt in [false, true] {
            for &option in option_states {
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

/// 사용 가능한 축 전체의 비어있지 않은 조합을 정렬해 반환(OS-aware).
///
/// 설정 UI 의 quick-switch modifier 피커가 소비한다 — 열거된 유효 조합만 선택 가능하게
/// 해 쓰레기 값 저장을 원천 차단한다. macOS 는 `option` 축 포함(15개), 그 외는 제외(7개).
/// [`Combo::name`] 이 저장용 정규 문자열, `KeybindingSettings::format_display` 가 표시용.
pub fn all_modifier_combos() -> Vec<Combo> {
    let mut combos = all_axis_combos();
    combos.sort_by_key(|c| c.sort_key());
    combos
}

/// 눌린 조합 `held` 를 부분집합으로 포함하는 모든 조합을 정렬해 반환.
///
/// `held` 가 단일 축이면 그 축을 포함하는 조합 전체(macOS 8개·비-macOS 4개), 다축이면
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

    /// 더블탭 표기 셋은 프리픽스를 떼면 모디파이어 키워드 단독만 남아 거부된다.
    /// [`parse_binding`] 이 그 셋을 별도 목록으로 들지 않는 근거 — 목록을 두면
    /// 더블탭 표기가 늘 때 두 자리를 함께 고쳐야 한다.
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
        // 파싱은 플랫폼 무관하다 — 비-macOS 에서도 `option` 축이 true 로 잡혀야
        // 이식 판정(`keybinding_bundle`)이 그 바인딩을 찾아낼 수 있다.
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
        let expected = if OPTION_AXIS { 15 } else { 7 };
        assert_eq!(all_modifier_combos().len(), expected);
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
