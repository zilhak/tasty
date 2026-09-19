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

/// MenuItem 우측 단축키 자리의 두 표현.
///
/// 디자인에서 이 자리는 한 종류가 아니다 — 메뉴는 mono micro muted **텍스트** 한
/// 덩이이고(`menu-item-shortcut-font-size`), 커맨드 팔레트는 키별 **`Kbd` 키캡**이다
/// (`components/core/Kbd`). 그래서 `menu_item` 의 인자 타입을 바꾸는 대신 표현을 둘로
/// 갈라 각자의 문을 둔다 — 텍스트 쪽 호출자 아홉 자리는 그대로다.
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

/// [`menu_item`] 의 **키캡 판** — 단축키를 키별 `Kbd` 로 그린다(커맨드 팔레트 관례).
///
/// `keys` 는 이미 나뉜 토큰이다(`["Ctrl", "T"]`) — `"ctrl++"` 같은 조합에서 `+` 가
/// 구분자인지 키인지 모호해지므로 문자열을 여기서 쪼개지 않는다.
///
/// 키캡은 `enabled=false` 에서도 흐려지지 않는다. 지금 이 문을 쓰는 자리는 팔레트
/// 하나이고 거기 행은 늘 enabled 라, 안 쓰는 상태를 미리 그리지 않는다.
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

#[allow(clippy::too_many_arguments)] // reason: 두 공개 문이 공유하는 한 구현, 인자는 그 둘의 합집합이다
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
    // 아이콘 글리프 = icon-size-md(16). (token-policy: 15 → 16 snap.)
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

    // 배경: active → surface-active, hover → overlay-hover.
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

    // fg: Normal 은 구현이 text_primary 를 쓴다(디자인 menu-item-fg=text-secondary 와
    // 불일치 → 픽셀 diff 0 위해 이식 제외). Danger accent-danger·아이콘색도 대응
    // component 토큰 없어 semantic 유지.
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

    // shortcut (우측 정렬) — 텍스트는 mono micro(10) muted, 키캡은 공용 `Kbd`.
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
