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
use egui::emath::GuiRounding as _;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::hspace;

pub const PICKER_POPUP_ID: &str = "file_handler_picker";

const POPUP_WIDTH: LogicalPx = LogicalPx(480.0);
const ITEM_HEIGHT: LogicalPx = LogicalPx(22.0);
const LIST_MIN_HEIGHT: LogicalPx = ITEM_HEIGHT.scaled(4.0); // 빈 list 도 시각적 공간 확보
const LIST_MAX_HEIGHT: LogicalPx = ITEM_HEIGHT.scaled(10.0);
const HEADER_HEIGHT: LogicalPx = LogicalPx(36.0); // 대상/형식 두 줄
const BUTTON_ROW_HEIGHT: LogicalPx = LogicalPx(28.0);
/// 컬럼 heading(`후보`/`최근`) 한 줄. `font_size_caption`(11) 의 행 높이다 —
/// `HEADER_HEIGHT`(36) 가 body 한 줄 + caption 한 줄 + item_spacing 이라는 것과 같은
/// 근거에서 나온다. **이 줄은 `ScrollArea` 바깥에 쌓이므로 `list_height` 에 안 들어간다.**
const HEADING_LINE_HEIGHT: LogicalPx = LogicalPx(15.0);
/// 빈 상태 안내문 한 줄(`font_size_body`(13) 의 행 높이).
const EMPTY_LABEL_HEIGHT: LogicalPx = LogicalPx(17.0);

/// PopupDef.title_fn — 타이틀바: 대상 파일/디렉토리 전체 경로 포함.
/// 타이틀바 폭에 맞춘 겹침 방지(elide)는 `popup/draw.rs`(모든 popup 공통)가 전담하므로
/// 여기서는 축약하지 않고 원본 경로를 그대로 넘긴다.
pub fn picker_title(state: &AppState, _engine: &crate::core::CoreState) -> String {
    match &state.dialogs.file_handler_picker {
        Some(p) => format!("{}: {}", t("file_handler.picker.title"), p.target_display),
        None => t("file_handler.picker.title").to_string(),
    }
}

/// 두 열 list 의 높이 — **sizer 와 view 가 같은 값을 써야** 마지막 행이 안 잘린다.
///
/// 예전에는 이 식이 두 곳에 복제돼 있었다. 복제 자체는 같은 값을 냈지만, 복제가 있다는
/// 것이 "레이아웃을 두 곳에서 따로 계산한다" 는 구조를 감췄고, 그 구조 때문에 view 에
/// 나중에 들어온 컬럼 heading 을 sizer 가 안 세게 됐다.
fn list_height_for(cand_n: usize, recent_n: usize) -> LogicalPx {
    ITEM_HEIGHT
        .scaled(cand_n.max(recent_n).max(4) as f32)
        .max(LIST_MIN_HEIGHT)
        .min(LIST_MAX_HEIGHT)
}

/// `tasty_egui_theme` 가 `style.spacing.item_spacing.y` 로 적용하는 값. egui 는 수직으로
/// 쌓이는 위젯 **사이마다** 이 값을 넣으므로 sizer 도 같은 식으로 세야 한다.
///
/// Theme 토큰이 host UI zoom 을 이미 반영하므로 별도 scale 곱셈을 하지 않는다.
fn effective_item_spacing() -> f32 {
    theme::theme().spacing_xs.value().round_ui()
}

/// popup 높이 — **view 가 세로로 쌓는 것을 하나도 빠짐없이** 센다.
///
/// [`draw_file_handler_picker_view`] 가 쌓는 순서와 1:1 로 대응한다. 갈래가 둘인 것이
/// 요점이다: 빈 상태에는 컬럼 heading 도 list 도 없고 안내문 한 줄이 대신 들어간다.
/// 한 식으로 뭉뚱그리면 한쪽은 잘리고 다른 쪽은 빈 여백이 남는다.
///
/// 바깥 수직 스택은 여섯 항목(대상 · 형식 · 여백 · 본문 · 여백 · 버튼 row)이라 그
/// **사이**가 다섯이다. 목록 갈래는 본문 안에서 heading 과 `ScrollArea` 가 한 번 더
/// 벌어져 여섯이 된다.
fn picker_size_for(cand_n: usize, recent_n: usize, item_spacing: f32) -> egui::Vec2 {
    let th = theme::theme();
    let gaps_outside = 5.0;

    let base = HEADER_HEIGHT
        + th.spacing_sm.scaled(2.0)
        + BUTTON_ROW_HEIGHT
        + LogicalPx(gaps_outside * item_spacing);

    // view 의 `is_empty` 와 같은 조건이다 — 갈라지는 자리가 둘이면 조건도 둘이 된다.
    let content_height = if cand_n == 0 && recent_n == 0 {
        base + EMPTY_LABEL_HEIGHT
    } else {
        base + HEADING_LINE_HEIGHT + LogicalPx(item_spacing) + list_height_for(cand_n, recent_n)
    };

    // `round_ui` 누적 오차와 egui `Ui::new` 초기 cursor padding 흡수용. 형제 popup
    // (`convert.rs`)이 같은 이유로 같은 값을 쓴다 — 마지막 항목 baseline 이 콘텐츠 경계와
    // 정확히 일치하면 anti-alias 한 줄이 잘려 보인다.
    let safety_margin = 1.0;
    egui::vec2(
        POPUP_WIDTH.value(),
        (popup::title_bar_height()
            + popup::content_margin().scaled(2.0)
            + content_height
            + LogicalPx(safety_margin))
        .value(),
    )
}

/// PopupDef.default_size — popup 등록 시점의 placeholder.
///
/// sizer 가 매 프레임 재계산하므로 첫 프레임에만 쓰이는데, 손으로 적은 값이 sizer 와
/// 어긋나면 그 첫 프레임이 깜빡인다. 그래서 **같은 식에서** 뽑는다 — 형제 popup 여섯이
/// 이미 이 형태다. 기준은 목록 갈래의 최소 크기(양쪽 4 행)다.
pub fn picker_default_size() -> egui::Vec2 {
    picker_size_for(4, 4, theme::theme().spacing_xs.value())
}

/// PopupDef.sizer — 후보/recent list 길이에 따라 높이 조절.
pub fn picker_sizer(state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    let (cand_n, recent_n) = match &state.dialogs.file_handler_picker {
        Some(p) => (p.candidates.len(), p.recent.len()),
        None => (0, 0),
    };
    picker_size_for(cand_n, recent_n, effective_item_spacing())
}

#[cfg(test)]
mod size_tests {
    //! popup 높이가 view 가 쌓는 것을 **전부** 덮는지 고정한다.
    //!
    //! 예전 식은 컬럼 heading 을 안 셌다. heading 은 `ScrollArea` **바깥**에 쌓이는데
    //! `list_height` 는 항목 행만 계산해서, popup 이 최소 heading 한 줄만큼 짧게 열리고
    //! 목록 마지막 항목이 세로로 잘렸다. egui 가 위젯 사이마다 넣는 `item_spacing.y` 도
    //! 식에 없었다.
    use super::*;

    /// view 가 세로로 쌓는 것을 **다시 한 번 서술해서** 필요 높이를 만든다.
    ///
    /// 식을 두 번 쓰는 것이 목적이다 — sizer 는 이 값 **이상**을 내야 하고, 이 서술이
    /// [`draw_file_handler_picker_view`] 를 따라간다. 한쪽만 고치면 어긋남이 여기서 뜬다.
    fn needed_height(cand_n: usize, recent_n: usize, item_spacing: f32) -> LogicalPx {
        let th = theme::theme();
        let body = popup::title_bar_height()
            + popup::content_margin().scaled(2.0)
            + HEADER_HEIGHT
            + th.spacing_sm.scaled(2.0)
            + BUTTON_ROW_HEIGHT
            + LogicalPx(5.0 * item_spacing);
        if cand_n == 0 && recent_n == 0 {
            body + EMPTY_LABEL_HEIGHT
        } else {
            body + HEADING_LINE_HEIGHT + LogicalPx(item_spacing) + list_height_for(cand_n, recent_n)
        }
    }

    fn assert_fits(cand_n: usize, recent_n: usize, item_spacing: f32) {
        let got = LogicalPx(picker_size_for(cand_n, recent_n, item_spacing).y);
        let needed = needed_height(cand_n, recent_n, item_spacing);
        assert!(
            got >= needed,
            "popup 높이 {got:?} < 필요 {needed:?} (후보 {cand_n} · 최근 {recent_n} · \
             spacing {item_spacing}) — 마지막 항목이 잘린다"
        );
    }

    /// 항목 수 세 갈래를 다 본다: 최소 높이(4 미만) · 비례(4~10) · 상한+스크롤(10 초과).
    #[test]
    fn every_row_count_branch_fits() {
        for (cand, recent) in [(1, 0), (3, 2), (4, 4), (7, 3), (10, 10), (12, 5)] {
            assert_fits(cand, recent, 4.0);
        }
    }

    /// host UI zoom 이 `item_spacing` 을 바꿔도 덮어야 한다 — 형제 popup 이 옛 하드코딩
    /// 3.0 으로 4 px 부족했던 자리와 같은 축이다.
    #[test]
    fn every_ui_scale_fits() {
        for spacing in [3.40625_f32, 4.0, 4.78125] {
            assert_fits(3, 3, spacing);
            assert_fits(0, 0, spacing);
        }
    }

    /// ★ 이 결함의 본체 — heading 한 줄을 안 세면 그만큼 짧아진다.
    #[test]
    fn the_column_heading_is_counted() {
        let th = theme::theme();
        let spacing = 4.0;
        let without_heading = popup::title_bar_height()
            + popup::content_margin().scaled(2.0)
            + HEADER_HEIGHT
            + th.spacing_sm.scaled(2.0)
            + list_height_for(3, 3)
            + BUTTON_ROW_HEIGHT;
        let got = LogicalPx(picker_size_for(3, 3, spacing).y);
        assert!(
            got >= without_heading + HEADING_LINE_HEIGHT,
            "heading 을 안 세던 옛 식({without_heading:?})보다 최소 heading 한 줄만큼은 \
             커야 한다 — 지금 {got:?}"
        );
    }

    /// 빈 상태에는 heading 도 목록도 없다. 같은 식으로 뭉뚱그리면 목록 자리만큼 빈 여백이
    /// 남는다 — 잘리는 것의 반대 방향 결함이다.
    #[test]
    fn the_empty_branch_does_not_reserve_a_list() {
        let spacing = 4.0;
        let empty = LogicalPx(picker_size_for(0, 0, spacing).y);
        let listed = LogicalPx(picker_size_for(1, 0, spacing).y);
        assert!(
            empty < listed,
            "빈 상태({empty:?})가 항목 하나짜리({listed:?})보다 낮아야 한다"
        );
        // 두 갈래의 차가 정확히 "heading + 그 아래 간격 + 목록 최소 높이 − 안내문 한 줄"
        // 이어야 한다. 부등호만 보면 빈 갈래가 목록 자리를 **일부** 잡고 있어도 통과한다.
        let expected_gap =
            HEADING_LINE_HEIGHT + LogicalPx(spacing) + LIST_MIN_HEIGHT - EMPTY_LABEL_HEIGHT;
        assert_eq!(
            listed - empty,
            expected_gap,
            "빈 상태가 목록 자리를 잡아 두면 하단에 그만큼 빈 여백이 남는다"
        );
    }

    /// sizer 와 view 가 같은 목록 높이를 본다는 것 — 상한·하한이 실제로 문다.
    #[test]
    fn the_list_height_is_clamped_at_both_ends() {
        assert_eq!(
            list_height_for(1, 0),
            LIST_MIN_HEIGHT,
            "빈 자리도 4 행은 잡는다"
        );
        assert_eq!(
            list_height_for(99, 0),
            LIST_MAX_HEIGHT,
            "10 행을 넘으면 스크롤이다"
        );
        assert_eq!(
            list_height_for(7, 3),
            ITEM_HEIGHT.scaled(7.0),
            "긴 쪽이 높이를 정한다"
        );
    }
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

    let list_height = list_height_for(props.candidates.len(), props.recent.len());

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
