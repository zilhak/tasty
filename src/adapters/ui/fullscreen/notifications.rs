//! 알림 팝업과 같은 내용 함수를 전체화면에서도 사용한다.
//! 알림 데이터는 공유하지만 스크롤 위치는 화면별 egui ID로 분리한다.
//! 그릴 때 스크롤 ID를 저장하고 on_close에서 임시 상태를 지운다.

use super::StageAction;

pub(crate) use crate::fullscreen_stages::NOTIFICATIONS_STAGE_ID;

fn scroll_id_slot() -> egui::Id {
    egui::Id::new("fullscreen.notifications.scroll_id")
}

/// 콘텐츠 Ui에서 ScrollArea와 같은 ID를 계산한다. egui의 ID 규칙이 바뀌면 이 계산도 확인해야 한다.
fn content_scroll_id(ui: &egui::Ui) -> egui::Id {
    ui.make_persistent_id(egui::Id::new("scroll_area"))
}

/// 알림 프레임과 목록만 그린다. 제목과 종료 버튼은 공용 전체화면 코드가 제공한다.
pub(crate) fn draw(
    ui: &mut egui::Ui,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
) -> StageAction {
    let th = crate::theme::theme();
    let frame = ui.max_rect();
    let painter = ui.painter().clone();

    painter.rect_filled(frame, th.corner_radius.value(), th.surface_raised());
    painter.rect_stroke(
        frame,
        th.corner_radius.value(),
        egui::Stroke::new(th.border_width.value(), th.border_strong()),
        egui::StrokeKind::Outside,
    );

    let content_rect = frame.shrink(crate::adapters::ui::popup::content_margin().value());
    let mut content = ui.new_child(egui::UiBuilder::new().max_rect(content_rect));
    let scroll_id = content_scroll_id(&content);
    content
        .ctx()
        .memory_mut(|m| m.data.insert_temp(scroll_id_slot(), scroll_id));
    crate::adapters::ui::notification::draw_notification_content_inner(&mut content, state, engine);

    StageAction::None
}

#[cfg(test)]
pub(crate) fn recorded_scroll_id(ctx: &egui::Context) -> Option<egui::Id> {
    ctx.memory(|m| m.data.get_temp::<egui::Id>(scroll_id_slot()))
}

/// 이 화면의 스크롤 상태를 정리한다.
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
