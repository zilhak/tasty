//! `TreeRow` — 사이드바/트리 행 (디자인 `components/navigation/TreeRow`).
//!
//! [chevron] [icon] label [meta]. height control-height-tree(22), depth 들여쓰기.
//! hover overlay-hover+text-primary, selected surface-active+text-primary(아이콘
//! accent-primary). chevron 은 has_children 일 때만, open 이면 90° 회전.

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;
use crate::tokens;

const CHEVRON_SLOT: f32 = 14.0;
const ICON_GLYPH: f32 = 14.0;
const INDENT_PER_DEPTH: f32 = 14.0;
const GAP: f32 = 6.0;

/// 트리 행. `selected` 면 surface-active. 클릭 응답 반환(행 전체 클릭).
#[allow(clippy::too_many_arguments)]
pub fn tree_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    depth: u16,
    has_children: bool,
    open: bool,
    icon: Option<IconPainter<'_>>,
    label: &str,
    meta: Option<&str>,
    selected: bool,
    enabled: bool,
) -> egui::Response {
    let height = tokens::TREE_ROW_HEIGHT;
    let pad_l = theme.spacing_xs.value();
    let pad_r = theme.spacing_sm.value();
    let radius = theme.corner_radius_sm.value();
    let body = theme.font_size_body.value();
    let width = ui.available_width();

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let dim = |c: egui::Color32| if enabled { c } else { c.gamma_multiply(0.45) };

    if selected {
        ui.painter()
            .rect_filled(rect, radius, theme.surface_active().to_egui());
    } else if enabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius, theme.overlay_hover().to_egui_premultiplied());
    }

    let fg = if selected || (enabled && resp.hovered()) {
        theme.text_primary().to_egui()
    } else {
        theme.text_secondary().to_egui()
    };
    let muted = theme.subtext0.to_egui();

    let mut x = rect.left() + pad_l + depth as f32 * INDENT_PER_DEPTH;

    // chevron (has_children 일 때만 — leaf 면 슬롯만 비움).
    let chev_c = egui::pos2(x + CHEVRON_SLOT * 0.5, rect.center().y);
    if has_children {
        let s = 3.0;
        // open → ▾ (아래), closed → ▸ (오른쪽).
        let pts = if open {
            vec![
                egui::pos2(chev_c.x - s, chev_c.y - s * 0.6),
                egui::pos2(chev_c.x, chev_c.y + s * 0.6),
                egui::pos2(chev_c.x + s, chev_c.y - s * 0.6),
            ]
        } else {
            vec![
                egui::pos2(chev_c.x - s * 0.6, chev_c.y - s),
                egui::pos2(chev_c.x + s * 0.6, chev_c.y),
                egui::pos2(chev_c.x - s * 0.6, chev_c.y + s),
            ]
        };
        ui.painter()
            .add(egui::Shape::line(pts, egui::Stroke::new(1.5, dim(muted))));
    }
    x += CHEVRON_SLOT + GAP;

    if let Some(paint) = icon {
        let icon_color = if selected {
            theme.accent_primary().to_egui()
        } else {
            muted
        };
        let irect = egui::Rect::from_center_size(
            egui::pos2(x + ICON_GLYPH * 0.5, rect.center().y),
            egui::vec2(ICON_GLYPH, ICON_GLYPH),
        );
        paint(ui, irect, dim(icon_color));
        x += ICON_GLYPH + GAP;
    }

    // meta (우측, mono 10 muted).
    let mut right = rect.right() - pad_r;
    if let Some(m) = meta {
        let g = ui.painter().layout_no_wrap(
            m.to_owned(),
            egui::FontId::monospace(10.0),
            egui::Color32::PLACEHOLDER,
        );
        let pos = egui::pos2(right - g.rect.width(), rect.center().y - g.rect.height() * 0.5);
        ui.painter().galley(pos, g.clone(), dim(muted));
        right -= g.rect.width() + GAP;
    }

    let g = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    let label_rect =
        egui::Rect::from_min_max(egui::pos2(x, rect.top()), egui::pos2(right.max(x), rect.bottom()));
    let pos = egui::pos2(x, rect.center().y - g.rect.height() * 0.5);
    ui.painter().with_clip_rect(label_rect).galley(pos, g, dim(fg));

    resp
}
