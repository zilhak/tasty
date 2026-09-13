//! `option` 바인딩 이식 판정 — 비-macOS 에서 발화 불가능한 자리를 전수로 찾고,
//! 사용자가 고른 대체 값을 그 목록대로 적용한다.
//!
//! `option` 토큰이 든 바인딩은 **비-macOS 에서 절대 매칭되지 않는다**
//! (`src/adapters/ui/input/shortcuts/binding.rs` 의 매칭 규칙: 비-macOS 는
//! `option_matches = !parsed.option`). 화면에는 그대로 보이는데 눌러도 아무 일이
//! 없으므로, macOS 에서 만든 구성을 그대로 가져오면 **조용히 죽은 바인딩**이 남는다.
//! 이식이 깨지는 토큰은 이 하나뿐이다 — 나머지 토큰은 저장이 이미 OS 독립이다
//! (`docs/design/policies/key-mapping.md` "설정 파일 이식성").
//!
//! 판정은 **문자열 검색이 아니라 실제 파서**로 한다. `"option"` 이라는 이름의 키가
//! 있을 가능성·대소문자·토큰 순서를 문자열 매칭으로 다루면 틀리고, 틀리는 방향이
//! 거짓 음성(죽은 바인딩을 못 찾음)이라 조용하다.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use tasty_settings::keybindings::parse::{Combo, bindings_equivalent, parse_binding};
use tasty_settings::{KeybindingSettings, SwitchAxis, SwitchStep};

use super::PluginShortcutOverrides;
use crate::registry_state::ShortcutOverride;

/// 번들이 착지할 플랫폼.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    /// `option` 이 그대로 동작한다 — 마이그레이션이 필요 없다.
    Mac,
    /// `option` 바인딩이 절대 매칭되지 않는다.
    NonMac,
}

impl TargetOs {
    /// 지금 돌고 있는 빌드의 플랫폼. 판정이 컴파일 타임인 이유는 비-macOS 바이너리의
    /// 매칭 경로가 `option` 을 **항상 불일치**로 접기 때문이다 — 런타임에 갈릴 여지가 없다.
    pub fn host() -> Self {
        if cfg!(target_os = "macos") {
            Self::Mac
        } else {
            Self::NonMac
        }
    }
}

/// 콤보 하나가 저장되는 자리. 스캔 결과의 위치이자 [`MigrationPlan`] 의 키다.
///
/// `AxisModifier` 만 콤보가 아니라 **modifier 조합**을 담는다 —
/// [`BindingSite::replacement_kind`] 가 그 차이를 말한다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BindingSite {
    /// 일반 콤보 필드의 `index` 번째 바인딩(한 액션에 콤보가 여럿일 수 있다).
    GeneralBinding {
        field_id: &'static str,
        index: usize,
    },
    /// quick-switch 축 modifier.
    AxisModifier { axis: SwitchAxis },
    /// quick-switch 슬롯. **개별 지정 축에서만** 콤보를 담는다.
    AxisSlot { axis: SwitchAxis, index: usize },
    /// quick-switch 다음/이전. **개별 지정 축에서만** 콤보를 담는다.
    AxisStep { axis: SwitchAxis, step: SwitchStep },
    /// 스크립트 바인딩(스크립트당 하나).
    ScriptBinding { script_id: String },
    /// plugin command override 의 `index` 번째 키. `Inherit`/`None` 은 콤보를 담지
    /// 않으므로 여기 오지 않는다.
    PluginOverride {
        plugin_id: String,
        command_id: String,
        index: usize,
    },
}

/// 이 자리가 요구하는 대체 값의 종류 — 받는 수단이 자리마다 다르다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplacementKind {
    /// 완전 콤보(`"ctrl+alt+t"`). 기존 녹화로 받는다.
    Combo,
    /// modifier 조합(`"ctrl+shift"`). 녹화가 modifier 단독 입력을 무시하므로 녹화로
    /// 못 받는다 — `all_modifier_combos()` 중에서 고르게 해야 한다.
    ModifierCombo,
}

impl BindingSite {
    /// 이 자리에 넣을 값의 종류.
    pub fn replacement_kind(&self) -> ReplacementKind {
        match self {
            Self::AxisModifier { .. } => ReplacementKind::ModifierCombo,
            _ => ReplacementKind::Combo,
        }
    }
}

fn axis_name(axis: SwitchAxis) -> &'static str {
    match axis {
        SwitchAxis::Tab => "tab",
        SwitchAxis::Workspace => "workspace",
        SwitchAxis::Category => "category",
    }
}

impl fmt::Display for BindingSite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GeneralBinding { field_id, index } => write!(f, "{field_id}[{index}]"),
            Self::AxisModifier { axis } => write!(f, "{}.modifier", axis_name(*axis)),
            Self::AxisSlot { axis, index } => write!(f, "{}.slot[{index}]", axis_name(*axis)),
            Self::AxisStep { axis, step } => {
                let s = match step {
                    SwitchStep::Next => "next",
                    SwitchStep::Prev => "prev",
                };
                write!(f, "{}.{s}", axis_name(*axis))
            }
            Self::ScriptBinding { script_id } => write!(f, "script:{script_id}"),
            Self::PluginOverride {
                plugin_id,
                command_id,
                index,
            } => write!(f, "plugin:{plugin_id}/{command_id}[{index}]"),
        }
    }
}

/// 발화 불가능한 `option` 바인딩 하나 — 위치와 지금 저장된 값.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionBinding {
    pub site: BindingSite,
    pub current: String,
}

/// 위치 → 대체 값.
pub type MigrationPlan = BTreeMap<BindingSite, String>;

/// 한 자리를 어떻게 해소하는가 — 대체 값을 주거나, 그 바인딩을 버린다.
///
/// "버린다" 는 사용자가 이 환경에서 그 단축키를 **의도적으로 비워 두는** 선택이라
/// 해소로 센다. 버리면 그 자리는 콤보가 아니게 되므로 대체 값 검증을 받지 않는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Replace(String),
    Unbind,
}

/// 위치 → 해소 방법.
pub type ResolutionPlan = BTreeMap<BindingSite, Resolution>;

/// 대체 값이 **이번에 생긴** 충돌을 만들 때 어떻게 하는가.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictPolicy {
    /// 거절한다([`MigrationError::Conflicts`]).
    Reject,
    /// 충돌 상대 중 **계획 밖의 자리**를 비운다 — 설정 창 충돌 확인의 "적용" 과 같은
    /// 뜻이다. 양쪽이 모두 계획 안이면(대체 값끼리 겹침) 비울 쪽을 정할 수 없어 거절한다.
    UnbindOther,
}

/// 대체 적용이 거절되는 이유.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MigrationError {
    /// 스캔이 찾은 자리 중 대체 값이 안 정해진 것이 있다.
    #[error("대체 값이 안 정해진 자리가 있다: {}", join_sites(.sites))]
    Unassigned { sites: Vec<BindingSite> },
    /// 계획이 지금 스캔에 없는 자리를 가리킨다(낡은 계획).
    #[error("지금 구성에 없는 자리다: {site}")]
    UnknownSite { site: BindingSite },
    /// 콤보 자리에 콤보가 아닌 값이 왔다.
    #[error("\"{value}\" 는 콤보로 파싱되지 않는다 ({site})")]
    NotACombo { site: BindingSite, value: String },
    /// modifier 자리에 modifier 조합이 아닌 값이 왔다.
    #[error("\"{value}\" 는 modifier 조합이 아니다 ({site})")]
    NotAModifierCombo { site: BindingSite, value: String },
    /// 대체 값 자신이 다시 `option` 을 담는다.
    #[error("대체 값 \"{value}\" 가 다시 option 을 담는다 ({site})")]
    ReplacementKeepsOption { site: BindingSite, value: String },
    /// 적용 결과가 새 충돌을 만든다(기존 바인딩과, 또는 대체 값끼리).
    #[error("대체 값이 충돌을 만든다: {}", join_conflicts(.0))]
    Conflicts(Vec<BindingConflict>),
    /// 비울 수 없는 자리를 비우라고 했다 — 축 modifier 는 조합 하나를 반드시 가진다.
    #[error("이 자리는 비울 수 없다: {site}")]
    CannotUnbind { site: BindingSite },
    /// 적용 후에도 `option` 이 남았다 — 스캔과 적용이 갈렸다는 뜻이므로 값으로 받는다.
    #[error("적용 후에도 option 이 남았다: {}", join_sites(.sites))]
    OptionRemains { sites: Vec<BindingSite> },
}

fn join_sites(sites: &[BindingSite]) -> String {
    sites
        .iter()
        .map(BindingSite::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

fn join_conflicts(conflicts: &[BindingConflict]) -> String {
    conflicts
        .iter()
        .map(BindingConflict::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// 같은 조합을 두 자리가 갖는 상태.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingConflict {
    pub combo: String,
    pub a: BindingSite,
    pub b: BindingSite,
}

impl fmt::Display for BindingConflict {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"{}\" ({} ↔ {})", self.combo, self.a, self.b)
    }
}

/// 이 환경에서 발화 불가능한 `option` 바인딩을 **다섯 자리 전부**에서 찾는다.
///
/// 1. 일반 콤보 필드(`GENERAL_BINDING_FIELDS` × 각 원소)
/// 2. quick-switch 축 modifier 셋
/// 3. quick-switch 슬롯·다음/이전 — **그 축이 개별 지정일 때만**. 규칙 기반 축의
///    슬롯은 raw 키 하나라 콤보가 아니고, 그것을 콤보로 해석하면 `"o"` 같은 값이
///    잡히는 거짓 양성이 난다.
/// 4. `script_bindings[].combo`
/// 5. plugin override 의 `Key { value }` — `Inherit`/`None` 은 콤보를 안 담는다.
///
/// `target` 이 [`TargetOs::Mac`] 이면 빈 목록이다.
pub fn scan_option_bindings(
    kb: &KeybindingSettings,
    overrides: &PluginShortcutOverrides,
    target: TargetOs,
) -> Vec<OptionBinding> {
    if target == TargetOs::Mac {
        return Vec::new();
    }
    let mut found = Vec::new();

    for &(field_id, _label) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        let Some(bindings) = kb.get_bindings(field_id) else {
            continue;
        };
        for (index, combo) in bindings.iter().enumerate() {
            if combo_has_option(combo) {
                found.push(OptionBinding {
                    site: BindingSite::GeneralBinding { field_id, index },
                    current: combo.clone(),
                });
            }
        }
    }

    for axis in SwitchAxis::ALL {
        let modifier = axis.modifier(kb);
        if Combo::parse_modifiers(modifier).is_some_and(|c| c.option) {
            found.push(OptionBinding {
                site: BindingSite::AxisModifier { axis },
                current: modifier.to_string(),
            });
        }
        if !axis.is_individual(kb) {
            continue;
        }
        for index in 0..axis.slot_count() {
            let Some(value) = axis.slot(kb, index) else {
                continue;
            };
            if combo_has_option(value) {
                found.push(OptionBinding {
                    site: BindingSite::AxisSlot { axis, index },
                    current: value.to_string(),
                });
            }
        }
        for step in SwitchStep::ALL {
            let value = axis.step(kb, step);
            if combo_has_option(value) {
                found.push(OptionBinding {
                    site: BindingSite::AxisStep { axis, step },
                    current: value.to_string(),
                });
            }
        }
    }

    for binding in &kb.script_bindings {
        if combo_has_option(&binding.combo) {
            found.push(OptionBinding {
                site: BindingSite::ScriptBinding {
                    script_id: binding.script_id.clone(),
                },
                current: binding.combo.clone(),
            });
        }
    }

    for (plugin_id, commands) in overrides {
        for (command_id, ov) in commands {
            let ShortcutOverride::Key { value } = ov else {
                continue;
            };
            for (index, combo) in value.iter().enumerate() {
                if combo_has_option(combo) {
                    found.push(OptionBinding {
                        site: BindingSite::PluginOverride {
                            plugin_id: plugin_id.clone(),
                            command_id: command_id.clone(),
                            index,
                        },
                        current: combo.clone(),
                    });
                }
            }
        }
    }

    found
}

/// 콤보 문자열이 `option` 축을 요구하는지 — 실제 파서로 판정한다.
fn combo_has_option(combo: &str) -> bool {
    parse_binding(combo).is_some_and(|p| p.option)
}

/// 대체 값을 적용해 새 구성을 만든다.
///
/// 계획은 [`scan_option_bindings`]가 찾은 자리를 **빠짐없이** 덮어야 한다 — 하나라도
/// 비면 그 바인딩은 이 환경에서 죽은 채로 남고, 그것이 조용히 통과하면 마이그레이션
/// 화면의 존재 이유가 사라진다.
///
/// 충돌은 **적용 전후의 차분**으로 본다. 원래부터 있던 중복까지 거절하면 사용자가
/// 이번 마이그레이션과 무관한 이유로 막힌다.
pub fn apply_migration(
    kb: &KeybindingSettings,
    overrides: &PluginShortcutOverrides,
    plan: &MigrationPlan,
) -> Result<(KeybindingSettings, PluginShortcutOverrides), MigrationError> {
    let plan: ResolutionPlan = plan
        .iter()
        .map(|(site, value)| (site.clone(), Resolution::Replace(value.clone())))
        .collect();
    resolve_migration(kb, overrides, &plan, ConflictPolicy::Reject)
}

/// [`apply_migration`] 의 일반형 — 자리마다 대체하거나 버릴 수 있고, 충돌을 만났을 때의
/// 처리를 고른다.
///
/// 순서가 요점이다. 대체를 먼저 쓰고(자리 좌표가 그대로다), 그 상태에서 충돌 차분을 본
/// 뒤, **마지막에** 버리는 자리를 지운다. 지우기를 먼저 하면 같은 필드·같은 override 의
/// 뒤쪽 원소가 앞으로 당겨져 좌표가 바뀌고, 좌표로 키를 잡는 충돌 차분이 원래 있던 충돌을
/// 새것으로 오판한다. 버리는 자리는 차분에서 뺀다 — 곧 사라질 값이다.
pub fn resolve_migration(
    kb: &KeybindingSettings,
    overrides: &PluginShortcutOverrides,
    plan: &ResolutionPlan,
    policy: ConflictPolicy,
) -> Result<(KeybindingSettings, PluginShortcutOverrides), MigrationError> {
    let found = scan_option_bindings(kb, overrides, TargetOs::NonMac);
    let found_sites: BTreeSet<&BindingSite> = found.iter().map(|f| &f.site).collect();

    for site in plan.keys() {
        if !found_sites.contains(site) {
            return Err(MigrationError::UnknownSite { site: site.clone() });
        }
    }
    let unassigned: Vec<BindingSite> = found
        .iter()
        .filter(|f| !plan.contains_key(&f.site))
        .map(|f| f.site.clone())
        .collect();
    if !unassigned.is_empty() {
        return Err(MigrationError::Unassigned { sites: unassigned });
    }

    for (site, resolution) in plan {
        match resolution {
            Resolution::Replace(value) => validate_replacement(site, value)?,
            Resolution::Unbind => validate_unbind(site)?,
        }
    }

    let mut new_kb = kb.clone();
    let mut new_overrides = overrides.clone();
    let mut unbind: BTreeSet<BindingSite> = BTreeSet::new();
    for (site, resolution) in plan {
        match resolution {
            Resolution::Replace(value) => write_site(&mut new_kb, &mut new_overrides, site, value),
            Resolution::Unbind => {
                unbind.insert(site.clone());
            }
        }
    }

    let introduced = introduced_between(kb, overrides, &new_kb, &new_overrides, &unbind);
    if !introduced.is_empty() {
        match policy {
            ConflictPolicy::Reject => return Err(MigrationError::Conflicts(introduced)),
            ConflictPolicy::UnbindOther => {
                let unresolvable: Vec<BindingConflict> = introduced
                    .iter()
                    .filter(|c| plan.contains_key(&c.a) && plan.contains_key(&c.b))
                    .cloned()
                    .collect();
                if !unresolvable.is_empty() {
                    return Err(MigrationError::Conflicts(unresolvable));
                }
                for c in &introduced {
                    let other = if plan.contains_key(&c.a) { &c.b } else { &c.a };
                    unbind.insert(other.clone());
                }
            }
        }
    }

    unbind_sites(&mut new_kb, &mut new_overrides, &unbind);

    let remaining = scan_option_bindings(&new_kb, &new_overrides, TargetOs::NonMac);
    if !remaining.is_empty() {
        return Err(MigrationError::OptionRemains {
            sites: remaining.into_iter().map(|f| f.site).collect(),
        });
    }

    Ok((new_kb, new_overrides))
}

/// 계획이 **아직 덜 채워졌어도** 지금 정해진 해소들이 만들 충돌을 돌려준다.
///
/// 마이그레이션 화면이 값을 하나 고를 때마다 그 자리에 충돌 사유를 붙이는 데 쓴다 —
/// [`resolve_migration`] 은 미지정 자리가 있으면 충돌을 보기 전에 거절하므로 그 용도로
/// 못 쓴다. 검증에 걸리는 값(콤보가 아닌 값 등)과 스캔에 없는 자리는 건너뛴다.
pub fn introduced_conflicts(
    kb: &KeybindingSettings,
    overrides: &PluginShortcutOverrides,
    plan: &ResolutionPlan,
) -> Vec<BindingConflict> {
    let (new_kb, new_overrides, unbind) = write_partial(kb, overrides, plan);
    introduced_between(kb, overrides, &new_kb, &new_overrides, &unbind)
}

/// 계획이 **아직 덜 채워졌어도** 지금 정해진 해소를 반영한 구성 — 미리보기 표가 "지금
/// 고른 값이면 무엇이 되는가" 를 그리는 데 쓴다. 검증에 걸리는 값과 스캔에 없는 자리는
/// 건너뛰고, 충돌은 보지 않는다(그것은 [`introduced_conflicts`] 의 일이다).
pub fn preview_resolution(
    kb: &KeybindingSettings,
    overrides: &PluginShortcutOverrides,
    plan: &ResolutionPlan,
) -> (KeybindingSettings, PluginShortcutOverrides) {
    let (mut new_kb, mut new_overrides, unbind) = write_partial(kb, overrides, plan);
    unbind_sites(&mut new_kb, &mut new_overrides, &unbind);
    (new_kb, new_overrides)
}

/// 부분 계획의 대체 값을 쓰고, 비울 자리는 모아서 돌려준다(아직 지우지 않는다 — 좌표가
/// 당겨지면 충돌 차분이 어긋난다).
fn write_partial(
    kb: &KeybindingSettings,
    overrides: &PluginShortcutOverrides,
    plan: &ResolutionPlan,
) -> (
    KeybindingSettings,
    PluginShortcutOverrides,
    BTreeSet<BindingSite>,
) {
    let found: BTreeSet<BindingSite> = scan_option_bindings(kb, overrides, TargetOs::NonMac)
        .into_iter()
        .map(|f| f.site)
        .collect();
    let mut new_kb = kb.clone();
    let mut new_overrides = overrides.clone();
    let mut unbind = BTreeSet::new();
    for (site, resolution) in plan {
        if !found.contains(site) {
            continue;
        }
        match resolution {
            Resolution::Replace(value) => {
                if validate_replacement(site, value).is_ok() {
                    write_site(&mut new_kb, &mut new_overrides, site, value);
                }
            }
            Resolution::Unbind => {
                if validate_unbind(site).is_ok() {
                    unbind.insert(site.clone());
                }
            }
        }
    }
    (new_kb, new_overrides, unbind)
}

/// 적용 전후 충돌 차분 — 버릴 자리가 낀 쌍은 뺀다.
fn introduced_between(
    before_kb: &KeybindingSettings,
    before_overrides: &PluginShortcutOverrides,
    after_kb: &KeybindingSettings,
    after_overrides: &PluginShortcutOverrides,
    unbind: &BTreeSet<BindingSite>,
) -> Vec<BindingConflict> {
    let before = conflicting_pairs(before_kb, before_overrides);
    conflicting_pairs(after_kb, after_overrides)
        .into_iter()
        .filter(|(pair, _)| !before.contains_key(pair))
        .filter(|((a, b), _)| !unbind.contains(a) && !unbind.contains(b))
        .map(|((a, b), combo)| BindingConflict { combo, a, b })
        .collect()
}

/// 비울 수 있는 자리인지 — 축 modifier 는 조합 하나를 반드시 가진다.
fn validate_unbind(site: &BindingSite) -> Result<(), MigrationError> {
    match site {
        BindingSite::AxisModifier { .. } => {
            Err(MigrationError::CannotUnbind { site: site.clone() })
        }
        _ => Ok(()),
    }
}

/// 자리들을 비운다. 같은 필드·같은 override 안에서는 **뒤 좌표부터** 지운다 — 앞을 먼저
/// 지우면 뒤 좌표가 당겨져 엉뚱한 원소를 지운다.
fn unbind_sites(
    kb: &mut KeybindingSettings,
    overrides: &mut PluginShortcutOverrides,
    sites: &BTreeSet<BindingSite>,
) {
    for site in sites.iter().rev() {
        match site {
            BindingSite::GeneralBinding { field_id, index } => {
                kb.remove_binding(field_id, *index);
            }
            // `validate_unbind` 가 막는다. 충돌 상대로도 오지 않는다 — 명부의 축 항목은
            // 슬롯·다음/이전이다.
            BindingSite::AxisModifier { .. } => {}
            BindingSite::AxisSlot { axis, index } => {
                axis.set_slot(kb, *index, "");
            }
            BindingSite::AxisStep { axis, step } => axis.set_step(kb, *step, ""),
            BindingSite::ScriptBinding { script_id } => {
                kb.remove_script_binding(script_id);
            }
            BindingSite::PluginOverride {
                plugin_id,
                command_id,
                index,
            } => {
                if let Some(ov) = overrides
                    .get_mut(plugin_id)
                    .and_then(|m| m.get_mut(command_id))
                {
                    if let ShortcutOverride::Key { value: keys } = ov
                        && *index < keys.len()
                    {
                        keys.remove(*index);
                    }
                    // 키가 하나도 안 남은 override 는 "단축키 없음" 이다 — 빈 `Key` 로 두면
                    // 표시·매칭이 그 뜻을 따로 해석해야 한다.
                    if matches!(ov, ShortcutOverride::Key { value } if value.is_empty()) {
                        *ov = ShortcutOverride::None;
                    }
                }
            }
        }
    }
}

/// 대체 값이 그 자리의 종류에 맞고, 다시 `option` 을 담지 않는지.
fn validate_replacement(site: &BindingSite, value: &str) -> Result<(), MigrationError> {
    match site.replacement_kind() {
        ReplacementKind::Combo => {
            let Some(parsed) = parse_binding(value) else {
                return Err(MigrationError::NotACombo {
                    site: site.clone(),
                    value: value.to_string(),
                });
            };
            if parsed.option {
                return Err(MigrationError::ReplacementKeepsOption {
                    site: site.clone(),
                    value: value.to_string(),
                });
            }
        }
        ReplacementKind::ModifierCombo => {
            // `"individual"` sentinel 도 여기서 걸린다 — 마이그레이션으로 축의 모드를
            // 바꾸지는 않는다.
            let Some(combo) = Combo::parse_modifiers(value) else {
                return Err(MigrationError::NotAModifierCombo {
                    site: site.clone(),
                    value: value.to_string(),
                });
            };
            if combo.option {
                return Err(MigrationError::ReplacementKeepsOption {
                    site: site.clone(),
                    value: value.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn write_site(
    kb: &mut KeybindingSettings,
    overrides: &mut PluginShortcutOverrides,
    site: &BindingSite,
    value: &str,
) {
    match site {
        BindingSite::GeneralBinding { field_id, index } => {
            kb.replace_binding_at(field_id, *index, value.to_string());
        }
        BindingSite::AxisModifier { axis } => axis.set_modifier(kb, value),
        BindingSite::AxisSlot { axis, index } => {
            axis.set_slot(kb, *index, value);
        }
        BindingSite::AxisStep { axis, step } => axis.set_step(kb, *step, value),
        BindingSite::ScriptBinding { script_id } => {
            kb.set_script_binding(script_id, value.to_string());
        }
        BindingSite::PluginOverride {
            plugin_id,
            command_id,
            index,
        } => {
            if let Some(ShortcutOverride::Key { value: keys }) = overrides
                .get_mut(plugin_id)
                .and_then(|m| m.get_mut(command_id))
                && let Some(slot) = keys.get_mut(*index)
            {
                *slot = value.to_string();
            }
        }
    }
}

/// 충돌 판정의 네임스페이스.
///
/// 호스트 액션·quick-switch·스크립트는 한 네임스페이스다(`combo_conflict` 가 보는
/// 범위와 같고, quick-switch 슬롯은 축을 넘어 서로도 겹치면 안 된다).
/// plugin 은 **plugin 마다** 별도다 — 겹치는 키에서 plugin 이 호스트보다 항상 우선하고
/// (`docs/design/policies/key-mapping.md` "Plugin 커맨드 단축키 우선순위"), 어느 plugin 의
/// 명령이 후보인지는 포커스가 가른다. 그래서 호스트↔plugin 과 plugin↔plugin 은
/// 충돌이 아니라 규정된 우선순위다.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum ConflictScope {
    Host,
    Plugin(String),
}

/// 발화 콤보 명부의 한 항목 — (충돌 네임스페이스, 자리, 콤보).
type RosterEntry = (ConflictScope, BindingSite, String);

/// 지금 구성에서 **실제로 발화하는 콤보** 전량 — 규칙 기반 축은 합성 콤보로 편다.
///
/// 축 modifier 를 바꾸면 그 축의 슬롯 전부와 다음/이전의 합성 콤보가 한꺼번에 바뀌므로,
/// 충돌 검사가 값 하나가 아니라 이 목록 전체를 대상으로 돌아야 한다.
fn combo_roster(kb: &KeybindingSettings, overrides: &PluginShortcutOverrides) -> Vec<RosterEntry> {
    let mut roster = Vec::new();

    for &(field_id, _label) in KeybindingSettings::GENERAL_BINDING_FIELDS {
        let Some(bindings) = kb.get_bindings(field_id) else {
            continue;
        };
        for (index, combo) in bindings.iter().enumerate() {
            if !combo.is_empty() {
                roster.push((
                    ConflictScope::Host,
                    BindingSite::GeneralBinding { field_id, index },
                    combo.clone(),
                ));
            }
        }
    }

    for axis in SwitchAxis::ALL {
        // 개별 지정 축은 저장값이 이미 완전 콤보라 접두어가 없고, 규칙 기반 축은
        // modifier 를 dispatch 시점에 합성하므로 접두어가 그 modifier 다.
        let prefix: Option<String> = if axis.is_individual(kb) {
            None
        } else {
            match Combo::parse_modifiers(axis.modifier(kb)) {
                Some(modifier) => Some(modifier.name()),
                // 조합으로 파싱되지 않는 modifier(쓰레기 값)면 그 축은 아무것도
                // 발화하지 않으므로 명부에서 통째로 빠진다.
                None => continue,
            }
        };
        let compose = |raw: &str| -> Option<String> {
            if raw.is_empty() {
                return None;
            }
            Some(match &prefix {
                Some(p) => format!("{p}+{raw}"),
                None => raw.to_string(),
            })
        };
        for index in 0..axis.slot_count() {
            let Some(raw) = axis.slot(kb, index) else {
                continue;
            };
            if let Some(combo) = compose(raw) {
                roster.push((
                    ConflictScope::Host,
                    BindingSite::AxisSlot { axis, index },
                    combo,
                ));
            }
        }
        for step in SwitchStep::ALL {
            if let Some(combo) = compose(axis.step(kb, step)) {
                roster.push((
                    ConflictScope::Host,
                    BindingSite::AxisStep { axis, step },
                    combo,
                ));
            }
        }
    }

    for binding in &kb.script_bindings {
        if !binding.combo.is_empty() {
            roster.push((
                ConflictScope::Host,
                BindingSite::ScriptBinding {
                    script_id: binding.script_id.clone(),
                },
                binding.combo.clone(),
            ));
        }
    }

    for (plugin_id, commands) in overrides {
        for (command_id, ov) in commands {
            let ShortcutOverride::Key { value } = ov else {
                continue;
            };
            for (index, combo) in value.iter().enumerate() {
                if !combo.is_empty() {
                    roster.push((
                        ConflictScope::Plugin(plugin_id.clone()),
                        BindingSite::PluginOverride {
                            plugin_id: plugin_id.clone(),
                            command_id: command_id.clone(),
                            index,
                        },
                        combo.clone(),
                    ));
                }
            }
        }
    }

    roster
}

/// 같은 네임스페이스에서 같은 조합을 갖는 자리 쌍 → 그 조합.
///
/// 키가 **자리 쌍뿐**인 이유: 적용 전후를 차분해 "이번에 생긴 충돌" 만 거절하는데,
/// 조합 문자열을 키에 넣으면 축 modifier 변경처럼 **양쪽이 함께 움직이는** 기존 충돌이
/// 값만 바뀐 채 새 항목으로 잡힌다.
fn conflicting_pairs(
    kb: &KeybindingSettings,
    overrides: &PluginShortcutOverrides,
) -> BTreeMap<(BindingSite, BindingSite), String> {
    let roster = combo_roster(kb, overrides);
    let mut pairs = BTreeMap::new();
    for i in 0..roster.len() {
        for j in (i + 1)..roster.len() {
            let (scope_a, site_a, combo_a) = &roster[i];
            let (scope_b, site_b, combo_b) = &roster[j];
            if scope_a != scope_b || !bindings_equivalent(combo_a, combo_b) {
                continue;
            }
            let (first, second) = if site_a <= site_b {
                (site_a, site_b)
            } else {
                (site_b, site_a)
            };
            pairs.insert((first.clone(), second.clone()), combo_a.clone());
        }
    }
    pairs
}

#[cfg(test)]
#[path = "option_migration/tests.rs"]
mod tests;
