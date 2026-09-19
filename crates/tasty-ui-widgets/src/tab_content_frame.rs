//! Tab content padding frame — 탭 내부 콘텐츠를 모달 테두리에서 일정
//! 거리 (`tokens::TAB_CONTENT_PADDING`) 만큼 띄우는 단순 wrapper.
//!
//! 본체 settings modal 의 ScrollArea 내부와 갤러리 `layout_2depth::draw_content`
//! 가 공통으로 사용.

use crate::tokens;

/// 4 면 동일 inner_margin (`TAB_CONTENT_PADDING`) 을 가진 빈 `Frame` 안에서
/// `content` 클로저를 실행한다.
pub fn tab_content_frame(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(tokens::TAB_CONTENT_PADDING))
        .show(ui, |ui| content(ui));
}

/// 설정 창 콘텐츠 컬럼 — `tab_content_frame` 의 패딩 안에서 폭을 `max_width` 로 막는다.
///
/// 상한을 **한 자리**에 두려고 이름을 붙였다. 서브탭마다 걸면 서브탭끼리 값이 갈리고,
/// 그때 갈린 것을 알아차릴 자리가 없다. full-bleed 서브탭은 이 컬럼을 자기 레이아웃으로
/// 대체하므로 애초에 이 함수를 안 지난다.
pub fn settings_content_column(
    ui: &mut egui::Ui,
    max_width: tasty_type_geometry::length::LogicalPx,
    content: impl FnOnce(&mut egui::Ui),
) {
    tab_content_frame(ui, |ui| {
        ui.set_max_width(ui.available_width().min(max_width.value()));
        content(ui);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_type_geometry::length::LogicalPx;

    /// 컬럼 안에서 콘텐츠가 실제로 받는 폭. 한 프레임을 돌려 그 자리의 `max_rect` 를 읽는다.
    fn column_width(available: f32, cap: LogicalPx) -> f32 {
        let ctx = egui::Context::default();
        let mut seen = 0.0;
        let _frame = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(available + 200.0, 600.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::Area::new(egui::Id::new("settings_content_column_test"))
                    .fixed_pos(egui::Pos2::ZERO)
                    .constrain(false)
                    .show(ctx, |ui| {
                        ui.set_max_width(available);
                        settings_content_column(ui, cap, |ui| seen = ui.max_rect().width());
                    });
            },
        );
        seen
    }

    /// 창이 넓으면 상한이 이긴다 — 그것이 이 컬럼이 존재하는 이유다.
    /// 창이 좁으면 남은 폭이 이긴다 — 상한은 최소 폭이 아니다.
    #[test]
    fn the_cap_binds_only_when_there_is_more_room_than_it() {
        let cap = LogicalPx(620.0);
        let wide = column_width(1000.0, cap);
        assert!(
            (wide - cap.value()).abs() < 0.5,
            "넓은 창에서 상한이 안 걸렸다 — 콘텐츠가 {wide}px 를 받았다"
        );

        let narrow = column_width(400.0, cap);
        assert!(
            narrow < cap.value(),
            "좁은 창에서 상한이 폭을 늘렸다 — 콘텐츠가 {narrow}px 를 받았다"
        );
        assert!(
            narrow > 0.0,
            "좁은 창에서 콘텐츠가 폭을 못 받았다 — {narrow}px"
        );
    }
}
