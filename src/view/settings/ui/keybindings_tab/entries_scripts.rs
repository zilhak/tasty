//! Keybindings › Scripts 서브탭 — 등록 스크립트(03)에 단축키를 바인딩한다 (ADR-0031).
//!
//! 고정 액션과 달리 스크립트는 동적이라 `RecordingSlot.field_id` 를 `script:<id>` 규약으로
//! 재사용한다. 바인딩 소유권은 이 탭에 있고(05 관리 창은 조회·진입만), combo 충돌은
//! 고정 액션 + 다른 스크립트 바인딩과 함께 검사한다.

use crate::i18n::t;
use crate::settings::{KeybindingSettings, Settings};
use tasty_type_geometry::length::LogicalPx;

use super::{FieldKind, KeyCapture, RecordingSlot};
use tasty_ui_widgets::vspace;

/// `RecordingSlot.field_id` 가 이 접두사면 스크립트 바인딩 슬롯.
const SCRIPT_SLOT_PREFIX: &str = "script:";

const BUTTON_HEIGHT: LogicalPx = LogicalPx(24.0);
const BUTTON_WIDTH: LogicalPx = LogicalPx(140.0);
const LABEL_GAP: LogicalPx = LogicalPx(12.0);

pub(super) fn draw_script_bindings(
    ui: &mut egui::Ui,
    settings: &mut Settings,
    recording_field: &mut Option<RecordingSlot>,
    captured: &KeyCapture,
) {
    let th = crate::theme::theme();
    // 행 간격은 `Theme.spacing_xs` 에서 읽는다. 같은 파일의 세로 리듬이 이미
    // `vspace(ui, th.spacing_sm)` 로 배율을 타므로, 이 간격만 평상수로 두면 1.2 에서
    // 그 리듬과 어긋난다.
    let row_gap = th.spacing_xs;

    // 녹화된 combo 처리 — script: 슬롯만.
    if let Some(slot) = recording_field.clone()
        && let Some(script_id) = slot.field_id.strip_prefix(SCRIPT_SLOT_PREFIX)
    {
        match captured {
            KeyCapture::Combo(combo) => {
                match settings.keybindings.combo_conflict(combo, Some(script_id)) {
                    Some(conflict) => tracing::warn!(
                        target: "tasty_lua",
                        "script binding '{combo}' conflicts with {conflict} — ignored"
                    ),
                    None => settings
                        .keybindings
                        .set_script_binding(script_id, combo.clone()),
                }
                *recording_field = None;
            }
            KeyCapture::Clear => {
                // Escape — 바인딩 해제.
                settings
                    .keybindings
                    .set_script_binding(script_id, String::new());
                *recording_field = None;
            }
            KeyCapture::None => {}
        }
    }

    if settings.scripts.is_empty() {
        vspace(ui, th.spacing_sm);
        ui.label(
            egui::RichText::new(t("settings.keybindings.scripts_empty")).color(th.text_disabled()),
        );
        return;
    }

    // 스크립트 (id, 표시이름) — keybindings 를 변형하는 동안 registry 대여를 피하려 미리 수집.
    let scripts: Vec<(String, String)> = settings
        .scripts
        .iter()
        .map(|e| {
            let name = if e.name.is_empty() {
                e.path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| e.id.clone())
            } else {
                e.name.clone()
            };
            (e.id.clone(), name)
        })
        .collect();

    for (id, name) in &scripts {
        let slot_id = format!("{SCRIPT_SLOT_PREFIX}{id}");
        let is_recording = matches!(recording_field, Some(slot) if slot.field_id == slot_id);
        let current = settings
            .keybindings
            .script_binding_combo(id)
            .unwrap_or("")
            .to_string();

        ui.horizontal_top(|ui| {
            // 라벨 컬럼: 서브탭 공유 고정 폭(`super::LABEL_COL_WIDTH`), 좌측 정렬(entries.rs 와 동일
            // 관례). 사용자 정의 스크립트 이름은 길이 상한이 없어 다른 서브탭과
            // 달리 고정폭을 넘을 수 있다 — 잘리는 대신 말줄임(…) 처리하고, hover
            // 툴팁으로 전체 이름을 확인할 수 있게 한다.
            ui.allocate_ui_with_layout(
                egui::vec2(super::LABEL_COL_WIDTH.value(), BUTTON_HEIGHT.value()),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.add(egui::Label::new(name).truncate())
                        .on_hover_text(name);
                },
            );
            ui.add_space(LABEL_GAP.value());

            let display = if is_recording {
                t("settings.keybindings.hint_press_key").to_string()
            } else if current.is_empty() {
                t("settings.keybindings.hint_none").to_string()
            } else {
                KeybindingSettings::format_display(&current, &settings.general)
            };
            let bg = if is_recording {
                th.surface_hover() // 녹화중 버튼 배경(값-동일: surface1)
            } else {
                th.surface_raised()
            };
            let fg = if is_recording {
                th.text_disabled()
            } else if current.is_empty() {
                th.text_muted()
            } else {
                th.text_primary()
            };
            let btn = egui::Button::new(egui::RichText::new(&display).color(fg).monospace())
                .fill(bg)
                .min_size(egui::vec2(BUTTON_WIDTH.value(), BUTTON_HEIGHT.value()));
            if ui.add(btn).clicked() {
                *recording_field = Some(RecordingSlot {
                    field_id: slot_id.clone(),
                    idx: 0,
                    field_kind: FieldKind::Combo,
                });
            }
        });
        ui.add_space(row_gap.value());
    }
}
