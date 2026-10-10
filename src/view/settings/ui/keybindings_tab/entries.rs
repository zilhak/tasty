use crate::i18n::t;
use crate::settings::KeybindingSettings;
use tasty_ui_widgets::{KbRecordSlot, StackRow, kb_record_slot, settings_stack_row};

use super::{FieldKind, KeyCapture, PendingBinding, RecordingSlot, RowLayout};

pub(super) fn draw_keybinding_entries(
    ui: &mut egui::Ui,
    keybindings: &mut KeybindingSettings,
    layout: RowLayout<'_>,
    recording_field: &mut Option<RecordingSlot>,
    pending_binding: &mut Option<PendingBinding>,
    captured: &KeyCapture,
    entries: &[(&str, &str, Option<&str>)],
) {
    let RowLayout { general, label_col } = layout;
    let th = crate::theme::theme();
    // 충돌 팝업이 떠 있는 동안은 녹화 버튼을 눌러도 녹화 상태로 진입하지 않도록 가드.
    let can_record = pending_binding.is_none();

    // 녹화된 combo 처리: 녹화 슬롯이 정해져 있을 때만 적용.
    // BareKey 슬롯(quick-switch)은 같은 서브탭에서 함께 렌더되는 `quick_switch.rs`
    // 가 소비하므로 여기서 가로채지 않는다(Combo 슬롯만 처리).
    if let Some(slot) = recording_field.clone()
        && slot.field_kind == FieldKind::Combo
    {
        match captured {
            KeyCapture::Combo(combo) => {
                match keybindings.find_conflict(&slot.field_id, combo) {
                    Some((conflicting, conflicting_idx)) => {
                        *pending_binding = Some(PendingBinding {
                            target_field: slot.field_id.clone(),
                            target_idx: slot.idx,
                            combo: combo.clone(),
                            conflicting_field: conflicting.to_string(),
                            conflicting_idx,
                            bare_target: None,
                            bare_raw_key: String::new(),
                            conflicting_bare: None,
                            conflicting_label: None,
                        });
                    }
                    None => {
                        keybindings.replace_binding_at(&slot.field_id, slot.idx, combo.clone());
                    }
                }
                *recording_field = None;
            }
            KeyCapture::Clear => {
                // Escape — 녹화 중인 슬롯이 기존 엔트리면 제거, 새 슬롯이면 그냥 취소.
                let current_len = keybindings
                    .get_bindings(&slot.field_id)
                    .map(|v| v.len())
                    .unwrap_or(0);
                if slot.idx < current_len {
                    keybindings.remove_binding(&slot.field_id, slot.idx);
                }
                *recording_field = None;
            }
            KeyCapture::None => {}
        }
    }

    let button_height = th.kb_record_height();
    let button_width = th.kb_record_width();
    let add_button_width = th.kb_record_add_width();
    // 한 행 안의 버튼 사이(줄바꿈된 버튼 줄 사이 포함) 간격. 행 사이는 kb-row-gap 이다.
    let in_row_gap = th.spacing_xs;

    for (field_id, label_key, desc_key) in entries.iter() {
        // 서브탭 공유 폭의 라벨 열. 라벨이 짧아도 열 폭을 그대로 차지하고 도움말 아이콘은 열 안에 둔다.
        // 첫 버튼도 라벨 옆에 들어가지 않으면 행을 쌓는다.
        let frame = StackRow {
            id: ui.id().with(("kb_entry", *field_id)),
            label_col,
            row_h: button_height,
            label: t(label_key),
            hint: desc_key.map(t),
            align_top: true,
        };
        settings_stack_row(ui, &th, frame, |ui| {
            // 버튼 영역: 남은 폭을 모두 사용. 폭을 초과하면 자동 줄바꿈.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(in_row_gap.value(), in_row_gap.value());

                let bindings_len = keybindings
                    .get_bindings(field_id)
                    .map(|v| v.len())
                    .unwrap_or(0);

                // 기존 바인딩 각각을 버튼으로 표시.
                for idx in 0..bindings_len {
                    let is_recording = matches!(
                        recording_field,
                        Some(slot) if slot.field_id == *field_id && slot.idx == idx
                    );
                    let current = keybindings
                        .get_bindings(field_id)
                        .and_then(|v| v.get(idx))
                        .cloned()
                        .unwrap_or_default();

                    let display_text = if is_recording {
                        t("settings.keybindings.hint_press_key").to_string()
                    } else {
                        KeybindingSettings::format_display(&current, general)
                    };
                    let slot = if is_recording {
                        KbRecordSlot::Recording(&display_text)
                    } else {
                        KbRecordSlot::Binding(&display_text)
                    };

                    if kb_record_slot(ui, &th, slot, button_width, can_record).clicked() {
                        *recording_field = Some(RecordingSlot {
                            field_id: field_id.to_string(),
                            idx,
                            field_kind: FieldKind::Combo,
                        });
                    }
                }

                // 새 바인딩 추가 버튼. 바인딩이 없을 때는 "없음" 플레이스홀더.
                let adding = matches!(
                    recording_field,
                    Some(slot) if slot.field_id == *field_id && slot.idx == bindings_len
                );
                let press_key = t("settings.keybindings.hint_press_key");
                let none = t("settings.keybindings.hint_none");
                // 바인딩이 없는 행은 None 슬롯 하나만 두고 + 를 따로 두지 않는다.
                let (add_slot, add_width) = if adding {
                    let width = if bindings_len == 0 {
                        button_width
                    } else {
                        add_button_width
                    };
                    (KbRecordSlot::Recording(press_key), width)
                } else if bindings_len == 0 {
                    (KbRecordSlot::Empty(none), button_width)
                } else {
                    (KbRecordSlot::Add, add_button_width)
                };
                if kb_record_slot(ui, &th, add_slot, add_width, can_record)
                    .on_hover_text(t("settings.keybindings.add_binding_button"))
                    .clicked()
                {
                    *recording_field = Some(RecordingSlot {
                        field_id: field_id.to_string(),
                        idx: bindings_len,
                        field_kind: FieldKind::Combo,
                    });
                }
            });
        });
        kb_row_gap(ui, &th);
    }
}

/// 단축키 행 사이를 `kb-row-gap` 으로 맞춘다. 부모의 `item_spacing.y` 가 행 뒤에 이미 들어가므로
/// 그만큼 빼고 띄운다.
pub(super) fn kb_row_gap(ui: &mut egui::Ui, th: &tasty_type_appearance::theme::Theme) {
    let auto = ui.spacing().item_spacing.y;
    ui.add_space((th.kb_row_gap().value() - auto).max(0.0));
}
