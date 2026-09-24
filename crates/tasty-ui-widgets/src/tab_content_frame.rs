//! 탭 안쪽 여백과 설정 콘텐츠의 최대 폭을 적용한다.

use crate::tokens;

/// 4 면 동일 inner_margin (`TAB_CONTENT_PADDING`) 을 가진 빈 `Frame` 안에서
/// `content` 클로저를 실행한다.
pub fn tab_content_frame(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .inner_margin(egui::Margin::same(tokens::TAB_CONTENT_PADDING))
        .show(ui, |ui| content(ui));
}

/// 여백 안의 콘텐츠 폭을 제한한다. 전체 폭을 쓰는 하위 탭은 자체 레이아웃을 사용한다.
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

    /// 넓은 창은 상한을 따르고 좁은 창은 남은 폭보다 커지지 않아야 한다.
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
