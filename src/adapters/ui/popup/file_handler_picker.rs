//! File handler picker popup — 사용자가 detector 후보 중 한 handler 를 직접 선택.
//!
//! Popup 자체는 dispatch 하지 않는다. 선택 결과를 `state.dialogs.file_handler_picker.result`
//! 로 남기고, host 본체 layer 가 frame 끝에 result 를 소비해 실행 + RecentPicks 기록.
//!
//! 레이아웃 (헤드리스 X, default 너비 480, 동적 높이 sizer):
//!   ┌───────────────────────────────────────────────────────────────────┐
//!   │ 대상: <target_display>                                             │
//!   │ 형식: <detector or unknown>                                        │
//!   ├──────────────── 후보 ─────────────────┬──────── 최근 ─────────────┤
//!   │ ▸ host/markdown-viewer                │ ▸ user/my-pdf-opener      │
//!   │   user/my-md-handler                  │   host/image-viewer       │
//!   │   ...                                 │   ...                     │
//!   ├───────────────────────────────────────┴───────────────────────────┤
//!   │                                              [ 취소 ]  [ 열기 ]   │
//!   └───────────────────────────────────────────────────────────────────┘

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::{AppState, FileHandlerPickerResult, PickerHandlerSummary};
use crate::theme;

pub const PICKER_POPUP_ID: &str = "file_handler_picker";

const POPUP_WIDTH: f32 = 480.0;
const ITEM_HEIGHT: f32 = 22.0;
const LIST_MIN_HEIGHT: f32 = 4.0 * ITEM_HEIGHT; // 빈 list 도 시각적 공간 확보
const LIST_MAX_HEIGHT: f32 = 10.0 * ITEM_HEIGHT;
const HEADER_HEIGHT: f32 = 36.0; // 대상/형식 두 줄
const BUTTON_ROW_HEIGHT: f32 = 28.0;
const VERTICAL_PADDING: f32 = 8.0;
const HORIZONTAL_MARGIN: f32 = 8.0;

/// PopupDef.title_fn — 타이틀바: 대상 파일/디렉토리 짧은 이름 포함.
pub fn picker_title(state: &AppState, _engine: &crate::core::CoreState) -> String {
    match &state.dialogs.file_handler_picker {
        Some(p) => format!(
            "{}: {}",
            t("file_handler.picker.title"),
            shorten_target(&p.target_display),
        ),
        None => t("file_handler.picker.title").to_string(),
    }
}

/// PopupDef.sizer — 후보/recent list 길이에 따라 높이 조절.
pub fn picker_sizer(state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    let (cand_n, recent_n) = match &state.dialogs.file_handler_picker {
        Some(p) => (p.candidates.len(), p.recent.len()),
        None => (0, 0),
    };
    let list_rows = cand_n.max(recent_n);
    let list_height =
        (list_rows.max(4) as f32 * ITEM_HEIGHT).clamp(LIST_MIN_HEIGHT, LIST_MAX_HEIGHT);

    let content_height =
        HEADER_HEIGHT + VERTICAL_PADDING + list_height + VERTICAL_PADDING + BUTTON_ROW_HEIGHT;

    egui::vec2(
        POPUP_WIDTH,
        popup::TITLE_BAR_HEIGHT + popup::CONTENT_MARGIN * 2.0 + content_height,
    )
}

/// PopupDef.draw_fn.
pub fn draw_file_handler_picker(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let ctx = ui.ctx().clone();

    // popup 이 데이터 없이 열려 있으면 즉시 닫기 (이상 상태 회복).
    if state.dialogs.file_handler_picker.is_none() {
        return PopupAction::Close;
    }

    // ESC — 취소 결과 기록 후 닫기.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        if let Some(p) = state.dialogs.file_handler_picker.as_mut() {
            p.result = Some(FileHandlerPickerResult::Cancelled);
        }
        return PopupAction::Close;
    }

    let th = theme::theme();

    // Horizontal margin
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(HORIZONTAL_MARGIN, 0.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    // ── Header ────────────────────────────────────────────────────────
    {
        let p = match state.dialogs.file_handler_picker.as_ref() {
            Some(p) => p,
            None => return PopupAction::Close,
        };
        ui.label(
            egui::RichText::new(format!(
                "{} {}",
                t("file_handler.picker.target_label"),
                shorten_target(&p.target_display)
            ))
            .size(th.font_size_body.value())
            .color(th.text),
        );
        let detector_text = match &p.detector {
            Some(id) => id.as_str().to_string(),
            None => t("file_handler.picker.unknown_format").to_string(),
        };
        ui.label(
            egui::RichText::new(format!(
                "{} {}",
                t("file_handler.picker.format_label"),
                detector_text
            ))
            .size(th.font_size_caption.value())
            .color(th.subtext0),
        );
    }

    ui.add_space(VERTICAL_PADDING);

    // ── 빈 상태 (handler 0개) ─────────────────────────────────────────
    let is_empty = state
        .dialogs
        .file_handler_picker
        .as_ref()
        .map(|p| p.candidates.is_empty() && p.recent.is_empty())
        .unwrap_or(true);

    if is_empty {
        ui.label(
            egui::RichText::new(t("file_handler.picker.empty"))
                .size(th.font_size_body.value())
                .color(th.subtext1),
        );
        ui.add_space(VERTICAL_PADDING);
        // 취소만 보여준다.
        let mut cancel_clicked = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("button.cancel")).clicked() {
                cancel_clicked = true;
            }
        });
        if cancel_clicked {
            if let Some(p) = state.dialogs.file_handler_picker.as_mut() {
                p.result = Some(FileHandlerPickerResult::Cancelled);
            }
            return PopupAction::Close;
        }
        return PopupAction::None;
    }

    // ── 두 열 list (좌: 후보 / 우: recent) ────────────────────────────
    // double_click_dispatch: 더블클릭 또는 [열기] 시 dispatch.
    let mut double_click_dispatch: Option<crate::file::handler::HandlerId> = None;

    let list_height = {
        let rows = state
            .dialogs
            .file_handler_picker
            .as_ref()
            .map(|p| p.candidates.len().max(p.recent.len()))
            .unwrap_or(0);
        (rows.max(4) as f32 * ITEM_HEIGHT).clamp(LIST_MIN_HEIGHT, LIST_MAX_HEIGHT)
    };

    let total_w = ui.available_width();
    let col_w = (total_w - 8.0) / 2.0;

    ui.horizontal(|ui| {
        // 후보 column
        ui.vertical(|ui| {
            ui.set_min_width(col_w);
            ui.set_max_width(col_w);
            ui.label(
                egui::RichText::new(t("file_handler.picker.candidates_heading"))
                    .size(th.font_size_caption.value())
                    .color(th.overlay1),
            );
            egui::ScrollArea::vertical()
                .id_salt("file_handler_picker_candidates")
                .max_height(list_height)
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    let items = state
                        .dialogs
                        .file_handler_picker
                        .as_ref()
                        .map(|p| p.candidates.clone())
                        .unwrap_or_default();
                    draw_handler_list(
                        ui,
                        &th,
                        &items,
                        col_w,
                        state,
                        engine,
                        &mut double_click_dispatch,
                    );
                });
        });

        ui.add_space(8.0);

        // recent column
        ui.vertical(|ui| {
            ui.set_min_width(col_w);
            ui.set_max_width(col_w);
            ui.label(
                egui::RichText::new(t("file_handler.picker.recent_heading"))
                    .size(th.font_size_caption.value())
                    .color(th.overlay1),
            );
            egui::ScrollArea::vertical()
                .id_salt("file_handler_picker_recent")
                .max_height(list_height)
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    let items = state
                        .dialogs
                        .file_handler_picker
                        .as_ref()
                        .map(|p| p.recent.clone())
                        .unwrap_or_default();
                    draw_handler_list(
                        ui,
                        &th,
                        &items,
                        col_w,
                        state,
                        engine,
                        &mut double_click_dispatch,
                    );
                });
        });
    });

    ui.add_space(VERTICAL_PADDING);

    // ── 버튼 row ──────────────────────────────────────────────────────
    let mut open_clicked = false;
    let mut cancel_clicked = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let has_selection = state
                .dialogs
                .file_handler_picker
                .as_ref()
                .map(|p| p.selected.is_some())
                .unwrap_or(false);
            ui.add_enabled_ui(has_selection, |ui| {
                if ui.button(t("file_handler.picker.open_button")).clicked() {
                    open_clicked = true;
                }
            });
            if ui.button(t("button.cancel")).clicked() {
                cancel_clicked = true;
            }
        });
    });

    // dispatch 결정 — 우선순위: 더블클릭 > [열기]
    let dispatch_id = double_click_dispatch.or_else(|| {
        if open_clicked {
            state
                .dialogs
                .file_handler_picker
                .as_ref()
                .and_then(|p| p.selected.clone())
        } else {
            None
        }
    });

    if let Some(id) = dispatch_id {
        if let Some(p) = state.dialogs.file_handler_picker.as_mut() {
            p.result = Some(FileHandlerPickerResult::Selected(id));
        }
        return PopupAction::Close;
    }

    if cancel_clicked {
        if let Some(p) = state.dialogs.file_handler_picker.as_mut() {
            p.result = Some(FileHandlerPickerResult::Cancelled);
        }
        return PopupAction::Close;
    }

    PopupAction::None
}

fn draw_handler_list(
    ui: &mut egui::Ui,
    th: &theme::Theme,
    items: &[PickerHandlerSummary],
    col_w: f32,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
    double_click_dispatch: &mut Option<crate::file::handler::HandlerId>,
) {
    for entry in items {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(col_w, ITEM_HEIGHT), egui::Sense::click());

        let is_selected = state
            .dialogs
            .file_handler_picker
            .as_ref()
            .and_then(|p| p.selected.as_ref())
            .map(|sel| sel == &entry.id)
            .unwrap_or(false);

        if is_selected {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
        }
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        ui.painter().text(
            egui::pos2(
                rect.min.x + 4.0,
                rect.center().y - th.font_size_caption.value() / 2.0,
            ),
            egui::Align2::LEFT_TOP,
            &entry.display,
            egui::FontId::proportional(th.font_size_caption.value()),
            if is_selected {
                th.text.into()
            } else {
                th.subtext0.into()
            },
        );

        if resp.clicked()
            && let Some(p) = state.dialogs.file_handler_picker.as_mut()
        {
            p.selected = Some(entry.id.clone());
        }
        if resp.double_clicked() {
            *double_click_dispatch = Some(entry.id.clone());
        }

        resp.on_hover_text(entry.id.as_str());
    }
}

fn shorten_target(s: &str) -> String {
    const MAX: usize = 64;
    if s.len() <= MAX {
        return s.to_string();
    }
    let parts: Vec<&str> = s.split(['/', '\\']).filter(|p| !p.is_empty()).collect();
    if parts.len() <= 2 {
        return s.to_string();
    }
    format!(".../{}", parts[parts.len() - 2..].join("/"))
}
