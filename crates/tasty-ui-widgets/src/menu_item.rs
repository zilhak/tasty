//! `MenuItem` — 메뉴/팔레트 행 (디자인 `components/navigation/MenuItem`).
//!
//! 전체폭 행: [icon] label [shortcut]. height control-height(28), pad space-md.
//! hover overlay-hover, active surface-active, danger accent-danger. 아이콘은
//! 호출측 [`IconPainter`] 로 주입(icon-size-md=16, text-muted / danger 면 accent-danger).

use tasty_type_appearance::theme::Theme;

use crate::icon_button::IconPainter;

/// MenuItem variant.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MenuItemVariant {
    Normal,
    Danger,
}

/// 일반 메뉴의 텍스트 단축키와 팔레트의 키캡 단축키를 구분한다.
enum Shortcut<'a> {
    Text(&'a str),
    Keys(&'a [&'a str]),
}

/// 전체폭 메뉴 행. `active` 면 surface-active 배경(현재 선택). 클릭 응답 반환.
#[allow(clippy::too_many_arguments)] // reason: 디자인 MenuItem 스펙(icon/label/shortcut/variant/active/enabled) 1:1 매핑, 인위적 그룹핑 불필요
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
    menu_item_inner(
        ui,
        theme,
        icon,
        label,
        shortcut.map(Shortcut::Text),
        variant,
        active,
        enabled,
    )
}

/// 단축키를 키캡으로 그린다. + 키를 구분자와 혼동하지 않도록 이미 나눈 목록을 받는다.
/// 비활성 행에서도 키캡은 흐려지지 않는다.
#[allow(clippy::too_many_arguments)] // reason: [`menu_item`] 과 같은 스펙, 단축키 표현만 다르다
pub fn menu_item_kbd(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<IconPainter<'_>>,
    label: &str,
    keys: &[&str],
    variant: MenuItemVariant,
    active: bool,
    enabled: bool,
) -> egui::Response {
    menu_item_inner(
        ui,
        theme,
        icon,
        label,
        (!keys.is_empty()).then_some(Shortcut::Keys(keys)),
        variant,
        active,
        enabled,
    )
}

#[allow(clippy::too_many_arguments)] // reason: 두 공개 함수가 공유하는 구현이며 두 함수에 필요한 인자를 받는다.
fn menu_item_inner(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Option<IconPainter<'_>>,
    label: &str,
    shortcut: Option<Shortcut<'_>>,
    variant: MenuItemVariant,
    active: bool,
    enabled: bool,
) -> egui::Response {
    let height = theme.menu_item_height().value();
    let pad_x = theme.menu_item_padding_x().value();
    // gap/label body/icon 글리프 = 대응 menu-item component 토큰 없음 → semantic.
    let gap = theme.spacing_sm.value();
    let radius = theme.menu_item_radius().value();
    let body = theme.font_size_body.value();
    let icon_glyph = theme.icon_glyph_size_md.value();
    let width = ui.available_width();

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    let dim = |c: egui::Color32| {
        if enabled {
            c
        } else {
            c.gamma_multiply(theme.opacity_disabled())
        }
    };

    if active {
        ui.painter()
            .rect_filled(rect, radius, theme.surface_active().to_egui());
    } else if enabled && resp.hovered() {
        ui.painter().rect_filled(
            rect,
            radius,
            theme.menu_item_bg_hover().to_egui_premultiplied(),
        );
    }

    // 일반 항목은 현재 text_primary를 사용한다. 디자인의 text-secondary와는 차이가 있다.
    let fg = match variant {
        MenuItemVariant::Normal => theme.text_primary().to_egui(),
        MenuItemVariant::Danger => theme.accent_danger().to_egui(),
    };
    let icon_color = match variant {
        MenuItemVariant::Normal => theme.text_muted().to_egui(),
        MenuItemVariant::Danger => theme.accent_danger().to_egui(),
    };

    let mut x = rect.left() + pad_x;
    if let Some(paint) = icon {
        let irect = egui::Rect::from_center_size(
            egui::pos2(x + icon_glyph * 0.5, rect.center().y),
            egui::vec2(icon_glyph, icon_glyph),
        );
        paint(ui, irect, dim(icon_color));
        x += icon_glyph + gap;
    }

    let mut right = rect.right() - pad_x;
    match shortcut {
        Some(Shortcut::Text(sc)) => {
            let g = ui.painter().layout_no_wrap(
                sc.to_owned(),
                egui::FontId::monospace(theme.menu_item_shortcut_font_size().value()),
                egui::Color32::PLACEHOLDER,
            );
            let pos = egui::pos2(
                right - g.rect.width(),
                rect.center().y - g.rect.height() * 0.5,
            );
            ui.painter()
                .galley(pos, g.clone(), dim(theme.text_muted().to_egui()));
            right -= g.rect.width() + gap;
        }
        Some(Shortcut::Keys(keys)) => {
            let parts: Vec<crate::KbdKey<'_>> =
                keys.iter().map(|k| crate::KbdKey::Text(k)).collect();
            let w = crate::kbd_parts_at(ui, theme, &parts, right, rect.center().y);
            right -= w.value() + gap;
        }
        None => {}
    }

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
    ui.painter()
        .with_clip_rect(label_rect)
        .galley(pos, g, dim(fg));

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
        egui::Stroke::new(theme.border_width.value(), theme.border_strong().to_egui()),
    );
    ui.add_space(xs);
}
