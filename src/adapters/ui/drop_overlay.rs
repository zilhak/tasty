//! 외부 drag&drop hover 중 표시되는 시각 피드백.
//!
//! `AppState.drop_hover` 가 활성인 동안 terminal_rect 위에 반투명 highlight +
//! "Drop to open" 라벨 + 1px 보더를 그린다. `HoveredFileCancelled` /
//! `DroppedFile` 직후 사라진다.

use crate::model::PhysicalRect;
use crate::state::AppState;

/// drop hover overlay 를 egui 프레임 마지막에 그린다 (popup 위, plugin popup 아래).
pub fn draw_drop_overlay(
    ctx: &egui::Context,
    state: &AppState,
    _engine: &crate::core::CoreState,
    terminal_rect: PhysicalRect,
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
    let rect = crate::adapters::ui::to_egui_rect(terminal_rect, scale_factor);

    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("drop_overlay"));
    let painter = ctx.layer_painter(layer);

    // 반투명 fill — accent_primary 의 12% alpha. 대응 토큰이 없어 이름만 둔다.
    const OVERLAY_FILL_ALPHA: u8 = 31;
    let fill = theme
        .accent_primary()
        .with_alpha(OVERLAY_FILL_ALPHA)
        .to_egui();
    painter.rect_filled(rect, theme.corner_radius.value(), fill);

    // 1px 보더 — accent_primary, alpha 0.6.
    const OVERLAY_BORDER_ALPHA: u8 = 153;
    let stroke = egui::Stroke::new(
        theme.border_width.value(),
        theme
            .accent_primary()
            .with_alpha(OVERLAY_BORDER_ALPHA)
            .to_egui(),
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
        theme.text_primary().to_egui(),
    );
}
