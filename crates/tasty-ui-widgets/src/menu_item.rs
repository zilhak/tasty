//! `MenuItem` — 메뉴/팔레트 행 (디자인 `components/navigation/MenuItem`).
//!
//! 전체폭 행: [icon] label [shortcut]. height control-height(28), pad space-md.
//! hover overlay-hover, active surface-active, danger accent-danger. 아이콘은
//! 호출측 [`IconPainter`] 로 주입(15px, text-muted / danger 면 accent-danger).

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;

const ICON_GLYPH: f32 = 15.0;

/// MenuItem variant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuItemVariant {
    Normal,
    Danger,
}

/// 전체폭 메뉴 행. `active` 면 surface-active 배경(현재 선택). 클릭 응답 반환.
#[allow(clippy::too_many_arguments)]
pub fn menu_item(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<IconPainter<'_>>,
    label: &str,
    shortcut: Option<&str>,
    variant: MenuItemVariant,
    active: bool,
    enabled: bool,
) -> egui::Response {
    let height = theme.item_height_interactive.value();
    let pad_x = theme.spacing_md.value();
    let gap = theme.spacing_sm.value();
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

    // 배경: active → surface-active, hover → overlay-hover.
    if active {
        ui.painter()
            .rect_filled(rect, radius, theme.surface_active().to_egui());
    } else if enabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, radius, theme.overlay_hover().to_egui_premultiplied());
    }

    let fg = match variant {
        MenuItemVariant::Normal => theme.text_primary().to_egui(),
        MenuItemVariant::Danger => theme.accent_danger().to_egui(),
    };
    let icon_color = match variant {
        MenuItemVariant::Normal => theme.subtext0.to_egui(),
        MenuItemVariant::Danger => theme.accent_danger().to_egui(),
    };

    let mut x = rect.left() + pad_x;
    if let Some(paint) = icon {
        let irect = egui::Rect::from_center_size(
            egui::pos2(x + ICON_GLYPH * 0.5, rect.center().y),
            egui::vec2(ICON_GLYPH, ICON_GLYPH),
        );
        paint(ui, irect, dim(icon_color));
        x += ICON_GLYPH + gap;
    }

    // shortcut (우측 정렬, mono 10.5 muted).
    let mut right = rect.right() - pad_x;
    if let Some(sc) = shortcut {
        let g = ui.painter().layout_no_wrap(
            sc.to_owned(),
            egui::FontId::monospace(10.5),
            egui::Color32::PLACEHOLDER,
        );
        let pos = egui::pos2(right - g.rect.width(), rect.center().y - g.rect.height() * 0.5);
        ui.painter().galley(pos, g.clone(), dim(theme.subtext0.to_egui()));
        right -= g.rect.width() + gap;
    }

    // label (좌측, 남은 폭 ellipsis 없이 clip).
    let g = ui.painter().layout_no_wrap(
        label.to_owned(),
        egui::FontId::proportional(body),
        egui::Color32::PLACEHOLDER,
    );
    let label_rect = egui::Rect::from_min_max(
        egui::pos2(x, rect.top()),
        egui::pos2(right.max(x), rect.bottom()),
    );
    let pos = egui::pos2(x, rect.center().y - g.rect.height() * 0.5);
    ui.painter().with_clip_rect(label_rect).galley(pos, g, dim(fg));

    resp
}

/// 메뉴 구분선 (디자인 `.tasty-menu-sep` — 1px separator, 상하 space-xs).
pub fn menu_separator(ui: &mut egui::Ui, theme: &Theme) {
    let xs = theme.spacing_xs.value();
    ui.add_space(xs);
    let r = ui.max_rect();
    let y = ui.cursor().top();
    ui.painter().hline(
        r.x_range(),
        y,
        egui::Stroke::new(theme.border_width.value(), theme.surface1.to_egui()),
    );
    ui.add_space(xs);
}
