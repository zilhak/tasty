//! `TreeRow` — 사이드바/트리 행 (디자인 `components/navigation/TreeRow`).
//!
//! [chevron] [icon] label [meta]. height control-height-tree(22), depth 들여쓰기.
//! hover overlay-hover+text-primary, selected surface-active+text-primary(아이콘
//! accent-primary). chevron 은 has_children 일 때만, open 이면 90° 회전.
//! disabled 상태는 없다. 열 수 없는 항목도 일반 행으로 두고, 여는 쪽이 이유를 알린다.

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;

const CHEVRON_SLOT: f32 = 14.0;
const ICON_GLYPH: f32 = 14.0;
const GAP: f32 = 6.0;

/// 깊이 0 행의 왼쪽 끝에서 아이콘 가운데까지의 거리. 같은 목록에 끼는 다른 행이 아이콘을 맞출 때 쓴다.
pub fn tree_row_icon_center(theme: &Theme) -> f32 {
    theme.tree_row_gap().value() + CHEVRON_SLOT + GAP + ICON_GLYPH * 0.5
}

/// 아이콘이 있는 깊이 0 행의 왼쪽 끝에서 이름 글자가 시작하는 곳까지의 거리. 이름 뒤에 표지를 붙일 때 쓴다.
pub fn tree_row_label_left(theme: &Theme) -> f32 {
    theme.tree_row_gap().value() + CHEVRON_SLOT + GAP + ICON_GLYPH + GAP
}

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
) -> egui::Response {
    tree_row_matching(
        ui,
        theme,
        depth,
        has_children,
        open,
        icon,
        label,
        "",
        meta,
        selected,
    )
}

/// `tree_row` 와 같고, 이름에서 `query` 와 맞는 부분(대소문자 무시)을 explorer-match-fg 로 칠한다.
#[allow(clippy::too_many_arguments)] // 이유: tree_row 의 인자에 검색어 하나를 더했다 — 같은 순서를 지켜 호출부를 맞바꿀 수 있게 한다
pub fn tree_row_matching(
    ui: &mut egui::Ui,
    theme: &Theme,
    depth: u16,
    has_children: bool,
    open: bool,
    icon: Option<IconPainter<'_>>,
    label: &str,
    query: &str,
    meta: Option<&str>,
    selected: bool,
) -> egui::Response {
    let height = theme.tree_row_height().value();
    let pad_l = theme.tree_row_gap().value();
    let pad_r = theme.spacing_sm.value();
    let radius = theme.corner_radius_sm.value();
    let body = theme.tree_row_font_size().value();
    let width = ui.available_width();

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
    let indent_per_depth = theme.tree_row_indent().value();

    if selected {
        ui.painter()
            .rect_filled(rect, radius, theme.tree_row_bg_active().to_egui());
    } else if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            radius,
            theme.tree_row_bg_hover().to_egui_premultiplied(),
        );
    }

    let fg = if selected || resp.hovered() {
        theme.tree_row_fg_active().to_egui()
    } else {
        theme.tree_row_fg().to_egui()
    };
    let muted = theme.text_muted().to_egui();

    let mut x = rect.left() + pad_l + depth as f32 * indent_per_depth;

    let chev_c = egui::pos2(x + CHEVRON_SLOT * 0.5, rect.center().y);
    if has_children {
        let s = 3.0;
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
        ui.painter().add(egui::Shape::line(
            pts,
            egui::Stroke::new(theme.icon_stroke_width.value(), muted),
        ));
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
        paint(ui, irect, icon_color);
        x += ICON_GLYPH + GAP;
    }

    let mut right = rect.right() - pad_r;
    if let Some(m) = meta {
        let g = ui.painter().layout_no_wrap(
            m.to_owned(),
            egui::FontId::monospace(theme.tree_row_meta_font_size().value()),
            egui::Color32::PLACEHOLDER,
        );
        let pos = egui::pos2(
            right - g.rect.width(),
            rect.center().y - g.rect.height() * 0.5,
        );
        ui.painter().galley(pos, g.clone(), muted);
        right -= g.rect.width() + GAP;
    }

    let g = ui.painter().layout_job(crate::explorer_match_job(
        theme,
        label,
        query,
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    ));
    let label_rect = egui::Rect::from_min_max(
        egui::pos2(x, rect.top()),
        egui::pos2(right.max(x), rect.bottom()),
    );
    let pos = egui::pos2(x, rect.center().y - g.rect.height() * 0.5);
    ui.painter().with_clip_rect(label_rect).galley(pos, g, fg);

    resp
}
