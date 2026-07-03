//! Keybindings › Tab/Workspace 서브탭의 **quick-switch 섹션** — modifier 드롭다운 +
//! 슬롯(1~N) + 다음/이전 raw 키 편집 UI (quickswitch-04).
//!
//! 일반 콤보 필드(`entries.rs`)와 달리 이 6종 필드(`tab_switch_slot_keys` 등)는
//! **modifier 없는 raw 키 하나**를 저장하고, 표시·dispatch 시점에
//! `tab_switch_modifier`/`workspace_switch_modifier` 와 조합된다. 따라서:
//!
//! - 저장값은 raw 키(`"q"`), 버튼 라벨은 표시 시점에 `"{Modifier}+{Key}"` 로 합성한다
//!   (modifier 드롭다운을 바꾸면 저장값 변경 없이 라벨이 자동으로 따라간다).
//! - 캡처는 [`super::capture_bare_key`](modifier 금지)로 한다.
//! - 충돌 검사는 합성 콤보를 일반 액션(`find_conflict`) + 다른 슬롯(자체 순회)과 비교한다.

use crate::i18n::{t, t_fmt};
use crate::settings::KeybindingSettings;

use super::{BareTarget, FieldKind, KeyCapture, PendingBinding, RecordingSlot};
use tasty_ui_widgets::vspace;

/// 버튼/간격 치수. 4px 그리드 준수 (entries.rs 와 동일 값).
const BUTTON_HEIGHT: f32 = 24.0;
const BUTTON_WIDTH: f32 = 140.0;
const LABEL_GAP: f32 = 12.0;
const ROW_GAP: f32 = 4.0;

/// 이 섹션이 편집하는 quick-switch 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QuickSwitchKind {
    Tab,
    Workspace,
}

impl QuickSwitchKind {
    /// 이 종류의 슬롯 개수 (탭 10, 워크스페이스 9).
    fn slot_count(self) -> usize {
        match self {
            QuickSwitchKind::Tab => 10,
            QuickSwitchKind::Workspace => 9,
        }
    }

    fn slot_target(self, idx: usize) -> BareTarget {
        match self {
            QuickSwitchKind::Tab => BareTarget::TabSlot(idx),
            QuickSwitchKind::Workspace => BareTarget::WorkspaceSlot(idx),
        }
    }

    fn next_target(self) -> BareTarget {
        match self {
            QuickSwitchKind::Tab => BareTarget::TabNext,
            QuickSwitchKind::Workspace => BareTarget::WorkspaceNext,
        }
    }

    fn prev_target(self) -> BareTarget {
        match self {
            QuickSwitchKind::Tab => BareTarget::TabPrev,
            QuickSwitchKind::Workspace => BareTarget::WorkspacePrev,
        }
    }

    fn modifier_label_key(self) -> &'static str {
        match self {
            QuickSwitchKind::Tab => "settings.keybindings.tab_switch_modifier_label",
            QuickSwitchKind::Workspace => "settings.keybindings.workspace_switch_modifier_label",
        }
    }

    fn modifier_salt(self) -> &'static str {
        match self {
            QuickSwitchKind::Tab => "tab_switch_modifier",
            QuickSwitchKind::Workspace => "workspace_switch_modifier",
        }
    }
}

// ── BareTarget 데이터 접근 (accessor 경유) ────────────────────────────────

/// `target` 슬롯의 현재 raw 키를 읽는다.
fn bare_key_value(kb: &KeybindingSettings, target: BareTarget) -> String {
    match target {
        BareTarget::TabSlot(i) => kb.tab_slot_key(i).unwrap_or("").to_string(),
        BareTarget::WorkspaceSlot(i) => kb.workspace_slot_key(i).unwrap_or("").to_string(),
        BareTarget::TabNext => kb.tab_next_key().to_string(),
        BareTarget::TabPrev => kb.tab_prev_key().to_string(),
        BareTarget::WorkspaceNext => kb.workspace_next_key().to_string(),
        BareTarget::WorkspacePrev => kb.workspace_prev_key().to_string(),
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
        BareTarget::TabNext => kb.set_tab_next_key(raw_key),
        BareTarget::TabPrev => kb.set_tab_prev_key(raw_key),
        BareTarget::WorkspaceNext => kb.set_workspace_next_key(raw_key),
        BareTarget::WorkspacePrev => kb.set_workspace_prev_key(raw_key),
    }
}

/// `target` 슬롯을 비운다(빈 문자열). 슬롯 간 충돌 accept 시 상대 슬롯 클리어에 사용.
pub fn clear_bare_target(kb: &mut KeybindingSettings, target: BareTarget) {
    set_bare_target(kb, target, "");
}

/// `target` 이 조합에 쓰는 modifier(`"ctrl"`/`"alt"`).
fn bare_modifier(kb: &KeybindingSettings, target: BareTarget) -> &str {
    match target {
        BareTarget::TabSlot(_) | BareTarget::TabNext | BareTarget::TabPrev => {
            &kb.tab_switch_modifier
        }
        BareTarget::WorkspaceSlot(_) | BareTarget::WorkspaceNext | BareTarget::WorkspacePrev => {
            &kb.workspace_switch_modifier
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

/// `target` 의 (raw 키가 있을 때의) 최종 합성 콤보.
fn bare_combo(kb: &KeybindingSettings, target: BareTarget) -> String {
    compose(bare_modifier(kb, target), &bare_key_value(kb, target))
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
        BareTarget::TabNext => t("settings.keybindings.tab_switch_next_label").to_string(),
        BareTarget::TabPrev => t("settings.keybindings.tab_switch_prev_label").to_string(),
        BareTarget::WorkspaceNext => {
            t("settings.keybindings.workspace_switch_next_label").to_string()
        }
        BareTarget::WorkspacePrev => {
            t("settings.keybindings.workspace_switch_prev_label").to_string()
        }
    };
    raw.trim_end_matches(':').trim().to_string()
}

/// 모든 quick-switch bare 타겟 목록(슬롯 간 중복 검사용 — 탭·워크스페이스 교차 포함).
fn all_bare_targets() -> Vec<BareTarget> {
    let mut v = Vec::with_capacity(23);
    for i in 0..10 {
        v.push(BareTarget::TabSlot(i));
    }
    for i in 0..9 {
        v.push(BareTarget::WorkspaceSlot(i));
    }
    v.push(BareTarget::TabNext);
    v.push(BareTarget::TabPrev);
    v.push(BareTarget::WorkspaceNext);
    v.push(BareTarget::WorkspacePrev);
    v
}

/// `target` 에 `new_raw` 를 넣었을 때 합성 콤보가 겹치는 **다른 슬롯**을 찾는다.
/// 탭/워크스페이스 modifier 가 우연히 같으면 교차 충돌도 잡힌다.
fn find_slot_conflict(
    kb: &KeybindingSettings,
    target: BareTarget,
    new_raw: &str,
) -> Option<BareTarget> {
    let target_combo = compose(bare_modifier(kb, target), new_raw);
    if target_combo.is_empty() {
        return None;
    }
    all_bare_targets().into_iter().find(|&other| {
        other != target && {
            let oc = bare_combo(kb, other);
            !oc.is_empty() && oc == target_combo
        }
    })
}

// ── 렌더 ──────────────────────────────────────────────────────────────────

pub(super) fn draw_quick_switch_section(
    ui: &mut egui::Ui,
    keybindings: &mut KeybindingSettings,
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

    // modifier 드롭다운 (기존 blocks 이관).
    egui::Grid::new(format!("{}_modifier_grid", kind.modifier_salt()))
        .num_columns(2)
        .spacing([LABEL_GAP, 8.0])
        .show(ui, |ui| {
            ui.label(t(kind.modifier_label_key()));
            let modifier = match kind {
                QuickSwitchKind::Tab => &mut keybindings.tab_switch_modifier,
                QuickSwitchKind::Workspace => &mut keybindings.workspace_switch_modifier,
            };
            egui::ComboBox::from_id_salt(kind.modifier_salt())
                .selected_text(modifier_display(modifier))
                .show_ui(ui, |ui| {
                    ui.selectable_value(modifier, "ctrl".to_string(), "Ctrl");
                    ui.selectable_value(modifier, "alt".to_string(), "Alt");
                });
            ui.end_row();
        });

    vspace(ui, th.spacing_xs);

    // 라벨 컬럼 폭 — 이 섹션의 모든 라벨(슬롯 + 다음/이전) 중 최장 기준.
    let label_col_width = {
        let font_id = egui::TextStyle::Body.resolve(ui.style());
        let mut targets: Vec<BareTarget> = (0..kind.slot_count())
            .map(|i| kind.slot_target(i))
            .collect();
        targets.push(kind.next_target());
        targets.push(kind.prev_target());
        targets
            .iter()
            .map(|&tg| {
                let text = format!("{}:", bare_display_label(tg));
                ui.ctx().fonts(|f| {
                    f.layout_no_wrap(text, font_id.clone(), egui::Color32::WHITE)
                        .size()
                        .x
                })
            })
            .fold(0.0_f32, f32::max)
    };

    // 슬롯 1~N.
    for i in 0..kind.slot_count() {
        slot_row(
            ui,
            keybindings,
            recording_field,
            can_record,
            kind.slot_target(i),
            label_col_width,
        );
    }
    // 다음/이전.
    slot_row(
        ui,
        keybindings,
        recording_field,
        can_record,
        kind.next_target(),
        label_col_width,
    );
    slot_row(
        ui,
        keybindings,
        recording_field,
        can_record,
        kind.prev_target(),
        label_col_width,
    );

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
    let mut targets: Vec<BareTarget> = (0..kind.slot_count())
        .map(|i| kind.slot_target(i))
        .collect();
    targets.push(kind.next_target());
    targets.push(kind.prev_target());

    let mut out = Vec::new();
    for tg in targets {
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

/// 슬롯 한 줄: 라벨 + 현재 값 버튼(클릭 시 bare-key 녹화 진입).
fn slot_row(
    ui: &mut egui::Ui,
    keybindings: &KeybindingSettings,
    recording_field: &mut Option<RecordingSlot>,
    can_record: bool,
    target: BareTarget,
    label_col_width: f32,
) {
    let th = crate::theme::theme();
    let is_recording = matches!(
        recording_field,
        Some(slot) if slot.field_kind == FieldKind::BareKey(target)
    );

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(label_col_width, BUTTON_HEIGHT),
            egui::Layout::right_to_left(egui::Align::Center),
            |ui| {
                ui.label(format!("{}:", bare_display_label(target)));
            },
        );
        ui.add_space(LABEL_GAP);

        let combo = bare_combo(keybindings, target);
        let display = if is_recording {
            t("settings.keybindings.hint_press_bare_key").to_string()
        } else if combo.is_empty() {
            t("settings.keybindings.hint_none").to_string()
        } else {
            KeybindingSettings::format_display(&combo)
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
            .min_size(egui::vec2(BUTTON_WIDTH, BUTTON_HEIGHT));
        if ui.add_enabled(can_record, btn).clicked() {
            *recording_field = Some(RecordingSlot {
                field_id: String::new(),
                idx: 0,
                field_kind: FieldKind::BareKey(target),
            });
        }
    });
    ui.add_space(ROW_GAP);
}

/// 녹화된 bare 키를 소비해 슬롯에 반영. 충돌 시 기존 `PendingBinding` 팝업 흐름 재사용.
fn consume_capture(
    keybindings: &mut KeybindingSettings,
    recording_field: &mut Option<RecordingSlot>,
    pending_binding: &mut Option<PendingBinding>,
    captured: &KeyCapture,
) {
    let Some(slot) = recording_field.clone() else {
        return;
    };
    let FieldKind::BareKey(target) = slot.field_kind else {
        return;
    };

    match captured {
        KeyCapture::Combo(raw) => {
            let combo = compose(bare_modifier(keybindings, target), raw);
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
                    combo,
                    conflicting_field: cf.to_string(),
                    conflicting_idx: ci,
                    bare_target: Some(target),
                    bare_raw_key: raw.clone(),
                    conflicting_bare: None,
                    conflicting_label: Some(label),
                });
            } else if let Some(other) = find_slot_conflict(keybindings, target, raw) {
                // 다른 quick-switch 슬롯과 중복.
                *pending_binding = Some(PendingBinding {
                    target_field: String::new(),
                    target_idx: 0,
                    combo,
                    conflicting_field: String::new(),
                    conflicting_idx: 0,
                    bare_target: Some(target),
                    bare_raw_key: raw.clone(),
                    conflicting_bare: Some(other),
                    conflicting_label: Some(bare_display_label(other)),
                });
            } else {
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

fn modifier_display(modifier: &str) -> &str {
    match modifier.to_lowercase().as_str() {
        "alt" => "Alt",
        _ => "Ctrl",
    }
}
