//! 워크스페이스 강제 끊기 destructive 확인 다이얼로그.
//!
//! 사이드바 워크스페이스 행 우클릭 -> "강제 끊기" 가 연다. 즉시 끊지 않고 취소/강제
//! 끊기 2버튼 confirm 을 한 번 거친다 — 원격 사용자의 세션을 끊는 비가역 행동이고
//! 우클릭은 오조작이 쉬운 자리다. 폼은 `confirm_delete_category` 와 같은 380px
//! destructive confirm 이고, 확인 버튼은 danger(`accent_danger` + `text_on_accent`),
//! 헤더 글리프는 `CLOSE`(서피스 오버레이의 강제 끊기 버튼과 같은 글리프)다.
//!
//! ## 왜 두 번째 진입점인가
//!
//! GUI 의 기존 해제 수단은 점유된 **서피스** 우상단 버튼 하나뿐인데, 그 버튼은 활성
//! 워크스페이스의 pane 만 순회하는 오버레이에 그려진다 — 즉 **그 워크스페이스로
//! 전환해야만 보인다.** 사이드바에서 점유 표시를 보고 있는 사용자는 그 자리에서 할 수
//! 있는 것이 없었다. `PopupScope::Window` + `OpenPopupMode::CenteredFocused` 로 여는
//! 것이 그래서 필수다 — `PopupScope::Workspace` 로 두면 대상으로 전환해야 보여
//! 목적이 무너진다.
//!
//! ## 본문에 점유자를 안 적는다
//!
//! 서버는 transport 를 모르고 항상 loopback 으로 받는다 — SSH 터널 너머든 아니든
//! 서버 입장에선 전부 같은 주소다. 들고 있는 것은 숫자 `AttachClientId` 뿐이라
//! 사용자에게 보여도 뜻이 없다. 그래서 본문은 **대상 워크스페이스 이름 + 끊었을 때의
//! 결과**로 쓴다.
//!
//! `state.dialogs.pending_force_detach_workspace` 가 대상 워크스페이스 **id** 를 든다.
//! 그 워크스페이스가 사라졌거나 **점유가 이미 풀렸으면 즉시 닫힌다** — 메뉴를 연 뒤
//! 원격이 스스로 release 했거나 EOF/TTL 로 회수됐을 수 있다.

use crate::adapters::ui::icons;
use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::{t, t_fmt};
use crate::state::AppState;
use crate::theme;
use tasty_type_geometry::length::LogicalPx;

pub const CONFIRM_FORCE_DETACH_WORKSPACE_POPUP_ID: &str = "confirm_force_detach_workspace";

/// 다이얼로그 폭 (destructive confirm 공통 380px).
const WIDTH: LogicalPx = LogicalPx(380.0);

/// 대상 워크스페이스 스냅샷. 그리기에 필요한 것은 이름뿐이다 — 끊는 행동은
/// [`apply_force_detach`] 가 보류 id 에서 다시 읽는다(팝업이 열려 있는 동안 대상이
/// 바뀔 수 있으니 두 자리가 같은 값을 두 번 들지 않게 한다).
struct Target {
    name: String,
}

/// `pending_force_detach_workspace` 의 대상을 해석. 워크스페이스가 없거나 **점유가 이미
/// 풀렸으면** None(닫힘). 점유를 함께 보는 것이 요점이다 — 이 팝업이 열려 있는 동안
/// 원격이 스스로 끊을 수 있고, 그때 확인 버튼은 아무 대상도 없는 행동이 된다.
fn resolve_target(state: &AppState, engine: &crate::core::CoreState) -> Option<Target> {
    let ws_id = state.dialogs.pending_force_detach_workspace?;
    let ws = engine.workspaces.iter().find(|w| w.id == ws_id)?;
    engine.attach.workspace_holder(ws_id)?;
    Some(Target {
        name: ws.name.clone(),
    })
}

/// PopupDef.title_fn — headless 라 실제 타이틀바는 없지만, 접근성/디버그용 라벨.
pub fn confirm_force_detach_workspace_title(
    _state: &AppState,
    _engine: &crate::core::CoreState,
) -> String {
    t("attach.force_detach_confirm_title").to_string()
}

/// 대상 이름을 뺀 안내문 자체의 길이. 대상이 아직 안 잡힌 상태의 기준이기도 하다.
const BASE_BODY_LEN: usize = 90;

/// 본문 길이 → 팝업 크기. **sizer 와 등록 placeholder 가 같은 식을 쓴다.**
///
/// 등록 값을 손으로 적으면 sizer 와 어긋날 자리가 생기고, 그 어긋남은 첫 프레임의
/// 깜빡임으로만 드러난다. 형제 `file_handler_picker` 가 같은 이유로 같은 형태다.
fn size_for(body_len: usize) -> egui::Vec2 {
    let approx_lines = (body_len as f32 / 42.0).ceil().max(2.0);
    let body_h = approx_lines * theme::theme().font_size_body.value() * 1.5;
    // 헤더(글리프+제목) + 본문 + 버튼 행 + 여백.
    let content_h = 24.0 + body_h + 40.0;
    egui::vec2(
        WIDTH.value(),
        (popup::content_margin().scaled(2.0) + LogicalPx(content_h)).value(),
    )
}

/// PopupDef.default_size — 등록 시점의 placeholder. 대상이 아직 없을 때 sizer 가 내는
/// 값과 **같은 식에서** 나온다.
pub fn confirm_force_detach_workspace_default_size() -> egui::Vec2 {
    size_for(BASE_BODY_LEN)
}

/// PopupDef.sizer — 본문 길이에 따라 height 조정(소형 모달).
pub fn confirm_force_detach_workspace_sizer(
    state: &AppState,
    engine: &crate::core::CoreState,
) -> egui::Vec2 {
    let body_len = resolve_target(state, engine)
        .map(|tgt| tgt.name.chars().count() + BASE_BODY_LEN)
        .unwrap_or(BASE_BODY_LEN);
    size_for(body_len)
}

/// PopupDef::on_close entry point — 어떤 경로로 닫히든(취소/외부/Escape) 대상을 비운다.
pub fn on_close_confirm_force_detach_workspace(
    _ctx: &egui::Context,
    state: &mut AppState,
    _engine: &mut crate::core::CoreState,
) {
    state.dialogs.pending_force_detach_workspace = None;
}

pub fn draw_confirm_force_detach_workspace(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    let ctx = ui.ctx().clone();
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.dialogs.pending_force_detach_workspace = None;
        return PopupAction::Close;
    }
    let Some(target) = resolve_target(state, engine) else {
        state.dialogs.pending_force_detach_workspace = None;
        return PopupAction::Close;
    };
    let th = theme::theme();

    let margin = th.spacing_sm.value();
    let available = ui.available_rect_before_wrap();
    let inner_rect = available.shrink2(egui::vec2(margin, th.spacing_xs.value()));
    let mut child_ui = ui.new_child(egui::UiBuilder::new().max_rect(inner_rect));
    let ui = &mut child_ui;

    // ── 헤더: close danger 글리프 + "점유를 끊을까요?" (semibold). ──
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = th.spacing_sm.value();
        let icon_size = th.icon_glyph_size_md.value();
        let (icon_rect, _) =
            ui.allocate_exact_size(egui::vec2(icon_size, icon_size), egui::Sense::hover());
        icons::CLOSE
            .image(icon_size, th.accent_danger().into())
            .paint_at(ui, icon_rect);
        ui.label(
            egui::RichText::new(t("attach.force_detach_confirm_title"))
                .color(th.text_primary())
                .size(th.font_size_body.value())
                .strong(),
        );
    });

    ui.add_space(th.spacing_sm.value());

    // ── 본문: 대상 이름 + 끊었을 때의 결과. 점유자 식별자는 안 적는다(모듈 문서). ──
    ui.label(
        egui::RichText::new(t_fmt("attach.force_detach_confirm_body", &target.name))
            .color(th.text_secondary())
            .size(th.font_size_body.value()),
    );

    // ── 푸터: 우측 정렬 Cancel(ghost) + Force detach(danger). ──
    let mut confirm = false;
    let mut cancel = false;
    ui.with_layout(egui::Layout::bottom_up(egui::Align::RIGHT), |ui| {
        ui.add_space(th.spacing_sm.value());
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Force detach — danger 채움 버튼.
                let go = egui::Button::new(
                    egui::RichText::new(t("attach.force_detach")).color(th.text_on_accent()),
                )
                .fill(th.accent_danger());
                if ui.add(go).clicked() {
                    confirm = true;
                }
                if ui.button(t("button.cancel")).clicked() {
                    cancel = true;
                }
            });
        });
    });

    if cancel {
        state.dialogs.pending_force_detach_workspace = None;
        return PopupAction::Close;
    }
    if confirm {
        apply_force_detach(state, engine);
        return PopupAction::Close;
    }
    PopupAction::None
}

/// 확인 버튼이 하는 일 — 보류 대상의 점유를 끊고 보류를 비운다. 반환은 실제로 끊긴
/// holder(이미 풀려 있었으면 `None`).
///
/// 그리기에서 떼어 둔 이유는 **이 자리를 시험할 다른 길이 없기 때문**이다. popup 닫힘
/// 하네스(`state::popup_close_tests`)는 프레임마다 `egui::Context` 를 새로 만들어,
/// egui 의 위젯 클릭 판정이 기대는 이전 프레임 기억이 없다 — Escape 와 바깥 클릭은
/// 그 기억 없이도 도는 경로라 시험되지만, 버튼 클릭은 그 하네스에서 재현되지 않는다.
/// 함수로 두면 확인이 무엇을 하는지가 클릭 재현과 무관하게 값으로 남는다.
pub(crate) fn apply_force_detach(
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> Option<crate::core::attach::AttachClientId> {
    let holder = state
        .dialogs
        .pending_force_detach_workspace
        .and_then(|ws_id| engine.attach.force_detach_workspace(ws_id));
    if holder.is_none() {
        tracing::debug!("force_detach_workspace: nothing to detach");
    }
    state.dialogs.pending_force_detach_workspace = None;
    holder
}
