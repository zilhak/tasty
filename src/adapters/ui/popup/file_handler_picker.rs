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
//!
//! ## Split: wrapper / view / action
//!
//! 순수 시각 `draw_file_handler_picker_view` 는 [`FileHandlerPickerProps`] 만 받고
//! [`FileHandlerPickerAction`] 만 반환한다 (AppState/CoreState 비의존).
//! `draw_file_handler_picker` wrapper 가 runtime 상태에서 props 를 추출하고,
//! 반환된 action 을 state mutation + [`PopupAction`] 으로 변환한다.
//! Gallery (`tasty-gallery`) 는 view 를 mock props 로 mirror 해서 시각 검증.

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::{AppState, FileHandlerPickerResult};
use crate::theme;
use crate::theme::Theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::hspace;

pub const PICKER_POPUP_ID: &str = "file_handler_picker";

const POPUP_WIDTH: LogicalPx = LogicalPx(480.0);
const ITEM_HEIGHT: LogicalPx = LogicalPx(22.0);
const LIST_MIN_HEIGHT: LogicalPx = ITEM_HEIGHT.scaled(4.0); // 빈 list 도 시각적 공간 확보
const LIST_MAX_HEIGHT: LogicalPx = ITEM_HEIGHT.scaled(10.0);
const HEADER_HEIGHT: LogicalPx = LogicalPx(36.0); // 대상/형식 두 줄
const BUTTON_ROW_HEIGHT: LogicalPx = LogicalPx(28.0);

/// PopupDef.title_fn — 타이틀바: 대상 파일/디렉토리 전체 경로 포함.
/// 타이틀바 폭에 맞춘 겹침 방지(elide)는 `popup/draw.rs`(모든 popup 공통)가 전담하므로
/// 여기서는 축약하지 않고 원본 경로를 그대로 넘긴다.
pub fn picker_title(state: &AppState, _engine: &crate::core::CoreState) -> String {
    match &state.dialogs.file_handler_picker {
        Some(p) => format!("{}: {}", t("file_handler.picker.title"), p.target_display),
        None => t("file_handler.picker.title").to_string(),
    }
}

/// PopupDef.sizer — 후보/recent list 길이에 따라 높이 조절.
pub fn picker_sizer(state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    let th = theme::theme();
    let (cand_n, recent_n) = match &state.dialogs.file_handler_picker {
        Some(p) => (p.candidates.len(), p.recent.len()),
        None => (0, 0),
    };
    let list_rows = cand_n.max(recent_n);
    let list_height = ITEM_HEIGHT
        .scaled(list_rows.max(4) as f32)
        .max(LIST_MIN_HEIGHT)
        .min(LIST_MAX_HEIGHT);

    let content_height =
        HEADER_HEIGHT + th.spacing_sm.scaled(2.0) + list_height + BUTTON_ROW_HEIGHT;

    egui::vec2(
        POPUP_WIDTH.value(),
        (popup::title_bar_height() + popup::content_margin().scaled(2.0) + content_height).value(),
    )
}

/// 후보/recent 리스트 한 행의 시각 입력 — `HandlerId` 가 owned `String` 이라
/// gallery mock 에서도 안전하게 만들 수 있다.
#[derive(Clone, Debug)]
pub struct FileHandlerPickerEntryView {
    /// `HandlerId::as_str()` 값 (예: `host/markdown-viewer`).
    pub id: String,
    /// 사용자에게 보일 라벨 (번역된 값 또는 handler id).
    pub display: String,
}

/// 순수 시각 view 의 입력. AppState/CoreState 의존 없음.
pub struct FileHandlerPickerProps<'a> {
    pub theme: &'a Theme,
    /// 헤더 본문("대상: ...")에 보일 대상 표시 (이미 축약됨 — `shorten_target` 적용 후).
    /// 타이틀바 텍스트는 이 값을 쓰지 않는다 — `picker_title`이 원본 경로를 넘기고
    /// `popup/draw.rs`의 공통 elide 로직이 타이틀 겹침 방지를 전담한다.
    pub target_display: &'a str,
    /// 탐지된 detector 라벨. `None` 이면 "알 수 없음" 텍스트로 표시.
    pub detector_label: Option<&'a str>,
    pub candidates: &'a [FileHandlerPickerEntryView],
    pub recent: &'a [FileHandlerPickerEntryView],
    /// 현재 선택된 handler id (없으면 [열기] 버튼 비활성화).
    pub selected_id: Option<&'a str>,

    // i18n 라벨 — 호출처가 미리 t() 로 해상해서 전달.
    pub target_label: &'a str,
    pub format_label: &'a str,
    pub unknown_format_label: &'a str,
    pub candidates_heading: &'a str,
    pub recent_heading: &'a str,
    pub empty_label: &'a str,
    pub open_button_label: &'a str,
    pub cancel_button_label: &'a str,
    /// 시스템 전체 handler 가 0개(진짜 빈 상태)일 때만 노출되는 "설정에서
    /// 핸들러 등록" 버튼 라벨. Case A(fallback 후보 존재)에서는 쓰이지 않는다.
    pub open_settings_button_label: &'a str,
}

/// View 가 발생시킨 사용자 의도. Wrapper 가 mutation 으로 변환.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileHandlerPickerAction {
    None,
    /// ESC 또는 [취소] — popup 닫기 + result=Cancelled.
    Cancel,
    /// 단일 클릭 — `selected` 만 갱신, popup 유지.
    Select(String),
    /// 더블클릭 또는 [열기] — result=Selected(id) 후 popup 닫기.
    Dispatch(String),
    /// 진짜 빈 상태(시스템 전체 handler 0개)에서 [설정에서 핸들러 등록] —
    /// result=OpenSettings 후 popup 닫기.
    OpenSettings,
}

/// 순수 시각 view. AppState/CoreState/`theme::theme()` 비의존.
pub fn draw_file_handler_picker_view(
    ui: &mut egui::Ui,
    props: &FileHandlerPickerProps<'_>,
) -> FileHandlerPickerAction {
    let ctx = ui.ctx().clone();

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return FileHandlerPickerAction::Cancel;
    }

    let th = props.theme;

    // Horizontal margin
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(th.spacing_sm.value(), 0.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    // ── Header ────────────────────────────────────────────────────────
    ui.label(
        egui::RichText::new(format!("{} {}", props.target_label, props.target_display))
            .size(th.font_size_body.value())
            .color(th.text_primary()),
    );
    let detector_text = props.detector_label.unwrap_or(props.unknown_format_label);
    ui.label(
        egui::RichText::new(format!("{} {}", props.format_label, detector_text))
            .size(th.font_size_caption.value())
            .color(th.text_muted()),
    );

    ui.add_space(th.spacing_sm.value());

    // ── 빈 상태 (handler 0개) ─────────────────────────────────────────
    let is_empty = props.candidates.is_empty() && props.recent.is_empty();

    if is_empty {
        ui.label(
            egui::RichText::new(props.empty_label)
                .size(th.font_size_body.value())
                .color(th.text_secondary()),
        );
        ui.add_space(th.spacing_sm.value());
        let mut cancel_clicked = false;
        let mut open_settings_clicked = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(props.open_settings_button_label).clicked() {
                open_settings_clicked = true;
            }
            if ui.button(props.cancel_button_label).clicked() {
                cancel_clicked = true;
            }
        });
        if open_settings_clicked {
            return FileHandlerPickerAction::OpenSettings;
        }
        if cancel_clicked {
            return FileHandlerPickerAction::Cancel;
        }
        return FileHandlerPickerAction::None;
    }

    // ── 두 열 list (좌: 후보 / 우: recent) ────────────────────────────
    let mut action = FileHandlerPickerAction::None;

    let list_height = {
        let rows = props.candidates.len().max(props.recent.len());
        ITEM_HEIGHT
            .scaled(rows.max(4) as f32)
            .max(LIST_MIN_HEIGHT)
            .min(LIST_MAX_HEIGHT)
    };

    let total_w = ui.available_width();
    let col_w = (total_w - 8.0) / 2.0;

    ui.horizontal(|ui| {
        // 후보 column
        ui.vertical(|ui| {
            ui.set_min_width(col_w);
            ui.set_max_width(col_w);
            ui.label(
                egui::RichText::new(props.candidates_heading)
                    .size(th.font_size_caption.value())
                    .color(th.text_disabled()),
            );
            egui::ScrollArea::vertical()
                .id_salt("file_handler_picker_candidates")
                .max_height(list_height.value())
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    draw_handler_list(
                        ui,
                        th,
                        props.candidates,
                        props.selected_id,
                        col_w,
                        &mut action,
                    );
                });
        });

        hspace(ui, th.spacing_sm);

        // recent column
        ui.vertical(|ui| {
            ui.set_min_width(col_w);
            ui.set_max_width(col_w);
            ui.label(
                egui::RichText::new(props.recent_heading)
                    .size(th.font_size_caption.value())
                    .color(th.text_disabled()),
            );
            egui::ScrollArea::vertical()
                .id_salt("file_handler_picker_recent")
                .max_height(list_height.value())
                .drag_to_scroll(false)
                .show(ui, |ui| {
                    draw_handler_list(ui, th, props.recent, props.selected_id, col_w, &mut action);
                });
        });
    });

    ui.add_space(th.spacing_sm.value());

    // ── 버튼 row ──────────────────────────────────────────────────────
    let mut open_clicked = false;
    let mut cancel_clicked = false;
    ui.horizontal(|ui| {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let has_selection = props.selected_id.is_some();
            ui.add_enabled_ui(has_selection, |ui| {
                if ui.button(props.open_button_label).clicked() {
                    open_clicked = true;
                }
            });
            if ui.button(props.cancel_button_label).clicked() {
                cancel_clicked = true;
            }
        });
    });

    // 우선순위: 더블클릭(Dispatch) > [열기] > [취소] > 단일클릭(Select)
    if matches!(action, FileHandlerPickerAction::Dispatch(_)) {
        return action;
    }
    if open_clicked && let Some(id) = props.selected_id {
        return FileHandlerPickerAction::Dispatch(id.to_string());
    }
    if cancel_clicked {
        return FileHandlerPickerAction::Cancel;
    }
    action
}

fn draw_handler_list(
    ui: &mut egui::Ui,
    th: &Theme,
    items: &[FileHandlerPickerEntryView],
    selected_id: Option<&str>,
    col_w: f32,
    action: &mut FileHandlerPickerAction,
) {
    for entry in items {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(col_w, ITEM_HEIGHT.value()), egui::Sense::click());

        let is_selected = selected_id == Some(entry.id.as_str());

        if is_selected {
            ui.painter()
                .rect_filled(rect, 0.0, th.active_overlay.to_egui_premultiplied());
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 0.0, th.hover_overlay.to_egui_premultiplied());
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        ui.painter().text(
            egui::pos2(
                rect.min.x + th.spacing_xs.value(),
                rect.center().y - th.font_size_caption.value() / 2.0,
            ),
            egui::Align2::LEFT_TOP,
            &entry.display,
            egui::FontId::proportional(th.font_size_caption.value()),
            if is_selected {
                th.text_primary().into()
            } else {
                th.text_muted().into()
            },
        );

        if resp.double_clicked() {
            *action = FileHandlerPickerAction::Dispatch(entry.id.clone());
        } else if resp.clicked() && !matches!(action, FileHandlerPickerAction::Dispatch(_)) {
            *action = FileHandlerPickerAction::Select(entry.id.clone());
        }

        resp.on_hover_text(&entry.id);
    }
}

/// PopupDef::on_close entry point — X 버튼/외부 클릭 등 draw_fn 을 거치지 않는
/// 경로로 닫히면 `result` 가 아직 `None` 일 수 있다(dispatch 로 이미 채워졌으면
/// 손대지 않는다). 미확정이면 Cancelled 로 명시해 호스트 본체의 result-drain 이
/// 대기 상태로 남지 않게 한다.
pub fn on_close_file_handler_picker(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    if let Some(p) = state.dialogs.file_handler_picker.as_mut()
        && p.result.is_none()
    {
        p.result = Some(crate::state::FileHandlerPickerResult::Cancelled);
    }
}

/// PopupDef.draw_fn — runtime wrapper. props 추출 + view 호출 + action → mutation.
pub fn draw_file_handler_picker(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) -> PopupAction {
    // popup 이 데이터 없이 열려 있으면 즉시 닫기 (이상 상태 회복).
    let Some(picker) = state.dialogs.file_handler_picker.as_ref() else {
        return PopupAction::Close;
    };

    let th = theme::theme();
    let target_display = shorten_target(&picker.target_display);
    let detector_str = picker.detector.as_ref().map(|d| d.as_str().to_string());
    let candidates: Vec<FileHandlerPickerEntryView> = picker
        .candidates
        .iter()
        .map(|s| FileHandlerPickerEntryView {
            id: s.id.as_str().to_string(),
            display: s.display.clone(),
        })
        .collect();
    let recent: Vec<FileHandlerPickerEntryView> = picker
        .recent
        .iter()
        .map(|s| FileHandlerPickerEntryView {
            id: s.id.as_str().to_string(),
            display: s.display.clone(),
        })
        .collect();
    let selected_id_owned = picker.selected.as_ref().map(|s| s.as_str().to_string());

    let target_label = t("file_handler.picker.target_label");
    let format_label = t("file_handler.picker.format_label");
    let unknown_format_label = t("file_handler.picker.unknown_format");
    // Case A(fallback 후보) 는 detector 매칭이 아니라 전체 핸들러이므로 "후보"
    // 대신 명시적으로 구분되는 heading 을 쓴다 — 두 케이스의 안내 문구 구분
    // (`docs/features/file-handler/index.md` 참고).
    let candidates_heading = if picker.candidates_are_fallback {
        t("file_handler.picker.fallback_heading")
    } else {
        t("file_handler.picker.candidates_heading")
    };
    let recent_heading = t("file_handler.picker.recent_heading");
    let empty_label = t("file_handler.picker.empty");
    let open_button_label = t("file_handler.picker.open_button");
    let cancel_button_label = t("button.cancel");
    let open_settings_button_label = t("file_handler.picker.open_settings_button");

    let props = FileHandlerPickerProps {
        theme: &th,
        target_display: &target_display,
        detector_label: detector_str.as_deref(),
        candidates: &candidates,
        recent: &recent,
        selected_id: selected_id_owned.as_deref(),
        target_label,
        format_label,
        unknown_format_label,
        candidates_heading,
        recent_heading,
        empty_label,
        open_button_label,
        cancel_button_label,
        open_settings_button_label,
    };

    let action = draw_file_handler_picker_view(ui, &props);

    match action {
        FileHandlerPickerAction::None => PopupAction::None,
        FileHandlerPickerAction::Cancel => {
            if let Some(p) = state.dialogs.file_handler_picker.as_mut() {
                p.result = Some(FileHandlerPickerResult::Cancelled);
            }
            PopupAction::Close
        }
        FileHandlerPickerAction::Select(id) => {
            if let Some(p) = state.dialogs.file_handler_picker.as_mut() {
                p.selected = Some(crate::file::handler::HandlerId(id));
            }
            PopupAction::None
        }
        FileHandlerPickerAction::Dispatch(id) => {
            if let Some(p) = state.dialogs.file_handler_picker.as_mut() {
                p.result = Some(FileHandlerPickerResult::Selected(
                    crate::file::handler::HandlerId(id),
                ));
            }
            PopupAction::Close
        }
        FileHandlerPickerAction::OpenSettings => {
            if let Some(p) = state.dialogs.file_handler_picker.as_mut() {
                p.result = Some(FileHandlerPickerResult::OpenSettings);
            }
            PopupAction::Close
        }
    }
}

/// 헤더 본문("대상: ...") 한 줄 표시용 축약. 타이틀바 겹침 방지 목적으로는 쓰지
/// 않는다(그건 `popup/draw.rs`의 폭 기준 elide 가 전담) — 이 함수는 문자 수(64) 기준의
/// 대략적인 축약으로, 본문이 popup 폭을 크게 벗어나는 것만 막는다.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn run_with_input(
        raw: egui::RawInput,
        candidates: &[FileHandlerPickerEntryView],
        recent: &[FileHandlerPickerEntryView],
        selected_id: Option<&str>,
    ) -> FileHandlerPickerAction {
        let ctx = egui::Context::default();
        let theme = test_theme();
        let mut out = FileHandlerPickerAction::None;
        drop(ctx.run(raw, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let props = FileHandlerPickerProps {
                    theme: &theme,
                    target_display: "/tmp/foo.md",
                    detector_label: Some("markdown"),
                    candidates,
                    recent,
                    selected_id,
                    target_label: "Target:",
                    format_label: "Format:",
                    unknown_format_label: "unknown",
                    candidates_heading: "Candidates",
                    recent_heading: "Recent",
                    empty_label: "No handlers registered.",
                    open_button_label: "Open",
                    cancel_button_label: "Cancel",
                    open_settings_button_label: "Open Settings",
                };
                out = draw_file_handler_picker_view(ui, &props);
            });
        }));
        out
    }

    #[test]
    fn view_returns_cancel_on_escape() {
        let mut raw = egui::RawInput::default();
        raw.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let action = run_with_input(raw, &[], &[], None);
        assert_eq!(action, FileHandlerPickerAction::Cancel);
    }

    #[test]
    fn view_returns_none_on_idle_empty() {
        let action = run_with_input(egui::RawInput::default(), &[], &[], None);
        assert_eq!(action, FileHandlerPickerAction::None);
    }

    #[test]
    fn view_renders_with_entries_without_panic() {
        let cands = vec![
            FileHandlerPickerEntryView {
                id: "host/markdown-viewer".into(),
                display: "Markdown Viewer".into(),
            },
            FileHandlerPickerEntryView {
                id: "user/my-md".into(),
                display: "user/my-md".into(),
            },
        ];
        let recent = vec![FileHandlerPickerEntryView {
            id: "host/image-viewer".into(),
            display: "Image Viewer".into(),
        }];
        let action = run_with_input(
            egui::RawInput::default(),
            &cands,
            &recent,
            Some("host/markdown-viewer"),
        );
        assert_eq!(action, FileHandlerPickerAction::None);
    }
}
