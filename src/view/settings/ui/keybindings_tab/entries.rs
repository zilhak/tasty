use crate::i18n::t;
use crate::settings::KeybindingSettings;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{settings_label_cell, settings_label_gap};

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
        ui.horizontal_top(|ui| {
            // 서브탭 공유 폭의 라벨 열. 라벨이 짧아도 열 폭을 그대로 차지하고 도움말 아이콘은 열 안에 둔다.
            settings_label_cell(
                ui,
                &th,
                label_col,
                button_height,
                t(label_key),
                desc_key.map(t),
            );
            settings_label_gap(ui, &th);

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

                    let bg_color = if is_recording {
                        th.surface_hover() // 녹화중 버튼 배경(값-동일: surface1)
                    } else {
                        th.surface_raised()
                    };
                    let text_color = if is_recording {
                        th.text_disabled()
                    } else {
                        th.text_primary()
                    };

                    let button = record_button(
                        &th,
                        can_record,
                        &display_text,
                        text_color,
                        bg_color,
                        button_width,
                        button_height,
                    );

                    if ui.add(button).clicked() {
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
                let add_label = if adding {
                    t("settings.keybindings.hint_press_key").to_string()
                } else if bindings_len == 0 {
                    t("settings.keybindings.hint_none").to_string()
                } else {
                    "+".to_string()
                };
                let add_bg = if adding {
                    th.surface_hover() // 추가중 버튼 배경(값-동일: surface1)
                } else {
                    th.surface_raised()
                };
                let add_fg = if adding {
                    th.text_disabled()
                } else {
                    th.text_muted()
                };
                let add_width = if bindings_len == 0 {
                    button_width
                } else {
                    add_button_width
                };
                let add_btn = record_button(
                    &th,
                    can_record,
                    &add_label,
                    add_fg,
                    add_bg,
                    add_width,
                    button_height,
                );
                if ui
                    .add(add_btn)
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

/// 녹화 버튼. Import / Export 의 녹화 슬롯과 같은 모양이다 — mono caption 글자, 1px border-default 테두리.
/// 다른 녹화가 대기 중이면(`enabled=false`) egui의 비활성 흐림 대신 disabled 상자 role과
/// disabled ink로 그리고 클릭을 받지 않는다.
pub(super) fn record_button(
    th: &tasty_type_appearance::theme::Theme,
    enabled: bool,
    label: &str,
    fg: tasty_type_appearance::color::HexColor,
    bg: tasty_type_appearance::color::HexColor,
    width: LogicalPx,
    height: LogicalPx,
) -> egui::Button<'static> {
    let text = egui::RichText::new(label)
        .monospace()
        .size(th.font_size_caption.value());
    let button = if enabled {
        egui::Button::new(text.color(fg))
            .fill(bg)
            .stroke(egui::Stroke::new(
                th.border_width.value(),
                th.border_default(),
            ))
    } else {
        egui::Button::new(text.color(th.state_disabled_fg()))
            .fill(th.state_disabled_fill())
            .stroke(egui::Stroke::new(
                th.border_width.value(),
                th.state_disabled_border(),
            ))
            .sense(egui::Sense::hover())
    };
    button.min_size(egui::vec2(width.value(), height.value()))
}
