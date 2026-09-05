//! Keybindings › Tab/Workspace 서브탭의 **quick-switch 섹션** — modifier 드롭다운 +
//! 슬롯(1~N) + 다음/이전 raw 키 편집 UI (quickswitch-04).
//!
//! 일반 콤보 필드(`entries.rs`)와 달리 이 8종 필드(`tab_switch_slot_keys` 등)는
//! **modifier 없는 raw 키 하나**를 저장하고, 표시·dispatch 시점에
//! `tab_switch_modifier`/`workspace_switch_modifier`/`category_switch_modifier` 와
//! 조합된다. 따라서:
//!
//! - 저장값은 raw 키(`"q"`), 버튼 라벨은 표시 시점에 `"{Modifier}+{Key}"` 로 합성한다
//!   (modifier 드롭다운을 바꾸면 저장값 변경 없이 라벨이 자동으로 따라간다).
//! - 캡처는 [`super::capture_bare_key`](modifier 금지)로 한다.
//! - 충돌 검사는 합성 콤보를 일반 액션(`find_conflict`) + 다른 슬롯(자체 순회)과 비교한다.
//!
//! ## "개별 지정" 모드 (S-9)
//!
//! modifier 드롭다운에서 `KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER` sentinel 을
//! 고르면 그 축은 규칙 기반을 벗어난다 — 슬롯 필드의 "의미"가 갈린다:
//!
//! - 규칙 기반: 필드는 **modifier 없는 raw 키 하나**. 캡처는 [`super::capture_bare_key`].
//! - 개별 지정: 필드는 **이미 완성된 콤보 문자열**(예: `"ctrl+alt+q"`). 캡처는 일반
//!   콤보와 동일한 [`super::capture_winit_key_combo`](자유 조합). `bare_combo` 가
//!   이 모드에서는 `compose()` 를 거치지 않고 저장값을 그대로 반환한다 — 스키마를
//!   늘리지 않되 "raw 냐 완전 콤보냐"가 modifier 값에 따라 갈리는 암묵적 불변식이다.
//! - 개별 지정 축은 `switch_target_for`(`switch_overlay.rs`) 가 그 축을 절대 반환하지
//!   않으므로(sentinel 은 `Combo::parse_modifiers` 에서 파싱 실패) 탭바/사이드바의
//!   switch-number 키캡 오버레이가 그 축에서 자동으로 뜨지 않는다 — 슬롯마다 콤보가
//!   달라 통일된 숫자 힌트를 그릴 근거가 없으므로 의도된 부작용이다. 실제 디스패치는
//!   `numeric.rs` 의 개별 지정 전용 분기(`matches_binding` 슬롯 순회)가 담당한다.
//! - 모드 전환 시 슬롯 값은 [`apply_modifier_transition`] 이 이관/복원한다(규칙 기반→
//!   개별 지정은 `구 modifier+raw` 로 자동 합성, 역방향은 이 축의 기본값으로 복원 —
//!   개별 지정 콤보는 raw 로 역산 불가능하므로 버림이 유일하게 안전한 선택).

use crate::adapters::ui::input::shortcuts::modifier_hint::all_modifier_combos;
use crate::i18n::{t, t_fmt};
use crate::settings::{GeneralSettings, KeybindingSettings};
use tasty_type_geometry::length::LogicalPx;

use super::{BareTarget, FieldKind, KeyCapture, PendingBinding, RecordingSlot};
use tasty_ui_widgets::vspace;

/// 버튼/간격 치수. 4px 그리드 준수 (entries.rs 와 동일 값).
const BUTTON_HEIGHT: LogicalPx = LogicalPx(24.0);
const BUTTON_WIDTH: LogicalPx = LogicalPx(140.0);
const LABEL_GAP: LogicalPx = LogicalPx(12.0);

/// 이 섹션이 편집하는 quick-switch 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuickSwitchKind {
    Tab,
    Workspace,
    Category,
}

impl QuickSwitchKind {
    /// 이 종류의 슬롯 개수 (탭 10, 워크스페이스 9, 카테고리 10).
    fn slot_count(self) -> usize {
        match self {
            QuickSwitchKind::Tab => 10,
            QuickSwitchKind::Workspace => 9,
            QuickSwitchKind::Category => 10,
        }
    }

    fn slot_target(self, idx: usize) -> BareTarget {
        match self {
            QuickSwitchKind::Tab => BareTarget::TabSlot(idx),
            QuickSwitchKind::Workspace => BareTarget::WorkspaceSlot(idx),
            QuickSwitchKind::Category => BareTarget::CategorySlot(idx),
        }
    }

    /// 다음/이전 raw 키 타겟.
    fn next_target(self) -> Option<BareTarget> {
        match self {
            QuickSwitchKind::Tab => Some(BareTarget::TabNext),
            QuickSwitchKind::Workspace => Some(BareTarget::WorkspaceNext),
            QuickSwitchKind::Category => Some(BareTarget::CategoryNext),
        }
    }

    fn prev_target(self) -> Option<BareTarget> {
        match self {
            QuickSwitchKind::Tab => Some(BareTarget::TabPrev),
            QuickSwitchKind::Workspace => Some(BareTarget::WorkspacePrev),
            QuickSwitchKind::Category => Some(BareTarget::CategoryPrev),
        }
    }

    fn modifier_label_key(self) -> &'static str {
        match self {
            QuickSwitchKind::Tab => "settings.keybindings.tab_switch_modifier_label",
            QuickSwitchKind::Workspace => "settings.keybindings.workspace_switch_modifier_label",
            QuickSwitchKind::Category => "settings.keybindings.category_switch_modifier_label",
        }
    }

    fn modifier_salt(self) -> &'static str {
        match self {
            QuickSwitchKind::Tab => "tab_switch_modifier",
            QuickSwitchKind::Workspace => "workspace_switch_modifier",
            QuickSwitchKind::Category => "category_switch_modifier",
        }
    }

    /// 이 축의 슬롯 + 다음/이전 타겟 전체(순서: 슬롯 1~N → 다음 → 이전).
    fn all_targets(self) -> Vec<BareTarget> {
        let mut targets: Vec<BareTarget> = (0..self.slot_count())
            .map(|i| self.slot_target(i))
            .collect();
        targets.extend(self.next_target());
        targets.extend(self.prev_target());
        targets
    }
}

/// `target` 이 속한 quick-switch 축.
fn axis_of(target: BareTarget) -> QuickSwitchKind {
    match target {
        BareTarget::TabSlot(_) | BareTarget::TabNext | BareTarget::TabPrev => QuickSwitchKind::Tab,
        BareTarget::WorkspaceSlot(_) | BareTarget::WorkspaceNext | BareTarget::WorkspacePrev => {
            QuickSwitchKind::Workspace
        }
        BareTarget::CategorySlot(_) | BareTarget::CategoryNext | BareTarget::CategoryPrev => {
            QuickSwitchKind::Category
        }
    }
}

/// `kind` 축의 현재 modifier 필드 값.
fn modifier_value(kb: &KeybindingSettings, kind: QuickSwitchKind) -> &str {
    match kind {
        QuickSwitchKind::Tab => &kb.tab_switch_modifier,
        QuickSwitchKind::Workspace => &kb.workspace_switch_modifier,
        QuickSwitchKind::Category => &kb.category_switch_modifier,
    }
}

/// `kind` 축이 현재 "개별 지정" 모드인지.
fn is_individual_axis(kb: &KeybindingSettings, kind: QuickSwitchKind) -> bool {
    modifier_value(kb, kind) == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER
}

// ── BareTarget 데이터 접근 (accessor 경유) ────────────────────────────────

/// `target` 슬롯의 현재 raw 키를 읽는다.
fn bare_key_value(kb: &KeybindingSettings, target: BareTarget) -> String {
    match target {
        BareTarget::TabSlot(i) => kb.tab_slot_key(i).unwrap_or("").to_string(),
        BareTarget::WorkspaceSlot(i) => kb.workspace_slot_key(i).unwrap_or("").to_string(),
        BareTarget::CategorySlot(i) => kb.category_slot_key(i).unwrap_or("").to_string(),
        BareTarget::TabNext => kb.tab_next_key().to_string(),
        BareTarget::TabPrev => kb.tab_prev_key().to_string(),
        BareTarget::WorkspaceNext => kb.workspace_next_key().to_string(),
        BareTarget::WorkspacePrev => kb.workspace_prev_key().to_string(),
        BareTarget::CategoryNext => kb.category_next_key().to_string(),
        BareTarget::CategoryPrev => kb.category_prev_key().to_string(),
    }
}

/// `target` 슬롯에 raw 키를 기록한다. 충돌 팝업 accept 경로(ui.rs)에서도 사용.
pub fn set_bare_target(kb: &mut KeybindingSettings, target: BareTarget, raw_key: &str) {
    match target {
        BareTarget::TabSlot(i) => {
            kb.set_tab_slot_key(i, raw_key);
        }
        BareTarget::WorkspaceSlot(i) => {
            kb.set_workspace_slot_key(i, raw_key);
        }
        BareTarget::CategorySlot(i) => {
            kb.set_category_slot_key(i, raw_key);
        }
        BareTarget::TabNext => kb.set_tab_next_key(raw_key),
        BareTarget::TabPrev => kb.set_tab_prev_key(raw_key),
        BareTarget::WorkspaceNext => kb.set_workspace_next_key(raw_key),
        BareTarget::WorkspacePrev => kb.set_workspace_prev_key(raw_key),
        BareTarget::CategoryNext => kb.set_category_next_key(raw_key),
        BareTarget::CategoryPrev => kb.set_category_prev_key(raw_key),
    }
}

/// `target` 슬롯을 비운다(빈 문자열). 슬롯 간 충돌 accept 시 상대 슬롯 클리어에 사용.
pub fn clear_bare_target(kb: &mut KeybindingSettings, target: BareTarget) {
    set_bare_target(kb, target, "");
}

/// `target` 이 조합에 쓰는 modifier 조합(`"ctrl"`/`"alt"`/`"ctrl+shift"` …).
fn bare_modifier(kb: &KeybindingSettings, target: BareTarget) -> &str {
    match target {
        BareTarget::TabSlot(_) | BareTarget::TabNext | BareTarget::TabPrev => {
            &kb.tab_switch_modifier
        }
        BareTarget::WorkspaceSlot(_) | BareTarget::WorkspaceNext | BareTarget::WorkspacePrev => {
            &kb.workspace_switch_modifier
        }
        BareTarget::CategorySlot(_) | BareTarget::CategoryNext | BareTarget::CategoryPrev => {
            &kb.category_switch_modifier
        }
    }
}

/// modifier + raw 키를 합성한 콤보 문자열. raw 가 비면 빈 문자열(= 미설정).
fn compose(modifier: &str, raw: &str) -> String {
    if raw.is_empty() {
        String::new()
    } else {
        format!("{modifier}+{raw}")
    }
}

/// `target` 의 최종 콤보. 규칙 기반 축이면 `compose(modifier, raw)`, 개별 지정 축이면
/// 슬롯 필드에 이미 저장된 완전 콤보를 그대로 반환한다(compose 하지 않음).
fn bare_combo(kb: &KeybindingSettings, target: BareTarget) -> String {
    if is_individual_axis(kb, axis_of(target)) {
        bare_key_value(kb, target)
    } else {
        compose(bare_modifier(kb, target), &bare_key_value(kb, target))
    }
}

/// 팝업/경고에 표시할 `target` 라벨(콜론·공백 정리됨).
fn bare_display_label(target: BareTarget) -> String {
    let raw = match target {
        BareTarget::TabSlot(i) => t_fmt(
            "settings.keybindings.tab_switch_slot_label",
            &(i + 1).to_string(),
        ),
        BareTarget::WorkspaceSlot(i) => t_fmt(
            "settings.keybindings.workspace_switch_slot_label",
            &(i + 1).to_string(),
        ),
        BareTarget::CategorySlot(i) => t_fmt(
            "settings.keybindings.category_switch_slot_label",
            &(i + 1).to_string(),
        ),
        BareTarget::TabNext => t("settings.keybindings.tab_switch_next_label").to_string(),
        BareTarget::TabPrev => t("settings.keybindings.tab_switch_prev_label").to_string(),
        BareTarget::WorkspaceNext => {
            t("settings.keybindings.workspace_switch_next_label").to_string()
        }
        BareTarget::WorkspacePrev => {
            t("settings.keybindings.workspace_switch_prev_label").to_string()
        }
        BareTarget::CategoryNext => {
            t("settings.keybindings.category_switch_next_label").to_string()
        }
        BareTarget::CategoryPrev => {
            t("settings.keybindings.category_switch_prev_label").to_string()
        }
    };
    raw.trim_end_matches(':').trim().to_string()
}

/// 모든 quick-switch bare 타겟 목록(슬롯 간 중복 검사용 — 탭·워크스페이스·카테고리 교차 포함).
fn all_bare_targets() -> Vec<BareTarget> {
    [
        QuickSwitchKind::Tab,
        QuickSwitchKind::Workspace,
        QuickSwitchKind::Category,
    ]
    .into_iter()
    .flat_map(QuickSwitchKind::all_targets)
    .collect()
}

/// `target` 에 `candidate_combo`(이미 합성 완료된 최종 콤보 — 규칙 기반이든 개별
/// 지정이든 호출측이 [`bare_combo`] 와 동일 규칙으로 만들어 넘긴다)를 넣었을 때 겹치는
/// **다른 슬롯**을 찾는다. 탭/워크스페이스/카테고리 축이 우연히 같은 콤보를 가지면
/// 교차 충돌도 잡힌다.
fn find_slot_conflict(
    kb: &KeybindingSettings,
    target: BareTarget,
    candidate_combo: &str,
) -> Option<BareTarget> {
    if candidate_combo.is_empty() {
        return None;
    }
    all_bare_targets().into_iter().find(|&other| {
        other != target && {
            let oc = bare_combo(kb, other);
            !oc.is_empty() && oc == candidate_combo
        }
    })
}

// ── 렌더 ──────────────────────────────────────────────────────────────────

pub(super) fn draw_quick_switch_section(
    ui: &mut egui::Ui,
    keybindings: &mut KeybindingSettings,
    general: &GeneralSettings,
    recording_field: &mut Option<RecordingSlot>,
    pending_binding: &mut Option<PendingBinding>,
    captured: &KeyCapture,
    kind: QuickSwitchKind,
) {
    // 녹화된 bare 키 소비 — 이 섹션 소속 BareKey 슬롯만.
    consume_capture(keybindings, recording_field, pending_binding, captured);

    let th = crate::theme::theme();
    // 충돌 팝업이 떠 있는 동안은 새 녹화 진입 금지.
    let can_record = pending_binding.is_none();

    // 전환 감지용 — Grid 클로저가 modifier 필드를 직접 mutate 하므로, 그 전/후 값을
    // 비교해 실제로 바뀐 경우에만 슬롯 이관/복원(apply_modifier_transition)을 수행한다.
    let old_modifier = modifier_value(keybindings, kind).to_string();

    // modifier 드롭다운 (기존 blocks 이관).
    egui::Grid::new(format!("{}_modifier_grid", kind.modifier_salt()))
        .num_columns(2)
        .spacing([LABEL_GAP.value(), 8.0])
        .show(ui, |ui| {
            ui.label(t(kind.modifier_label_key()));
            let modifier = match kind {
                QuickSwitchKind::Tab => &mut keybindings.tab_switch_modifier,
                QuickSwitchKind::Workspace => &mut keybindings.workspace_switch_modifier,
                QuickSwitchKind::Category => &mut keybindings.category_switch_modifier,
            };
            let is_individual = modifier.as_str() == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER;
            let selected_text = if is_individual {
                t("settings.keybindings.quick_switch_individual_label").to_string()
            } else {
                KeybindingSettings::format_display(modifier, general)
            };
            // OS-aware 허용 조합만 열거(쓰레기 값 원천 차단, decision 1). macOS 는 option
            // 축 포함, 그 외 제외 — modifier_hint 의 조합 열거를 단일 소스로 재사용한다.
            // "개별 지정" sentinel 은 이 열거와 별도로 마지막에 추가한다(규칙 기반
            // 조합이 아니므로 all_modifier_combos() 목록에 섞이지 않음).
            egui::ComboBox::from_id_salt(kind.modifier_salt())
                .selected_text(selected_text)
                .show_ui(ui, |ui| {
                    for combo in all_modifier_combos() {
                        let name = combo.name();
                        let display = KeybindingSettings::format_display(&name, general);
                        ui.selectable_value(modifier, name, display);
                    }
                    ui.selectable_value(
                        modifier,
                        KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER.to_string(),
                        t("settings.keybindings.quick_switch_individual_label"),
                    );
                });
            ui.end_row();
        });

    // modifier 가 실제로 바뀌었으면 슬롯 값을 이관(규칙→개별)하거나 복원(개별→규칙)한다.
    let new_modifier = modifier_value(keybindings, kind).to_string();
    if new_modifier != old_modifier {
        apply_modifier_transition(keybindings, kind, &old_modifier, &new_modifier);
    }

    vspace(ui, th.spacing_xs);

    let is_individual = is_individual_axis(keybindings, kind);

    // 슬롯 1~N.
    for i in 0..kind.slot_count() {
        slot_row(
            ui,
            keybindings,
            general,
            recording_field,
            can_record,
            kind.slot_target(i),
            is_individual,
        );
    }
    // 다음/이전 (세 축 모두 존재 — 카테고리도 대칭).
    for tg in [kind.next_target(), kind.prev_target()]
        .into_iter()
        .flatten()
    {
        slot_row(
            ui,
            keybindings,
            general,
            recording_field,
            can_record,
            tg,
            is_individual,
        );
    }

    // modifier 변경 등으로 현재 합성 콤보가 다른 액션과 충돌하면 조용히 넘기지 않고 경고.
    let conflicts = current_conflicts(keybindings, kind);
    if !conflicts.is_empty() {
        vspace(ui, th.spacing_xs);
        ui.label(
            egui::RichText::new(t_fmt(
                "settings.keybindings.quick_switch_conflict_warning",
                &conflicts.join(", "),
            ))
            .small()
            .color(th.accent_warning()),
        );
    }
}

/// 이 섹션 슬롯들의 현재 합성 콤보가 **일반 액션**과 충돌하는 목록을 라벨로 반환.
fn current_conflicts(kb: &KeybindingSettings, kind: QuickSwitchKind) -> Vec<String> {
    let mut out = Vec::new();
    for tg in kind.all_targets() {
        let combo = bare_combo(kb, tg);
        if combo.is_empty() {
            continue;
        }
        if kb.find_conflict("", &combo).is_some() {
            out.push(bare_display_label(tg));
        }
    }
    out
}

/// modifier 드롭다운 전환 시 슬롯 값을 이관/복원한다(S-9 분석검증 Q2/Q3 확정값).
///
/// - **규칙 기반 → 개별 지정**: 각 슬롯의 현재 합성 콤보(`구 modifier + raw`)를 그대로
///   슬롯 필드에 저장한다 — `bare_combo` 가 이후 이 값을 compose 없이 그대로 반환하므로
///   전환 직후 사용자 체감 동작이 100% 유지된다. `raw` 를 modifier 없이 그대로
///   재해석(옵션 a)하면 `capture_winit_key_combo` 의 "modifier 없는 타이핑 키는 단축키
///   등록 불가" 가드가 막으려던 상태를 우회 생성하게 되므로 채택하지 않는다.
/// - **개별 지정 → 규칙 기반**: 개별 지정 콤보 문자열(예: `"ctrl+alt+1"`)은 어느 부분이
///   modifier 였는지 구조적으로 유실돼 있어 raw 로 역산이 불가능하다. 이 축을
///   기본값으로 복원하는 것이 유일하게 안전한 선택이다.
/// - **규칙 기반 → 다른 규칙 기반**(예: ctrl→alt): 슬롯 raw 값은 그대로 두고 표시만
///   새 modifier 로 자동 재합성된다(기존 동작, 변경 없음).
fn apply_modifier_transition(
    kb: &mut KeybindingSettings,
    kind: QuickSwitchKind,
    old_modifier: &str,
    new_modifier: &str,
) {
    let was_individual = old_modifier == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER;
    let becomes_individual = new_modifier == KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER;
    if !was_individual && becomes_individual {
        for target in kind.all_targets() {
            let migrated = compose(old_modifier, &bare_key_value(kb, target));
            set_bare_target(kb, target, &migrated);
        }
    } else if was_individual && !becomes_individual {
        match kind {
            QuickSwitchKind::Tab => kb.reset_tab_switch_to_defaults(),
            QuickSwitchKind::Workspace => kb.reset_workspace_switch_to_defaults(),
            QuickSwitchKind::Category => kb.reset_category_switch_to_defaults(),
        }
    }
}

/// 슬롯 한 줄: 라벨 + 현재 값 버튼(클릭 시 녹화 진입). `is_individual` 이면
/// [`FieldKind::IndividualSlot`](modifier 포함 자유 콤보), 아니면 [`FieldKind::BareKey`]
/// (modifier 금지 raw 키 하나)로 녹화한다 — 이 축의 modifier 값에 따라 정해진다.
fn slot_row(
    ui: &mut egui::Ui,
    keybindings: &KeybindingSettings,
    general: &GeneralSettings,
    recording_field: &mut Option<RecordingSlot>,
    can_record: bool,
    target: BareTarget,
    is_individual: bool,
) {
    let th = crate::theme::theme();
    let field_kind = if is_individual {
        FieldKind::IndividualSlot(target)
    } else {
        FieldKind::BareKey(target)
    };
    let is_recording = matches!(
        recording_field,
        Some(slot) if slot.field_kind == field_kind
    );

    ui.horizontal_top(|ui| {
        // 라벨 컬럼: 서브탭 공유 고정 폭(`super::LABEL_COL_WIDTH`), 좌측 정렬(entries.rs 와 동일 관례).
        ui.allocate_ui_with_layout(
            egui::vec2(super::LABEL_COL_WIDTH.value(), BUTTON_HEIGHT.value()),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.label(format!("{}:", bare_display_label(target)));
            },
        );
        ui.add_space(LABEL_GAP.value());

        let combo = bare_combo(keybindings, target);
        let display = if is_recording {
            let hint_key = if is_individual {
                "settings.keybindings.hint_press_key"
            } else {
                "settings.keybindings.hint_press_bare_key"
            };
            t(hint_key).to_string()
        } else if combo.is_empty() {
            t("settings.keybindings.hint_none").to_string()
        } else {
            KeybindingSettings::format_display(&combo, general)
        };
        let bg = if is_recording {
            th.surface_hover()
        } else {
            th.surface_raised()
        };
        let fg = if is_recording {
            th.text_disabled()
        } else if combo.is_empty() {
            th.text_muted()
        } else {
            th.text_primary()
        };
        let btn = egui::Button::new(egui::RichText::new(&display).color(fg).monospace())
            .fill(bg)
            .min_size(egui::vec2(BUTTON_WIDTH.value(), BUTTON_HEIGHT.value()));
        if ui.add_enabled(can_record, btn).clicked() {
            *recording_field = Some(RecordingSlot {
                field_id: String::new(),
                idx: 0,
                field_kind,
            });
        }
    });
    // 행 간격은 `Theme.spacing_xs` 에서 읽는다. 이 행들을 쌓는 섹션이 이미
    // `vspace(ui, th.spacing_xs)` 로 배율을 타므로, 여기만 평상수면 1.2 에서
    // 같은 4 가 5 와 4 로 갈린다.
    ui.add_space(th.spacing_xs.value());
}

/// 녹화된 키(bare 또는 개별 지정 콤보)를 소비해 슬롯에 반영. 충돌 시 기존
/// `PendingBinding` 팝업 흐름 재사용.
///
/// `bare_raw_key`/`set_bare_target`/`clear_bare_target` 는 이름이 "raw 키" 지만 실제로는
/// "이 슬롯 필드에 그대로 저장할 최종 문자열" 을 나르는 모드 무관 통로다 — 규칙 기반은
/// raw 한 글자, 개별 지정은 이미 완성된 콤보. 저장 시점엔 이 함수가 이미 모드별로 올바른
/// 값(`combo`)을 만들어 넘기므로 accessor 자체는 손댈 필요가 없다.
fn consume_capture(
    keybindings: &mut KeybindingSettings,
    recording_field: &mut Option<RecordingSlot>,
    pending_binding: &mut Option<PendingBinding>,
    captured: &KeyCapture,
) {
    let Some(slot) = recording_field.clone() else {
        return;
    };
    let (target, is_individual) = match slot.field_kind {
        FieldKind::BareKey(target) => (target, false),
        FieldKind::IndividualSlot(target) => (target, true),
        FieldKind::Combo => return,
    };

    match captured {
        KeyCapture::Combo(raw) => {
            // 개별 지정: `raw` 는 이미 완성된 콤보(capture_winit_key_combo 산출) — 그대로
            // 쓴다. 규칙 기반: `raw` 는 modifier 없는 키 하나 — compose 로 합성한다.
            let combo = if is_individual {
                raw.clone()
            } else {
                compose(bare_modifier(keybindings, target), raw)
            };
            if let Some((cf, ci)) = keybindings.find_conflict("", &combo) {
                // 일반 액션과 충돌.
                let label = KeybindingSettings::label_key_for(cf)
                    .map(t)
                    .unwrap_or(cf)
                    .trim_end_matches(':')
                    .trim()
                    .to_string();
                *pending_binding = Some(PendingBinding {
                    target_field: String::new(),
                    target_idx: 0,
                    combo: combo.clone(),
                    conflicting_field: cf.to_string(),
                    conflicting_idx: ci,
                    bare_target: Some(target),
                    bare_raw_key: combo,
                    conflicting_bare: None,
                    conflicting_label: Some(label),
                });
            } else if let Some(other) = find_slot_conflict(keybindings, target, &combo) {
                // 다른 quick-switch 슬롯과 중복.
                *pending_binding = Some(PendingBinding {
                    target_field: String::new(),
                    target_idx: 0,
                    combo: combo.clone(),
                    conflicting_field: String::new(),
                    conflicting_idx: 0,
                    bare_target: Some(target),
                    bare_raw_key: combo,
                    conflicting_bare: Some(other),
                    conflicting_label: Some(bare_display_label(other)),
                });
            } else {
                // `raw` 를 그대로 저장 — 개별 지정이면 `raw == combo`(이미 완전 콤보),
                // 규칙 기반이면 `raw` 는 modifier 없는 키 하나(합성은 표시 시점 몫).
                set_bare_target(keybindings, target, raw);
            }
            *recording_field = None;
        }
        KeyCapture::Clear => {
            // Escape — 슬롯 비우기.
            clear_bare_target(keybindings, target);
            *recording_field = None;
        }
        KeyCapture::None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INDIVIDUAL: &str = KeybindingSettings::INDIVIDUAL_SWITCH_MODIFIER;

    #[test]
    fn bare_combo_rule_based_composes_modifier() {
        let kb = KeybindingSettings::preset_tasty(); // tab modifier = "ctrl"
        assert_eq!(bare_combo(&kb, BareTarget::TabSlot(0)), "ctrl+1");
    }

    #[test]
    fn bare_combo_individual_mode_returns_stored_value_as_is() {
        let mut kb = KeybindingSettings::preset_tasty();
        kb.tab_switch_modifier = INDIVIDUAL.to_string();
        kb.set_tab_slot_key(0, "ctrl+alt+q"); // 이미 완성된 콤보를 그대로 저장.
        // compose 를 거치지 않고 저장값을 그대로 반환해야 한다(이중 합성 금지).
        assert_eq!(bare_combo(&kb, BareTarget::TabSlot(0)), "ctrl+alt+q");
    }

    #[test]
    fn apply_modifier_transition_rule_to_individual_migrates_slots() {
        let mut kb = KeybindingSettings::preset_tasty(); // tab modifier = "ctrl", slot0 = "1"
        apply_modifier_transition(&mut kb, QuickSwitchKind::Tab, "ctrl", INDIVIDUAL);
        // 슬롯 필드가 "구 modifier+raw" 로 완전 합성돼 그대로 남는다(Q2 확정).
        assert_eq!(kb.tab_slot_key(0), Some("ctrl+1"));
        assert_eq!(kb.tab_next_key(), "ctrl+l");
        assert_eq!(kb.tab_prev_key(), "ctrl+h");
        // 이후 bare_combo 는 이 값을 그대로 반환(이중 합성 없음).
        kb.tab_switch_modifier = INDIVIDUAL.to_string();
        assert_eq!(bare_combo(&kb, BareTarget::TabSlot(0)), "ctrl+1");
    }

    #[test]
    fn apply_modifier_transition_individual_to_rule_based_resets_to_defaults() {
        let mut kb = KeybindingSettings::preset_tasty();
        kb.tab_switch_modifier = INDIVIDUAL.to_string();
        kb.set_tab_slot_key(0, "ctrl+alt+q"); // 개별 지정 완전 콤보(역산 불가능한 값).
        kb.set_tab_next_key("shift+f5");
        apply_modifier_transition(&mut kb, QuickSwitchKind::Tab, INDIVIDUAL, "alt");
        // 역산 대신 이 축의 기본값으로 복원(Q3 확정).
        assert_eq!(
            kb.tab_switch_slot_keys,
            ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"]
        );
        assert_eq!(kb.tab_next_key(), "l");
        assert_eq!(kb.tab_prev_key(), "h");
    }

    #[test]
    fn apply_modifier_transition_rule_to_rule_leaves_slots_unchanged() {
        let mut kb = KeybindingSettings::preset_tasty();
        kb.set_tab_slot_key(0, "q");
        apply_modifier_transition(&mut kb, QuickSwitchKind::Tab, "ctrl", "alt");
        // raw 값은 그대로 — 표시만 새 modifier 로 재합성(호출측 bare_combo 책임).
        assert_eq!(kb.tab_slot_key(0), Some("q"));
    }

    #[test]
    fn apply_modifier_transition_only_touches_target_axis() {
        let mut kb = KeybindingSettings::preset_tasty();
        apply_modifier_transition(&mut kb, QuickSwitchKind::Tab, "ctrl", INDIVIDUAL);
        // 워크스페이스/카테고리 축은 여전히 규칙 기반 raw 값 그대로.
        assert_eq!(kb.workspace_slot_key(0), Some("1"));
        assert_eq!(kb.category_slot_key(0), Some("1"));
    }

    #[test]
    fn find_slot_conflict_detects_cross_axis_duplicate_in_individual_mode() {
        let mut kb = KeybindingSettings::preset_tasty();
        kb.tab_switch_modifier = INDIVIDUAL.to_string();
        kb.set_tab_slot_key(1, "ctrl+alt+q");
        // 워크스페이스 축(규칙 기반, modifier=alt)의 슬롯을 우연히 같은 완전 콤보로
        // 만들면(alt+... 가 아니라 그 자체로 "ctrl+alt+q" 처럼 저장될 일은 없지만, 여기선
        // 교차 축 충돌 탐지 자체를 검증하기 위해 워크스페이스도 개별 지정으로 바꾼다).
        kb.workspace_switch_modifier = INDIVIDUAL.to_string();
        kb.set_workspace_slot_key(0, "ctrl+alt+q");
        let conflict = find_slot_conflict(&kb, BareTarget::TabSlot(1), "ctrl+alt+q");
        assert_eq!(conflict, Some(BareTarget::WorkspaceSlot(0)));
    }

    #[test]
    fn find_slot_conflict_empty_combo_is_never_a_conflict() {
        let kb = KeybindingSettings::preset_tasty();
        assert_eq!(find_slot_conflict(&kb, BareTarget::TabSlot(0), ""), None);
    }

    #[test]
    fn consume_capture_individual_slot_stores_full_combo_verbatim() {
        let mut kb = KeybindingSettings::preset_tasty();
        kb.tab_switch_modifier = INDIVIDUAL.to_string();
        let mut recording = Some(RecordingSlot {
            field_id: String::new(),
            idx: 0,
            field_kind: FieldKind::IndividualSlot(BareTarget::TabSlot(0)),
        });
        let mut pending = None;
        let captured = KeyCapture::Combo("ctrl+alt+q".to_string());
        consume_capture(&mut kb, &mut recording, &mut pending, &captured);
        assert_eq!(kb.tab_slot_key(0), Some("ctrl+alt+q"));
        assert!(recording.is_none());
        assert!(pending.is_none());
    }

    #[test]
    fn consume_capture_individual_slot_conflicts_with_general_action() {
        // preset_tasty 의 restore_closed = "ctrl+shift+t" — 개별 지정 슬롯에 같은 콤보를
        // 녹화하면 일반 액션 충돌 팝업(PendingBinding)이 뜨고 즉시 저장되지 않아야 한다.
        let mut kb = KeybindingSettings::preset_tasty();
        kb.tab_switch_modifier = INDIVIDUAL.to_string();
        let mut recording = Some(RecordingSlot {
            field_id: String::new(),
            idx: 0,
            field_kind: FieldKind::IndividualSlot(BareTarget::TabSlot(0)),
        });
        let mut pending = None;
        let captured = KeyCapture::Combo("ctrl+shift+t".to_string());
        consume_capture(&mut kb, &mut recording, &mut pending, &captured);
        // 충돌 발견 → 슬롯엔 아직 반영 안 됨, pending 팝업 상태로 전환.
        assert_eq!(kb.tab_slot_key(0), Some("1")); // preset 기본값 그대로.
        assert!(recording.is_none()); // 녹화는 종료(팝업으로 전환).
        let pending = pending.expect("일반 액션 충돌 시 PendingBinding 이 채워져야 함");
        assert_eq!(pending.conflicting_field, "restore_closed");
        assert_eq!(pending.bare_target, Some(BareTarget::TabSlot(0)));
        assert_eq!(pending.bare_raw_key, "ctrl+shift+t");
    }

    #[test]
    fn consume_capture_bare_slot_still_composes_via_modifier() {
        // 회귀 방지: IndividualSlot 분기 추가가 기존 BareKey 경로를 깨지 않아야 한다.
        let mut kb = KeybindingSettings::preset_tasty(); // tab modifier = "ctrl"
        let mut recording = Some(RecordingSlot {
            field_id: String::new(),
            idx: 0,
            field_kind: FieldKind::BareKey(BareTarget::TabSlot(0)),
        });
        let mut pending = None;
        let captured = KeyCapture::Combo("q".to_string());
        consume_capture(&mut kb, &mut recording, &mut pending, &captured);
        // 저장은 raw 그대로("q"), 합성은 표시 시점(bare_combo)의 몫.
        assert_eq!(kb.tab_slot_key(0), Some("q"));
        assert_eq!(bare_combo(&kb, BareTarget::TabSlot(0)), "ctrl+q");
    }

    #[test]
    fn axis_of_maps_every_variant_correctly() {
        assert_eq!(axis_of(BareTarget::TabSlot(0)), QuickSwitchKind::Tab);
        assert_eq!(axis_of(BareTarget::TabNext), QuickSwitchKind::Tab);
        assert_eq!(axis_of(BareTarget::TabPrev), QuickSwitchKind::Tab);
        assert_eq!(
            axis_of(BareTarget::WorkspaceSlot(0)),
            QuickSwitchKind::Workspace
        );
        assert_eq!(
            axis_of(BareTarget::WorkspaceNext),
            QuickSwitchKind::Workspace
        );
        assert_eq!(
            axis_of(BareTarget::WorkspacePrev),
            QuickSwitchKind::Workspace
        );
        assert_eq!(
            axis_of(BareTarget::CategorySlot(0)),
            QuickSwitchKind::Category
        );
        assert_eq!(axis_of(BareTarget::CategoryNext), QuickSwitchKind::Category);
        assert_eq!(axis_of(BareTarget::CategoryPrev), QuickSwitchKind::Category);
    }
}
