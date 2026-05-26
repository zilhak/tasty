//! 외부 drag&drop hover 중 표시되는 시각 피드백.
//!
//! `AppState.drop_hover` 가 활성인 동안 terminal_rect 위에 반투명 highlight +
//! "Drop to open" 라벨 + 1px 보더를 그린다. `HoveredFileCancelled` /
//! `DroppedFile` 직후 사라진다.

use crate::model::Rect;
use crate::state::AppState;

/// drop hover overlay 를 egui 프레임 마지막에 그린다 (popup 위, plugin popup 아래).
pub fn draw_drop_overlay(
    ctx: &egui::Context,
    state: &AppState,
    _engine: &crate::engine_state::EngineState,
    terminal_rect: Rect,
    scale_factor: f32,
) {
    let Some(hover) = state.drop_hover.as_ref() else {
        return;
    };
    if hover.paths.is_empty() {
        return;
    }

    let theme = crate::theme::theme();

    // 물리 픽셀 → 논리 픽셀 (egui 좌표계).
    let rect = egui::Rect::from_min_size(
        egui::pos2(
            terminal_rect.x.value() / scale_factor,
            terminal_rect.y.value() / scale_factor,
        ),
        egui::vec2(
            terminal_rect.width.value() / scale_factor,
            terminal_rect.height.value() / scale_factor,
        ),
    );

    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drop_overlay"));
    let painter = ctx.layer_painter(layer);

    // 반투명 fill — theme.blue 의 12% alpha.
    let fill = theme.blue.with_alpha(31).to_egui();
    painter.rect_filled(rect, theme.corner_radius.value(), fill);

    // 1px 보더 — theme.blue, alpha 0.6.
    let stroke = egui::Stroke::new(
        theme.border_width.value(),
        theme.blue.with_alpha(153).to_egui(),
    );
    painter.rect_stroke(
        rect.shrink(theme.spacing_sm.value()),
        theme.corner_radius.value(),
        stroke,
        egui::StrokeKind::Inside,
    );

    // 중앙 라벨.
    let label = if hover.paths.len() > 1 {
        format!(
            "{}  ({})",
            crate::i18n::t("file_drop.hover_label"),
            crate::i18n::t_fmt("file_drop.multi_files", &hover.paths.len().to_string()),
        )
    } else {
        crate::i18n::t("file_drop.hover_label").to_string()
    };

    let font = egui::FontId::proportional(theme.font_size_heading.value());
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        font,
        theme.text.to_egui().into(),
    );
}
