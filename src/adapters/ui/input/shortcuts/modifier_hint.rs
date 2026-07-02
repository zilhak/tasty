//! Modifier-hint 오버레이 **조합 콘텐츠 모델** — 순수 로직, UI 비의존.
//!
//! "누른 modifier `M` → 정렬된 `(조합, 항목 목록)` 리스트" 를 만드는 읽기 전용
//! 모델이다. 렌더링/디자인(modifier-hint-03) 과 분리돼 있으며 기존 동작을 바꾸지
//! 않는다 — 오직 표시할 데이터를 계산한다.
//!
//! ## 조합 공간·정렬
//! - 축: `ctrl` / `alt` / `option` / `shift`. **`option` 은 macOS 전용**(비-macOS 컴파일에선
//!   축 자체가 빠진다). `"alt"` 토큰은 macOS 에서 물리 Command(⌘), 그 외에서 Alt 에
//!   매핑되는데(위치 기반 추상화), 이는 [`super::binding`] 의 파싱 규칙과 동일하므로
//!   여기서는 파서가 채워준 `ParsedBinding` 축을 그대로 쓴다.
//! - 정렬: ① 조합 크기 오름차순 → ② 같은 크기 내 우선순위 `Ctrl < Alt(Cmd) < Option < Shift`.
//!   예) Alt 홀드(macOS) → `alt, ctrl+alt, alt+option, alt+shift, ctrl+alt+option,
//!   ctrl+alt+shift, alt+option+shift, ctrl+alt+option+shift`.
//!
//! ## 열거 소스
//! - `KeybindingSettings` 고정 필드 전체([`KeybindingSettings::GENERAL_BINDING_FIELDS`] 재사용)
//!   + `script_bindings`.
//! - Plugin command 의 `EffectiveBinding`([`PluginBindingInput`] 로 주입) — 디자인 확정상 **전량
//!   포함**한다. focus 스코핑(전체 등록 plugin vs focused RemoteSurface 만)은 modifier-hint-03
//!   wiring 의 결정사항이라 이 모델은 받은 것을 모두 노출한다 (open).
//! - 더블탭(`shift+shift` 등)·무 modifier(`f11`)·모디파이어 단독(`ctrl`) 바인딩은
//!   [`parse_binding`] 이 `None` 을 돌려주어 자연히 제외된다.
//!
//! ## 특수 역할 (단축키 목록 외 설명 행) — **설정 현재값 기준**(기본값 가정 금지)
//! - Shift 를 **포함하는 모든 조합**: TUI 마우스 캡처 임시 우회(Shift+드래그=로컬 선택 등).
//!   판정이 `shift_key()` 만 검사하고 타 modifier 를 배제하지 않으므로 조합 전체에 붙는다.
//! - `tab_switch_modifier` 단독 조합: 탭 전환 + 숫자 오버레이.
//! - `workspace_switch_modifier` 단독 조합: 워크스페이스 전환 + 숫자 오버레이.
//! - `link_click_modifier`(`general`) 단독 조합: modifier+클릭 링크 열기. `"none"` 이면 역할 없음.
//!
//! 빈 조합(바인딩·역할 모두 없음)은 섹션 자체를 생략한다.
//!
//! NOTE: 소비처(modifier-hint-03 오버레이 wiring)가 아직 연결되지 않아 공개 심볼이
//! 전부 미사용이다. 03 배선 완료 시 이 allow 를 제거한다.
#![allow(dead_code)]

use tasty_settings::KeybindingSettings;

use crate::plugin::command_registry::{EffectiveBinding, PluginCommandEntry, effective_binding};
use crate::plugin::registry_state::ShortcutOverride;

use super::binding::parse_binding;

/// `option` 축 존재 여부 — macOS 전용. 비-macOS 는 조합 공간에서 완전히 빠진다.
#[cfg(target_os = "macos")]
const OPTION_AXIS: bool = true;
#[cfg(not(target_os = "macos"))]
const OPTION_AXIS: bool = false;

/// 사용자가 누르고 있는(홀드) 단일 modifier. 이 값을 포함하는 모든 조합을 노출한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldModifier {
    Ctrl,
    /// `"alt"` 토큰 — macOS 에서 물리 Command(⌘), 그 외에서 Alt.
    Alt,
    /// macOS 전용 Option 키. 비-macOS 에선 조합이 생성되지 않는다.
    Option,
    Shift,
}

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

    /// 이 조합이 주어진 홀드 modifier 를 포함하는지.
    pub fn contains(&self, held: HeldModifier) -> bool {
        match held {
            HeldModifier::Ctrl => self.ctrl,
            HeldModifier::Alt => self.alt,
            HeldModifier::Option => self.option,
            HeldModifier::Shift => self.shift,
        }
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

    /// 조합 이름 — 우선순위 순서로 `+` 연결. 예) `{ctrl,shift}` → `"ctrl+shift"`.
    /// 테스트·디버그용. UI 표시 문자열이 아니다(그건 03 이 토큰별로 그린다).
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
    let option_states: &[bool] = if OPTION_AXIS { &[false, true] } else { &[false] };
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

/// 홀드 modifier `M` 을 포함하는 모든 조합을 정렬해 반환.
///
/// macOS 는 8개, 비-macOS 는 4개(option 축 없음). 정렬은 [`Combo::sort_key`] 규칙.
pub fn combos_containing(held: HeldModifier) -> Vec<Combo> {
    let mut combos: Vec<Combo> = all_axis_combos()
        .into_iter()
        .filter(|c| c.contains(held))
        .collect();
    combos.sort_by_key(|c| c.sort_key());
    combos
}

/// 조합 섹션 안의 한 항목(바인딩된 액션/스크립트/plugin command).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintRow {
    pub source: HintRowSource,
    /// 이 행의 원본 바인딩 문자열(예: `"ctrl+shift+t"`). 03 이 키캡으로 분해해 그린다.
    pub binding: String,
}

/// 항목의 출처 — 라벨 해석 방식을 결정한다(모델은 키만 반환, 문자열 해석은 03).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintRowSource {
    /// 고정 호스트 액션 — 라벨 i18n 키(`settings.keybindings.*_label`).
    Host { label_key: &'static str },
    /// 사용자 스크립트 — `script_id`. 03 이 `ScriptRegistry` 로 이름을 해석한다.
    Script { script_id: String },
    /// Plugin command — `plugin_id` + command title i18n 키. 03 이 "plugin_id: title" 로 표기.
    Plugin {
        plugin_id: String,
        title_i18n_key: String,
    },
}

/// 조합에 붙는 특수 역할(바인딩이 아닌 설명 행).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HintRole {
    /// Shift 포함 조합 — TUI 마우스 캡처 임시 우회.
    MouseCaptureBypass,
    /// `tab_switch_modifier` 단독 — 탭 전환 + 숫자 오버레이.
    TabSwitch,
    /// `workspace_switch_modifier` 단독 — 워크스페이스 전환 + 숫자 오버레이.
    WorkspaceSwitch,
    /// `link_click_modifier` 단독 — modifier+클릭 링크 열기.
    LinkClick,
}

impl HintRole {
    /// 역할 설명 i18n 키. 03 이 `t()` 로 해석한다.
    pub fn desc_key(&self) -> &'static str {
        match self {
            HintRole::MouseCaptureBypass => "modifier_hint.role.mouse_capture_bypass",
            HintRole::TabSwitch => "modifier_hint.role.tab_switch",
            HintRole::WorkspaceSwitch => "modifier_hint.role.workspace_switch",
            HintRole::LinkClick => "modifier_hint.role.link_click",
        }
    }
}

/// 한 조합 섹션 — 조합 + 바인딩 항목들 + 특수 역할 설명들.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HintSection {
    pub combo: Combo,
    /// 이 조합에 매핑된 바인딩 항목(필드 순서 → 스크립트 → plugin 순).
    pub rows: Vec<HintRow>,
    /// 이 조합에 해당하는 특수 역할.
    pub roles: Vec<HintRole>,
}

impl HintSection {
    fn is_empty(&self) -> bool {
        self.rows.is_empty() && self.roles.is_empty()
    }
}

/// Plugin command 하나의 표시용 입력(effective 바인딩 해석 결과).
///
/// [`EffectiveBinding`] 을 재사용해 실제 매칭 키로 환원한 값을 담는다. registry 순회·
/// override 소스·focus 스코핑은 03 wiring 이 담당하고, 이 모델은 완성된 입력을 받는다
/// (순수 함수 테스트 가능성 유지).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBindingInput {
    pub plugin_id: String,
    pub title_i18n_key: String,
    /// 실제 매칭에 쓰는 키 목록(effective). 빈 벡터면 미할당.
    pub bindings: Vec<String>,
}

impl PluginBindingInput {
    /// registry entry + 사용자 override + host keybindings → 표시용 입력.
    ///
    /// `EffectiveBinding::Inherit` 는 호스트에서 해석된 `keys` 를, `Keys` 는 그대로,
    /// `None` 은 빈 목록을 쓴다. focus 스코핑은 하지 않는다(모델 전량 노출, open).
    pub fn resolve(
        entry: &PluginCommandEntry,
        user_override: Option<&ShortcutOverride>,
        host_kb: &KeybindingSettings,
    ) -> Self {
        let bindings = match effective_binding(entry, user_override, host_kb) {
            EffectiveBinding::Keys(v) => v,
            EffectiveBinding::Inherit { keys, .. } => keys,
            EffectiveBinding::None => Vec::new(),
        };
        Self {
            plugin_id: entry.plugin_id.clone(),
            title_i18n_key: entry.title_i18n_key.clone(),
            bindings,
        }
    }
}

/// modifier 이름 토큰(`"ctrl"`/`"alt"`) → 단독 축 조합. 그 외(빈 문자열·`"none"` 등)는 None.
fn single_axis_combo(token: &str) -> Option<Combo> {
    match token.to_ascii_lowercase().as_str() {
        "ctrl" => Some(Combo {
            ctrl: true,
            ..Default::default()
        }),
        "alt" => Some(Combo {
            alt: true,
            ..Default::default()
        }),
        _ => None,
    }
}

/// 파싱된 바인딩 축을 [`Combo`] 로.
fn combo_of(parsed: &super::binding::ParsedBinding<'_>) -> Combo {
    Combo {
        ctrl: parsed.ctrl,
        alt: parsed.alt,
        option: parsed.option,
        shift: parsed.shift,
    }
}

/// 홀드 modifier `M` 에 대한 정렬된 조합 콘텐츠를 만든다.
///
/// - `held`: 사용자가 누르고 있는 단일 modifier.
/// - `kb`: 고정 필드 + `script_bindings` + tab/workspace switch modifier 소스.
/// - `link_click_modifier`: `general.link_click_modifier`(`"ctrl"`|`"alt"`|`"none"`).
/// - `plugin_bindings`: 표시할 plugin command 입력(전량, [`PluginBindingInput::resolve`] 산출).
///
/// 반환은 정렬된 섹션 목록이며 **빈 섹션(바인딩·역할 모두 없음)은 생략**된다.
pub fn build_hint_sections(
    held: HeldModifier,
    kb: &KeybindingSettings,
    link_click_modifier: &str,
    plugin_bindings: &[PluginBindingInput],
) -> Vec<HintSection> {
    let mut sections: Vec<HintSection> = combos_containing(held)
        .into_iter()
        .map(|combo| HintSection {
            combo,
            rows: Vec::new(),
            roles: Vec::new(),
        })
        .collect();

    let push_row = |combo: Combo, row: HintRow, sections: &mut Vec<HintSection>| {
        if let Some(sec) = sections.iter_mut().find(|s| s.combo == combo) {
            sec.rows.push(row);
        }
    };

    // 1. 고정 호스트 액션 필드 (GENERAL_BINDING_FIELDS 를 SoT 로 재사용).
    //    toggle_sidebar/collapse 는 라벨 키가 없어 이 목록에서 제외돼 있으므로 자연히 빠진다.
    for (field_id, label_key) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        let Some(bindings) = kb.get_bindings(field_id) else {
            continue;
        };
        for b in bindings {
            let Some(parsed) = parse_binding(b) else {
                continue; // 더블탭·무 modifier·modifier 단독 → 제외
            };
            let combo = combo_of(&parsed);
            if !combo.contains(held) {
                continue;
            }
            push_row(
                combo,
                HintRow {
                    source: HintRowSource::Host { label_key },
                    binding: b.clone(),
                },
                &mut sections,
            );
        }
    }

    // 2. 사용자 스크립트 동적 바인딩.
    for sb in &kb.script_bindings {
        let Some(parsed) = parse_binding(&sb.combo) else {
            continue;
        };
        let combo = combo_of(&parsed);
        if !combo.contains(held) {
            continue;
        }
        push_row(
            combo,
            HintRow {
                source: HintRowSource::Script {
                    script_id: sb.script_id.clone(),
                },
                binding: sb.combo.clone(),
            },
            &mut sections,
        );
    }

    // 3. Plugin command (전량 노출 — focus 스코핑은 03 결정, open).
    for pb in plugin_bindings {
        for b in &pb.bindings {
            let Some(parsed) = parse_binding(b) else {
                continue;
            };
            let combo = combo_of(&parsed);
            if !combo.contains(held) {
                continue;
            }
            push_row(
                combo,
                HintRow {
                    source: HintRowSource::Plugin {
                        plugin_id: pb.plugin_id.clone(),
                        title_i18n_key: pb.title_i18n_key.clone(),
                    },
                    binding: b.clone(),
                },
                &mut sections,
            );
        }
    }

    // 4. 특수 역할 주입 (설정 현재값 기준).
    let tab_combo = single_axis_combo(&kb.tab_switch_modifier);
    let ws_combo = single_axis_combo(&kb.workspace_switch_modifier);
    let link_combo = single_axis_combo(link_click_modifier); // "none" → None
    for sec in &mut sections {
        // Shift 포함 조합 전체 → 마우스 캡처 우회.
        if sec.combo.shift {
            sec.roles.push(HintRole::MouseCaptureBypass);
        }
        if Some(sec.combo) == tab_combo {
            sec.roles.push(HintRole::TabSwitch);
        }
        if Some(sec.combo) == ws_combo {
            sec.roles.push(HintRole::WorkspaceSwitch);
        }
        if Some(sec.combo) == link_combo {
            sec.roles.push(HintRole::LinkClick);
        }
    }

    // 5. 빈 섹션 생략.
    sections.retain(|s| !s.is_empty());
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(combos: &[Combo]) -> Vec<String> {
        combos.iter().map(Combo::name).collect()
    }

    fn section_names(sections: &[HintSection]) -> Vec<String> {
        sections.iter().map(|s| s.combo.name()).collect()
    }

    /// 테스트용 기본 키바인딩(preset_tasty): new_workspace="alt+n", new_tab="alt+t",
    /// restore_closed="ctrl+shift+t", tab_switch="ctrl", workspace_switch="alt".
    fn kb() -> KeybindingSettings {
        KeybindingSettings::preset_tasty()
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn combos_for_alt_are_sorted_by_size_then_priority_macos() {
        let combos = combos_containing(HeldModifier::Alt);
        assert_eq!(
            names(&combos),
            [
                "alt",
                "ctrl+alt",
                "alt+option",
                "alt+shift",
                "ctrl+alt+option",
                "ctrl+alt+shift",
                "alt+option+shift",
                "ctrl+alt+option+shift",
            ]
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn combos_for_alt_are_sorted_by_size_then_priority_non_macos() {
        // 비-macOS: option 축 없음 → 4개.
        let combos = combos_containing(HeldModifier::Alt);
        assert_eq!(names(&combos), ["alt", "ctrl+alt", "alt+shift", "ctrl+alt+shift"]);
    }

    #[test]
    fn combos_for_ctrl_start_with_single_ctrl_then_size_two() {
        let combos = combos_containing(HeldModifier::Ctrl);
        // 크기 1 이 먼저, 그 다음 크기 2 (ctrl+alt 가 ctrl 단독보다 뒤).
        assert_eq!(combos.first().map(Combo::name), Some("ctrl".to_string()));
        assert!(combos.iter().all(|c| c.ctrl));
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn non_macos_never_generates_option_combos() {
        for held in [
            HeldModifier::Ctrl,
            HeldModifier::Alt,
            HeldModifier::Shift,
        ] {
            for c in combos_containing(held) {
                assert!(!c.option, "option 축이 비-macOS 에서 생성됨: {}", c.name());
            }
        }
        // Option 홀드 자체도 비-macOS 에선 아무 조합도 없어야 한다.
        assert!(combos_containing(HeldModifier::Option).is_empty());
    }

    #[test]
    fn restore_closed_grouped_into_ctrl_shift() {
        // 기본 restore_closed = "ctrl+shift+t" → Ctrl 홀드 시 ctrl+shift 섹션에 분류.
        let sections = build_hint_sections(HeldModifier::Ctrl, &kb(), "ctrl", &[]);
        let ctrl_shift = sections
            .iter()
            .find(|s| s.combo.name() == "ctrl+shift")
            .expect("ctrl+shift 섹션 존재해야 함");
        assert!(ctrl_shift.rows.iter().any(|r| {
            r.binding == "ctrl+shift+t"
                && matches!(
                    r.source,
                    HintRowSource::Host {
                        label_key: "settings.keybindings.restore_closed_label"
                    }
                )
        }));
    }

    #[test]
    fn alt_section_has_workspace_bindings_and_switch_role() {
        // Alt 홀드 → "alt" 단독 섹션에 new_workspace/new_tab + WorkspaceSwitch 역할.
        let sections = build_hint_sections(HeldModifier::Alt, &kb(), "ctrl", &[]);
        let alt = sections
            .iter()
            .find(|s| s.combo.name() == "alt")
            .expect("alt 섹션 존재");
        assert!(alt.rows.iter().any(|r| r.binding == "alt+n"));
        assert!(alt.rows.iter().any(|r| r.binding == "alt+t"));
        assert!(alt.roles.contains(&HintRole::WorkspaceSwitch));
    }

    #[test]
    fn ctrl_single_section_has_tab_switch_role() {
        let sections = build_hint_sections(HeldModifier::Ctrl, &kb(), "ctrl", &[]);
        let ctrl = sections
            .iter()
            .find(|s| s.combo.name() == "ctrl")
            .expect("ctrl 섹션 존재");
        assert!(ctrl.roles.contains(&HintRole::TabSwitch));
        // link_click_modifier="ctrl" 이므로 LinkClick 역할도 같은 섹션에.
        assert!(ctrl.roles.contains(&HintRole::LinkClick));
    }

    #[test]
    fn shift_containing_sections_get_mouse_capture_bypass() {
        let sections = build_hint_sections(HeldModifier::Ctrl, &kb(), "ctrl", &[]);
        for sec in &sections {
            if sec.combo.shift {
                assert!(
                    sec.roles.contains(&HintRole::MouseCaptureBypass),
                    "shift 포함 섹션 {} 에 우회 역할 누락",
                    sec.combo.name()
                );
            } else {
                assert!(!sec.roles.contains(&HintRole::MouseCaptureBypass));
            }
        }
    }

    #[test]
    fn link_none_produces_no_link_role() {
        let sections = build_hint_sections(HeldModifier::Ctrl, &kb(), "none", &[]);
        assert!(
            sections
                .iter()
                .all(|s| !s.roles.contains(&HintRole::LinkClick))
        );
    }

    #[test]
    fn double_tap_and_no_modifier_bindings_excluded() {
        let mut kb = KeybindingSettings::preset_tasty();
        // 더블탭·무 modifier·modifier 단독 → 어느 섹션에도 안 들어가야 한다.
        kb.new_tab = vec!["shift+shift".into(), "f11".into(), "ctrl".into()];
        // Shift 홀드 섹션에 shift+shift 가 새지 않는지 확인.
        let shift_sections = build_hint_sections(HeldModifier::Shift, &kb, "ctrl", &[]);
        assert!(shift_sections.iter().all(|s| {
            s.rows
                .iter()
                .all(|r| r.binding != "shift+shift" && r.binding != "f11" && r.binding != "ctrl")
        }));
        // Ctrl 홀드에서도 "ctrl" 단독/"f11" 이 새지 않음.
        let ctrl_sections = build_hint_sections(HeldModifier::Ctrl, &kb, "ctrl", &[]);
        assert!(ctrl_sections.iter().all(|s| {
            s.rows
                .iter()
                .all(|r| r.binding != "f11" && r.binding != "ctrl")
        }));
    }

    #[test]
    fn rebound_switch_modifiers_follow_settings() {
        // tab=alt / ws=ctrl 로 재바인딩 → 역할도 반대 섹션으로.
        let mut kb = KeybindingSettings::preset_tasty();
        kb.tab_switch_modifier = "alt".into();
        kb.workspace_switch_modifier = "ctrl".into();

        let alt_sections = build_hint_sections(HeldModifier::Alt, &kb, "none", &[]);
        let alt = alt_sections.iter().find(|s| s.combo.name() == "alt").unwrap();
        assert!(alt.roles.contains(&HintRole::TabSwitch));
        assert!(!alt.roles.contains(&HintRole::WorkspaceSwitch));

        let ctrl_sections = build_hint_sections(HeldModifier::Ctrl, &kb, "none", &[]);
        let ctrl = ctrl_sections
            .iter()
            .find(|s| s.combo.name() == "ctrl")
            .unwrap();
        assert!(ctrl.roles.contains(&HintRole::WorkspaceSwitch));
        assert!(!ctrl.roles.contains(&HintRole::TabSwitch));
    }

    #[test]
    fn plugin_bindings_included_with_source() {
        let plugins = [PluginBindingInput {
            plugin_id: "git-helper".into(),
            title_i18n_key: "git_helper.stage_hunk".into(),
            bindings: vec!["ctrl+alt+g".into()],
        }];
        let sections = build_hint_sections(HeldModifier::Ctrl, &kb(), "ctrl", &plugins);
        let ctrl_alt = sections
            .iter()
            .find(|s| s.combo.name() == "ctrl+alt")
            .expect("ctrl+alt 섹션 존재");
        assert!(ctrl_alt.rows.iter().any(|r| {
            r.binding == "ctrl+alt+g"
                && matches!(
                    &r.source,
                    HintRowSource::Plugin { plugin_id, title_i18n_key }
                        if plugin_id == "git-helper" && title_i18n_key == "git_helper.stage_hunk"
                )
        }));
    }

    #[test]
    fn empty_sections_are_omitted() {
        // 바인딩·역할이 하나도 안 걸리는 조합은 섹션에서 빠진다.
        let mut kb = KeybindingSettings::preset_tasty();
        // 모든 고정 필드를 비워 역할만 남긴다.
        for (field_id, _) in KeybindingSettings::GENERAL_BINDING_FIELDS {
            kb.clear_field(field_id);
        }
        kb.script_bindings.clear();
        // Alt 홀드, switch/link 모두 alt 아닌 값 → alt 단독 섹션에 아무것도 안 붙어 생략.
        kb.tab_switch_modifier = "ctrl".into();
        kb.workspace_switch_modifier = "ctrl".into();
        let sections = build_hint_sections(HeldModifier::Alt, &kb, "ctrl", &[]);
        // alt 단독 섹션은 shift 없음·switch 아님 → 생략되어야 한다.
        assert!(section_names(&sections).iter().all(|n| n != "alt"));
        // 남은 섹션은 전부 비어있지 않아야 한다.
        assert!(sections.iter().all(|s| !s.rows.is_empty() || !s.roles.is_empty()));
    }
}
