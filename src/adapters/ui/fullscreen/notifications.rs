//! 알림 무대 — 전체화면 무대의 **첫 실콘텐츠 소비자**.
//!
//! ## 왜 알림 popup 을 첫 소비자로 골랐나
//!
//! 무대는 popup **인스턴스**가 아니라 "같은 형상의 별개 콘텐츠" 를 올린다
//! (`docs/design/systems/fullscreen-stage.md` §모델 1). 따라서 첫 소비자는 draw
//! 로직을 popup 기하(`PopupState` 의 pos/size/drag)와 **무관하게** 호출할 수 있는
//! popup 이어야 한다. 후보 중 알림 popup 만이 그 조건을 이미 만족했다:
//!
//! - 형상 함수 [`draw_notification_content_inner`](crate::adapters::ui::notification::draw_notification_content_inner)
//!   가 **이미 분리돼 있다** — popup 의 `draw_fn` 은 그것을 호출하고 `PopupAction`
//!   을 붙이는 얇은 껍데기뿐이라, 무대도 같은 함수를 그대로 부르면 된다(형상 재사용
//!   세 갈래 중 "공통 함수 추출" — 추출이 이미 끝나 있었다).
//! - popup 자신의 UI 상태가 없다(`PopupDef.on_close: None`). 도메인
//!   (`engine.notifications`)만 읽으므로 무대 인스턴스가 popup 상태를 건드릴 여지가
//!   구조적으로 없다.
//! - 목록형이라 넓은 화면에서 실익이 분명하다(한 화면에 보이는 알림 수가 늘어난다).
//! - 타이틀바가 있는 popup 이라 전체화면 버튼을 놓을 자리가 있다(headless 패널
//!   popup 은 타이틀바가 없어 버튼을 달 수 없다).
//!
//! ## 무대 자체 상태 = 스크롤 위치
//!
//! 도메인은 공유하지만 **스크롤 위치는 무대 인스턴스의 것**이다. 셸이 콘텐츠 Ui 를
//! 무대 id 로 salt 하므로 안쪽 `ScrollArea` 의 egui state id 가 popup 쪽과 달라져,
//! 무대에서 목록을 내려도 원본 popup 의 스크롤은 그대로다(= "별개 데이터").
//!
//! 그 상태는 무대 수명에 속하므로 [`on_close`] 에서 지운다 — popup 의 `on_close`
//! 관례([ADR-0063](../../../../docs/adr/0063-popup-close-hook-single-choke-point.md))
//! 를 무대 정의가 그대로 따른다. 실제 id 는 draw 시점의 Ui 에서만 알 수 있으므로
//! 그때 temp memory 에 적어 두고 훅이 그것을 읽어 지운다(egui 내부 id 규칙을
//! 재현하지 않는다).

use super::StageAction;

/// 알림 무대 id. `PopupDef.fullscreen_stage` 와 debug IPC 가 이 값을 가리킨다.
pub(crate) use crate::fullscreen_stages::NOTIFICATIONS_STAGE_ID;

/// 콘텐츠 자체 상태(스크롤)의 egui id 를 적어 두는 temp memory 슬롯.
fn scroll_id_slot() -> egui::Id {
    egui::Id::new("fullscreen.notifications.scroll_id")
}

/// `draw_notification_content_inner` 안쪽 `ScrollArea` 가 쓰는 state id.
/// egui 는 salt 를 주지 않은 ScrollArea 의 id 를 `ui.id().with("scroll_area")` 로
/// 만든다 — 그 규칙을 여기서 재현하는 대신, draw 가 자기 Ui 로 계산한 값을 적어
/// 두고 훅이 읽는다(규칙이 바뀌면 아무것도 못 지우는 대신 오동작하지 않는다).
fn content_scroll_id(ui: &egui::Ui) -> egui::Id {
    ui.make_persistent_id(egui::Id::new("scroll_area"))
}

/// 무대 콘텐츠 — popup 과 같은 형상(프레임 + 목록)을 무대 rect 전체에 그린다.
/// 타이틀·닫기 버튼은 그리지 않는다: 제목과 종료 수단은 무대 셸이 공통 제공한다
/// (`super::draw_exit_button`).
pub(crate) fn draw(
    ui: &mut egui::Ui,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
) -> StageAction {
    let th = crate::theme::theme();
    let frame = ui.max_rect();
    let painter = ui.painter().clone();

    // 프레임 — 호스트 popup 셸(`popup::draw`)의 배경/보더와 같은 토큰.
    painter.rect_filled(frame, th.corner_radius.value(), th.surface_raised());
    painter.rect_stroke(
        frame,
        th.corner_radius.value(),
        egui::Stroke::new(th.border_width.value(), th.border_strong()),
        egui::StrokeKind::Outside,
    );

    // 타이틀바는 **다시 그리지 않는다** — 무대 셸이 이미 무대 제목을 그렸고
    // (`super::draw_fullscreen_stage`), 종료 버튼도 셸이 공통 제공한다. 여기서 popup
    // 타이틀바까지 옮겨오면 같은 제목이 두 줄로 겹치고 닫기 버튼이 두 개가 된다.
    // 재사용하는 "형상" 은 프레임(배경/보더)과 콘텐츠이고, 타이틀바는 셸 chrome 과
    // 역할이 겹치는 부분이다.

    // 콘텐츠 — popup 과 **같은** 형상 함수. 무대가 popup 기하를 하나도 넘기지 않는
    // 것이 이 함수가 무대에서도 성립하는 근거다.
    let content_rect = frame.shrink(crate::adapters::ui::popup::content_margin());
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    let scroll_id = content_scroll_id(&content);
    content
        .ctx()
        .memory_mut(|m| m.data.insert_temp(scroll_id_slot(), scroll_id));
    crate::adapters::ui::notification::draw_notification_content_inner(&mut content, state, engine);

    StageAction::None
}

/// 무대 콘텐츠가 기록해 둔 스크롤 상태 id. 테스트가 "무대 자체 상태" 의 존재와
/// 정리를 단정할 때 읽는다(런타임 경로는 [`on_close`] 안에서 직접 읽는다).
#[cfg(test)]
pub(crate) fn recorded_scroll_id(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.memory(|m| m.data.get_temp::<egui::Id>(scroll_id_slot()))
}

/// 무대 자체 상태(스크롤 위치) 정리. 어떤 경로로 닫히든 정확히 1 회 발화한다.
pub(crate) fn on_close(
    ctx: &egui::Context,
    _state: &mut crate::state::AppState,
    _engine: &mut crate::core::CoreState,
) {
    ctx.memory_mut(|m| {
        if let Some(id) = m.data.get_temp::<egui::Id>(scroll_id_slot()) {
            m.data.remove::<egui::containers::scroll_area::State>(id);
        }
        m.data.remove::<egui::Id>(scroll_id_slot());
    });
}
