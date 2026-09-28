//! 부팅 오류 등을 큐에 담아 차례로 보여주는 안내 팝업.
//! 닫기 버튼·Enter·Escape로 현재 메시지를 닫고 큐가 비면 팝업도 닫는다.
//!
//! 본문과 버튼 행은 갤러리와 같은 `tasty_ui_widgets::info_modal`이 그린다. 제목은 팝업
//! 타이틀바에 있고, 셸 배경·타이틀바 글자 크기·선 색은 `popup/draw.rs`가 이 팝업 id로 고른다.

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::state::AppState;
use crate::theme;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::{ButtonVariant, InfoModalView};

#[derive(Debug, Clone)]
pub enum InfoModalAction {
    /// 큐 처리 후 부팅/동작을 계속한다.
    Continue,
    /// 큐 처리 후 정상 종료. exit code는 best-effort (winit shutdown 후 process::exit).
    Exit(i32),
}

#[derive(Debug, Clone)]
pub enum InfoModalButtonAction {
    /// Tasty의 권한 화면(설정 > 일반 > 권한)을 연다. 이 버튼을 눌러도 안내 모달은 열린
    /// 채로 남고 [확인]으로만 닫힌다. 설정 창은 별도 창이라 안내를 가릴 수 있어서, 창을
    /// 닫은 뒤에도 무엇을 왜 허용해야 하는지 다시 읽을 수 있어야 하기 때문이다.
    ///
    /// 팝업을 그리는 코드는 `AppState`만 가지고 있어 winit 이벤트 루프에 접근할 수 없다.
    /// 그래서 `dialogs.permission_settings_requested`만 표시해 두고 App 계층이 이를 읽어
    /// 창을 연다.
    // 이유: 이 값을 만드는 곳이 macOS 전용 안내 하나뿐이라 다른 OS 빌드에는 생성처가 없다.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    OpenPermissionSettings,
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
    /// 닫기 버튼 왼쪽에 그려지는 추가 버튼들. 비어 있으면 닫기 버튼 하나만 나온다.
    pub extra_buttons: Vec<InfoModalButton>,
    /// 본문의 강조 표기(`**도입부**` · `*경로*` · `` `명령` ``)를 해석한다. 강조를 직접 쓴
    /// 메시지(현재 macOS 권한 안내)만 켠다.
    pub emphasis: bool,
    /// 닫기 버튼 라벨. 없으면 `button.ok`다. 앱을 끝내는 메시지는 그 동작을 라벨로 쓴다.
    pub dismiss_label: Option<String>,
}

pub const INFO_MODAL_ID: &str = "info_modal";

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

/// 셸 높이는 직전 프레임에 잰 본문 높이로 정한다. 처음 여는 프레임은 아직 잰 값이 없어
/// 글자 수로 줄 수를 어림한다. 어림이 틀려도 다음 프레임에 맞춰진다.
pub fn info_modal_sizer(state: &AppState, _engine: &crate::core::CoreState) -> egui::Vec2 {
    let th = theme::theme();
    let body_h = state.dialogs.info_modal_body_height.unwrap_or_else(|| {
        let (chars, paragraphs) = state
            .dialogs
            .info_modal_queue
            .front()
            .map(|m| (m.body.chars().count(), m.body.split("\n\n").count()))
            .unwrap_or((0, 1));
        // 셸 폭 440에서 본문 13px 한 줄에 들어가는 글자 수의 어림값.
        const APPROX_CHARS_PER_LINE: usize = 60;
        let lines = chars.div_ceil(APPROX_CHARS_PER_LINE).max(1) as f32 + paragraphs as f32;
        let line_h = th.font_size_body.value() * th.line_height_ui;
        LogicalPx(lines * line_h)
            + th.info_modal_para_gap() * paragraphs.saturating_sub(1) as f32
            + th.spacing_md * 2.0
    });
    let h = tasty_ui_widgets::info_modal_shell_height(&th, popup::title_bar_height(), body_h);
    egui::vec2(th.info_modal_width().value(), h.value())
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
    state.dialogs.info_modal_body_height = None;
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

    let mut buttons: Vec<tasty_ui_widgets::InfoModalButton<'_>> = current
        .extra_buttons
        .iter()
        .map(|b| tasty_ui_widgets::InfoModalButton {
            label: &b.label,
            variant: ButtonVariant::Secondary,
        })
        .collect();
    buttons.push(tasty_ui_widgets::InfoModalButton {
        label: current.dismiss_label.as_deref().unwrap_or(t("button.ok")),
        variant: ButtonVariant::Primary,
    });
    let out = tasty_ui_widgets::info_modal(
        ui,
        &th,
        &InfoModalView {
            // 다음 메시지가 앞 메시지의 스크롤 위치를 물려받지 않도록 메시지마다 id를 바꾼다.
            id_salt: egui::Id::new(INFO_MODAL_ID).with((&current.title, &current.body)),
            body: &current.body,
            emphasis: current.emphasis,
            buttons: &buttons,
            scroll_to: None,
        },
    );
    state.dialogs.info_modal_body_height = Some(out.body_content_height);

    let dismiss_idx = buttons.len() - 1;
    let confirm = out.clicked == Some(dismiss_idx)
        || ctx.input(|i| i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Escape));
    let mut open_permission_settings = false;
    if let Some(i) = out.clicked
        && let Some(button) = current.extra_buttons.get(i)
    {
        match button.action {
            InfoModalButtonAction::OpenPermissionSettings => open_permission_settings = true,
        }
    }

    if open_permission_settings {
        state.dialogs.permission_settings_requested = true;
    }

    if !confirm {
        return PopupAction::None;
    }

    let popped = state.dialogs.info_modal_queue.pop_front();
    state.dialogs.info_modal_body_height = None;
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
