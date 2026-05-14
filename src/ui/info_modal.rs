//! 정보 알림용 modal-like popup. 부팅 시점 fallback/에러 알림에 사용한다.
//!
//! 구조: 큐(`DialogState.info_modal_queue`)에 `InfoModal`을 push하고 popup을
//! 띄우면, 큐 head를 표시 + [확인]/Enter/Escape로 pop. 큐가 비면 popup 닫힘.
//!
//! 호출 패턴:
//! ```ignore
//! show_info_modal(state, InfoModal {
//!     title: "...".into(),
//!     body: "...".into(),
//!     on_close: InfoModalAction::Continue,
//! });
//! ```

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::{CONTENT_MARGIN, PopupAction, TITLE_BAR_HEIGHT};

/// 모달 [확인] 시 동작.
#[derive(Debug, Clone)]
pub enum InfoModalAction {
    /// 큐 처리 후 부팅/동작을 계속한다.
    Continue,
    /// 큐 처리 후 정상 종료. exit code는 best-effort (winit shutdown 후 process::exit).
    Exit(i32),
}

/// 큐에 들어가는 단일 메시지.
#[derive(Debug, Clone)]
pub struct InfoModal {
    pub title: String,
    pub body: String,
    pub on_close: InfoModalAction,
}

pub const INFO_MODAL_ID: &str = "info_modal";
const DEFAULT_WIDTH: f32 = 440.0;
const MIN_HEIGHT: f32 = 140.0;
const MAX_HEIGHT: f32 = 360.0;
const BODY_FONT_SIZE: f32 = 13.0;

/// 큐에 modal 한 건을 추가하고 popup을 연다. 이미 열려 있으면 큐만 추가.
pub fn show_info_modal(state: &mut AppState, modal: InfoModal) {
    state.dialogs.info_modal_queue.push_back(modal);
    if !state.popups.is_open(INFO_MODAL_ID) {
        state.popups.open_centered_focused(INFO_MODAL_ID);
    }
}

/// PopupDef.title_fn — 큐 head의 title을 popup 타이틀로 사용.
pub fn info_modal_title(state: &AppState) -> String {
    state
        .dialogs
        .info_modal_queue
        .front()
        .map(|m| m.title.clone())
        .unwrap_or_default()
}

/// PopupDef.sizer — body 길이에 따라 height를 조정.
pub fn info_modal_sizer(state: &AppState) -> egui::Vec2 {
    let body_len = state
        .dialogs
        .info_modal_queue
        .front()
        .map(|m| m.body.chars().count())
        .unwrap_or(0);
    // 대략 char당 1.6 line으로 가정 (한글/영문 혼합). 70 chars per line 기준.
    let approx_lines = (body_len as f32 / 60.0).ceil().max(2.0);
    let line_h = BODY_FONT_SIZE * 1.5;
    let body_h = approx_lines * line_h;
    let total_h = (TITLE_BAR_HEIGHT + CONTENT_MARGIN * 2.0 + body_h + 48.0)
        .clamp(MIN_HEIGHT, MAX_HEIGHT);
    egui::vec2(DEFAULT_WIDTH, total_h)
}

/// PopupDef.draw_fn — 큐 head를 보여주고 [확인]/Enter/Escape로 pop.
pub fn draw_info_modal(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    let Some(current) = state.dialogs.info_modal_queue.front().cloned() else {
        return PopupAction::Close;
    };

    let margin = 8.0;
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(margin, 4.0));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    ui.label(
        egui::RichText::new(&current.body)
            .color(th.text)
            .size(BODY_FONT_SIZE),
    );

    let mut confirm = ctx.input(|i| {
        i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape)
    });

    ui.with_layout(
        egui::Layout::bottom_up(egui::Align::RIGHT),
        |ui| {
            ui.add_space(4.0);
            if ui.button(t("button.ok")).clicked() {
                confirm = true;
            }
        },
    );

    if !confirm {
        return PopupAction::None;
    }

    // Pop the current head and act on its on_close.
    let popped = state.dialogs.info_modal_queue.pop_front();
    if let Some(modal) = popped {
        if let InfoModalAction::Exit(code) = modal.on_close {
            // 부팅 시점 fatal 알림에만 사용한다. winit destructor는 돌지 못하지만
            // 아직 PTY/plugin 등이 떠 있지 않은 시점이라 손실이 없다.
            tracing::info!("info modal exit requested (code={code})");
            std::process::exit(code);
        }
    }

    if state.dialogs.info_modal_queue.is_empty() {
        PopupAction::Close
    } else {
        // 다음 메시지로 이어진다. popup은 그대로 유지하되 title/size는 다음 프레임에
        // notification::draw_popups의 refresh 루프가 자동 갱신.
        PopupAction::None
    }
}
