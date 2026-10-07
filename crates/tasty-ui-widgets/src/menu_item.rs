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
    /// 글자가 `menu-item-fg`(text-secondary)이고 호버·선택 때 `menu-item-fg-hover`(text-primary)로 밝아진다.
    /// 디자인 Tools menu 행(`tasty-toolsmenu-item`)이 이 색을 쓴다.
    Secondary,
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
    let icon_glyph = theme.icon_glyph_size_md.value();
    let width = ui.available_width();

    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), sense);
    // disabled 항목의 아이콘·라벨·단축키 문구는 opacity 없이 disabled ink를 쓴다.
    let dim = |c: egui::Color32| {
        if enabled {
            c
        } else {
            theme.state_disabled_fg().to_egui()
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
        MenuItemVariant::Secondary if active || (enabled && resp.hovered()) => {
            theme.menu_item_fg_hover().to_egui()
        }
        MenuItemVariant::Secondary => theme.menu_item_fg().to_egui(),
        MenuItemVariant::Danger => theme.accent_danger().to_egui(),
    };
    let icon_color = match variant {
        MenuItemVariant::Normal | MenuItemVariant::Secondary => theme.text_muted().to_egui(),
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
            let ink = (!enabled).then(|| theme.state_disabled_fg().to_egui());
            let w = crate::chip::kbd_parts_at_ink(ui, theme, &parts, right, rect.center().y, ink);
            right -= w.value() + gap;
        }
        None => {}
    }

    // 디자인 `.tasty-menuitem__label` 처럼 남은 폭을 넘는 라벨은 끝을 말줄임표로 줄인다.
    let ink = dim(fg);
    let g = menu_label_galley(ui, theme, label, ink, right - x);
    let pos = egui::pos2(x, rect.center().y - g.rect.height() * 0.5);
    ui.painter().galley(pos, g, ink);

    resp
}

/// 아이콘·단축키 없는 메뉴 행을 내용 폭에 맞춘 메뉴의 border-box 폭.
/// 가장 넓은 라벨 + 행 패딩(`menu-item-padding-x`) 양쪽 + 안쪽 고리(`popup-content-margin`) 양쪽
/// + 테두리 양쪽을 `min`..`max` 로 제한한다. 상한에 걸린 라벨은 [`menu_label_galley`] 가 끝을 줄인다.
pub fn fit_menu_width<'a>(
    ctx: &egui::Context,
    theme: &Theme,
    labels: impl IntoIterator<Item = &'a str>,
    min: f32,
    max: f32,
) -> f32 {
    let font = egui::FontId::proportional(theme.font_size_body.value());
    let widest = ctx.fonts(|f| {
        labels
            .into_iter()
            .map(|l| {
                f.layout_no_wrap(l.to_owned(), font.clone(), egui::Color32::PLACEHOLDER)
                    .rect
                    .width()
            })
            .fold(0.0_f32, f32::max)
    });
    let chrome = theme.menu_item_padding_x().value() * 2.0
        + theme.popup_content_margin().value() * 2.0
        + theme.border_width.value() * 2.0;
    (widest.ceil() + chrome).clamp(min, max.max(min))
}

/// 한 줄 메뉴 라벨. `max_width` 를 넘으면 끝을 말줄임표로 줄인다.
pub fn menu_label_galley(
    ui: &egui::Ui,
    theme: &Theme,
    label: &str,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(
        label.to_owned(),
        egui::FontId::proportional(theme.font_size_body.value()),
        color,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(max_width.max(0.0));
    ui.painter().layout_job(job)
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

/// 내용 폭 메뉴가 가장 넓은 라벨에 행 패딩·안쪽 고리·테두리를 더해 하한·상한으로 제한하는지,
/// 상한에 걸린 라벨의 끝이 줄어드는지 검사한다.
#[cfg(test)]
mod fit_width_tests {
    use super::{fit_menu_width, menu_label_galley};
    use egui::RawInput;
    use tasty_type_appearance::theme::Theme;

    const LONG: &str = "A plugin tool label that is far too long for any menu to show in full";
    /// 시험용 좁은 행 폭. LONG 라벨보다 좁다.
    const NARROW: f32 = 120.0;

    fn theme() -> Theme {
        Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0)
    }

    fn label_width(ctx: &egui::Context, th: &Theme, label: &str) -> f32 {
        ctx.fonts(|f| {
            f.layout_no_wrap(
                label.to_owned(),
                egui::FontId::proportional(th.font_size_body.value()),
                egui::Color32::PLACEHOLDER,
            )
            .rect
            .width()
        })
    }

    /// 폰트가 준비된 Context 에서 `f` 를 실행한다. 첫 프레임은 폰트 준비 전이라 두 번 돌린다.
    fn with_ui<R>(mut f: impl FnMut(&egui::Context, &egui::Ui) -> R) -> R {
        let ctx = egui::Context::default();
        let mut out = None;
        for _ in 0..2 {
            // 프레임 출력(그린 도형)은 이 검사에 필요 없다. 측정값은 `out`으로 받는다.
            let frame = ctx.run(RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| out = Some(f(ctx, ui)));
            });
            drop(frame);
        }
        out.expect("frame ran")
    }

    #[test]
    fn menu_width_is_the_widest_row_clamped_between_min_and_max() {
        let th = theme();
        let (min, max) = (
            th.tools_menu_min_width().value(),
            th.tools_menu_max_width().value(),
        );
        let chrome = th.menu_item_padding_x().value() * 2.0
            + th.popup_content_margin().value() * 2.0
            + th.border_width.value() * 2.0;
        let (short, mid_label, mid, long) = with_ui(|ctx, _| {
            let mid_label = "Remote connections… extra";
            (
                fit_menu_width(ctx, &th, ["Git", "Presets"], min, max),
                label_width(ctx, &th, mid_label),
                fit_menu_width(ctx, &th, ["Git", mid_label], min, max),
                fit_menu_width(ctx, &th, ["Git", LONG], min, max),
            )
        });
        assert_eq!(short, min);
        assert!(
            mid_label.ceil() + chrome > min && mid_label.ceil() + chrome < max,
            "{mid_label} + {chrome} not in ({min}, {max})"
        );
        assert_eq!(mid, mid_label.ceil() + chrome);
        assert_eq!(long, max);
    }

    /// 공용 행이 남은 폭보다 넓은 라벨을 잘라 내지 않고 끝을 말줄임표로 줄여 행 안에 그리는지 검사한다.
    #[test]
    fn a_menu_item_row_ellipsizes_an_overflowing_label() {
        let th = theme();
        let ctx = egui::Context::default();
        let mut texts: Vec<(String, egui::Rect)> = Vec::new();
        let mut row_rect = egui::Rect::NOTHING;
        for _ in 0..2 {
            let frame = ctx.run(RawInput::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.allocate_ui(egui::vec2(NARROW, NARROW), |ui| {
                        ui.set_max_width(NARROW);
                        row_rect = super::menu_item(
                            ui,
                            &th,
                            None,
                            LONG,
                            None,
                            super::MenuItemVariant::Secondary,
                            false,
                            true,
                        )
                        .rect;
                    });
                });
            });
            texts = frame
                .shapes
                .iter()
                .filter_map(|c| match &c.shape {
                    egui::Shape::Text(t) => Some((
                        t.galley
                            .rows
                            .iter()
                            .flat_map(|r| r.glyphs.iter().map(|g| g.chr))
                            .collect(),
                        t.visual_bounding_rect(),
                    )),
                    _ => None,
                })
                .collect();
        }
        let (text, rect) = texts
            .iter()
            .find(|(t, _)| t.starts_with('A'))
            .expect("label drawn");
        assert!(text.ends_with('…'), "{text}");
        assert!(
            rect.right() <= row_rect.right() - th.menu_item_padding_x().value() + 0.5,
            "{rect:?} past {row_rect:?}"
        );
    }

    #[test]
    fn a_label_past_the_cap_ends_in_an_ellipsis() {
        let th = theme();
        let (full, cut, cut_w, fit) = with_ui(|ctx, ui| {
            let cut = menu_label_galley(ui, &th, LONG, egui::Color32::WHITE, 120.0);
            let fit = menu_label_galley(ui, &th, "Git", egui::Color32::WHITE, 120.0);
            (
                label_width(ctx, &th, LONG),
                cut.rows
                    .iter()
                    .flat_map(|r| r.glyphs.iter().map(|g| g.chr))
                    .collect::<String>(),
                cut.rect.width(),
                fit.rows
                    .iter()
                    .flat_map(|r| r.glyphs.iter().map(|g| g.chr))
                    .collect::<String>(),
            )
        });
        assert!(full > 120.0);
        assert!(cut_w <= 120.0, "{cut_w}");
        assert!(cut.ends_with('…'), "{cut}");
        assert_eq!(fit, "Git");
    }
}
