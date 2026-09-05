use crate::i18n::t;
use crate::settings::{GeneralSettings, KeybindingSettings};
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{HelpHint, TooltipPlacement};

use super::{FieldKind, KeyCapture, PendingBinding, RecordingSlot};

pub(super) fn draw_keybinding_entries(
    ui: &mut egui::Ui,
    keybindings: &mut KeybindingSettings,
    general: &GeneralSettings,
    recording_field: &mut Option<RecordingSlot>,
    pending_binding: &mut Option<PendingBinding>,
    captured: &KeyCapture,
    entries: &[(&str, &str, Option<&str>)],
) {
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

    // 버튼/간격 치수. 4px 그리드 준수.
    const BUTTON_HEIGHT: LogicalPx = LogicalPx(24.0);
    const BUTTON_WIDTH: LogicalPx = LogicalPx(140.0);
    const ADD_BUTTON_WIDTH: LogicalPx = LogicalPx(32.0);
    const LABEL_GAP: LogicalPx = LogicalPx(12.0);
    // 행 간격과 라벨↔`(?)` 간격은 `Theme.spacing_xs` 에서 읽는다. 종전에는 둘 다
    // `LogicalPx(4.0)` 평상수였는데, **이웃이 이미 배율을 탄다**: 이 서브탭을 감싸는
    // `keybindings_tab.rs` 의 세로 리듬이 `vspace(th.spacing_xs)` 이고, `(?)` 슬롯의
    // 아이콘 폭은 바로 아래에서 `th.icon_glyph_size_sm` 로 잡는다. 평상수만 배율을
    // 안 타면 1.2 에서 그 리듬과 슬롯이 어긋난다 — 값이 아니라 **어느 이름을 부르는가**
    // 가 배율 동작을 정한다(`tasty-ui-widgets` 의 `STRUCT_GAP_*` 주석과 같은 규칙).
    let row_gap = th.spacing_xs;
    let help_hint_gap = th.spacing_xs;

    for (field_id, label_key, desc_key) in entries.iter() {
        ui.horizontal_top(|ui| {
            // 라벨 컬럼: 서브탭 공유 고정 폭(`super::LABEL_COL_WIDTH`), 좌측 정렬(remote_transfer.rs
            // 의 settings_row() 와 동일 관례). left_to_right 이므로 먼저 add한
            // 위젯이 왼쪽 끝에 배치된다 — 라벨을 먼저 add해 "라벨 (?)" 순서(= (?)
            // 가 라벨 바로 뒤에 이어짐)를 만든다.
            ui.allocate_ui_with_layout(
                egui::vec2(super::LABEL_COL_WIDTH.value(), BUTTON_HEIGHT.value()),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(t(label_key));
                    ui.add_space(help_hint_gap.value());
                    if let Some(desc_key) = desc_key {
                        HelpHint::new(t(desc_key))
                            .placement(TooltipPlacement::Bottom)
                            .show(ui, &th);
                    } else {
                        ui.add_space(th.icon_glyph_size_sm.value());
                    }
                },
            );
            ui.add_space(LABEL_GAP.value());

            // 버튼 영역: 남은 폭을 모두 사용. 폭을 초과하면 자동 줄바꿈.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(row_gap.value(), row_gap.value());

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

                    let button = egui::Button::new(
                        egui::RichText::new(&display_text)
                            .color(text_color)
                            .monospace(),
                    )
                    .fill(bg_color)
                    .min_size(egui::vec2(BUTTON_WIDTH.value(), BUTTON_HEIGHT.value()));

                    if ui.add_enabled(can_record, button).clicked() {
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
                    BUTTON_WIDTH.value()
                } else {
                    ADD_BUTTON_WIDTH.value()
                };
                let add_btn =
                    egui::Button::new(egui::RichText::new(&add_label).color(add_fg).monospace())
                        .fill(add_bg)
                        .min_size(egui::vec2(add_width, BUTTON_HEIGHT.value()));
                if ui
                    .add_enabled(can_record, add_btn)
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
        ui.add_space(row_gap.value());
    }
}
