//! 부팅 오류 등을 큐에 담아 차례로 보여주는 안내 팝업.
//! 확인·Enter·Escape로 현재 메시지를 닫고 큐가 비면 팝업도 닫는다.

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::vspace;

#[derive(Debug, Clone)]
pub enum InfoModalAction {
    /// 큐 처리 후 부팅/동작을 계속한다.
    Continue,
    /// 큐 처리 후 정상 종료. exit code는 best-effort (winit shutdown 후 process::exit).
    Exit(i32),
}

#[derive(Debug, Clone)]
pub enum InfoModalButtonAction {
    /// OS 설정 등 외부 주소를 열되 안내는 계속 볼 수 있도록 팝업을 유지한다.
    // 이유: 현재 생성처가 macOS의 Full Disk Access 안내뿐이라 다른 플랫폼에서는 사용되지 않는다.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    OpenExternal(String),
}

#[derive(Debug, Clone)]
pub struct InfoModalButton {
    pub label: String,
    pub action: InfoModalButtonAction,
}

#[derive(Debug, Clone)]
pub struct InfoModal {
    pub title: String,
    pub body: String,
    pub on_close: InfoModalAction,
    /// [확인] 왼쪽에 그려지는 추가 버튼들. 비어 있으면 [확인] 하나만 나온다.
    pub extra_buttons: Vec<InfoModalButton>,
}

pub const INFO_MODAL_ID: &str = "info_modal";
const DEFAULT_WIDTH: LogicalPx = LogicalPx(440.0);
const MIN_HEIGHT: LogicalPx = LogicalPx(140.0);
const MAX_HEIGHT: LogicalPx = LogicalPx(360.0);
/// 고정된 버튼 행 높이. 갤러리는 토큰으로 계산하므로 gallery_copied_dimensions로 차이를 검사한다.
const FOOTER_ROOM: LogicalPx = LogicalPx(48.0);

/// 안내를 큐에 추가한다. 부팅 안내는 에이전트 요청이 아니므로 사용자 입력을 받는 팝업으로 연다.
pub fn show_info_modal(state: &mut AppState, modal: InfoModal) {
    state.dialogs.info_modal_queue.push_back(modal);
    state.dispatch_intent(
        crate::intent::UiIntent::OpenPopup {
            id: INFO_MODAL_ID,
            mode: crate::intent::OpenPopupMode::CenteredFocused,
        }
        .from_user_menu("info_modal"),
    );
}

pub fn info_modal_title(state: &AppState, _engine: &crate::core::CoreState) -> String {
    state
        .dialogs
        .info_modal_queue
        .front()
        .map(|m| m.title.clone())
        .unwrap_or_default()
}

pub fn info_modal_sizer(state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    let body_len = state
        .dialogs
        .info_modal_queue
        .front()
        .map(|m| m.body.chars().count())
        .unwrap_or(0);
    // 문자 수로 대략 높이를 정한다. 실제 줄바꿈과 다르면 본문 스크롤로 처리한다.
    let approx_lines = (body_len as f32 / 60.0).ceil().max(2.0);
    let line_h = theme::theme().font_size_body.value() * 1.5;
    let body_h = approx_lines * line_h;
    let total_h = (popup::title_bar_height()
        + popup::content_margin().scaled(2.0)
        + LogicalPx(body_h)
        + FOOTER_ROOM)
        .min(MAX_HEIGHT)
        .max(MIN_HEIGHT);
    egui::vec2(DEFAULT_WIDTH.value(), total_h.value())
}

/// 확인 버튼 외의 닫기 경로도 큐를 처리한다. 확인이 이미 큐를 비웠으면 다시 꺼내지 않는다.
pub fn on_close_info_modal(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    let Some(modal) = state.dialogs.info_modal_queue.pop_front() else {
        return;
    };
    if let InfoModalAction::Exit(code) = modal.on_close {
        tracing::info!("info modal exit requested (code={code})");
        std::process::exit(code);
    }
    if !state.dialogs.info_modal_queue.is_empty() {
        // intent-exempt: popup 자기-close cleanup — 이 함수가 on_close 훅이라 여기서 큐의 다음 항목을 잇는다
        state.popups.open_centered_focused(INFO_MODAL_ID);
    }
}

pub fn draw_info_modal(
    ui: &mut egui::Ui,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) -> PopupAction {
    let th = theme::theme();
    let ctx = ui.ctx().clone();

    let Some(current) = state.dialogs.info_modal_queue.front().cloned() else {
        return PopupAction::Close;
    };

    let margin = th.spacing_sm.value();
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(margin, th.spacing_xs.value()));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    // 예상보다 긴 본문이 버튼을 밀어내지 않도록 버튼 높이를 확보하고 나머지를 스크롤한다.
    let body_max_h = (ui.available_height() - FOOTER_ROOM.value()).max(0.0);
    egui::ScrollArea::vertical()
        .max_height(body_max_h)
        .auto_shrink([false, true])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.label(
                egui::RichText::new(&current.body)
                    .color(th.text_primary())
                    .size(th.font_size_body.value()),
            );
        });

    let mut confirm =
        ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));

    ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
        vspace(ui, th.spacing_xs);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button(t("button.ok")).clicked() {
                confirm = true;
            }
            for button in &current.extra_buttons {
                if ui.button(&button.label).clicked() {
                    match &button.action {
                        InfoModalButtonAction::OpenExternal(url) => open_external(url),
                    }
                }
            }
        });
    });

    if !confirm {
        return PopupAction::None;
    }

    let popped = state.dialogs.info_modal_queue.pop_front();
    if let Some(modal) = popped
        && let InfoModalAction::Exit(code) = modal.on_close
    {
        // 즉시 종료하므로 winit을 포함한 남은 객체의 destructor는 실행하지 않는다.
        tracing::info!("info modal exit requested (code={code})");
        std::process::exit(code);
    }

    if state.dialogs.info_modal_queue.is_empty() {
        PopupAction::Close
    } else {
        PopupAction::None
    }
}

/// 렌더를 막지 않도록 외부 URL/스킴을 열고 프로세스 완료는 기다리지 않는다.
fn open_external(url: &str) {
    #[cfg(debug_assertions)]
    if crate::platform::debug_os_open::intercepted("open_external", url) {
        return;
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(windows)]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/c", "start", ""]);
        c
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut cmd = std::process::Command::new("xdg-open");

    if let Err(err) = cmd.arg(url).spawn() {
        tracing::warn!(%err, url, "info modal: 외부 링크 열기 실패");
    }
}
