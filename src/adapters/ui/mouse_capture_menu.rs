//! 마우스 캡처 배너의 더보기 메뉴. 대상 surface는 dialogs에 보관한다.
//! 알림 끄기는 배너를 닫고, 캡처 비활성화는 배너를 남긴다.
//! 프로그램 이름만 줄일 수 있도록 라벨과 이름을 나누어 배치한다.

use crate::adapters::ui::banner::BannerScope;
use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::intent::{OpenPopupMode, UiIntent};
use crate::state::MainViewState;
use crate::theme::{self, Theme};
use tasty_icons::Icon;
use tasty_type_geometry::length::LogicalPx;

pub const MOUSE_CAPTURE_BANNER_MENU_POPUP_ID: crate::adapters::ui::popup::PopupId =
    "mouse_capture_banner_menu";

/// 셸 크기. 폭은 테두리를 뺀 안쪽 폭이다(셸 테두리는 바깥으로 그린다). 높이는 두 행과 위아래 패딩이다.
fn menu_size_for(th: &Theme, inner_width: LogicalPx) -> egui::Vec2 {
    let row = th.menu_item_height().value();
    let pad = th.banner_more_menu_padding().value();
    egui::vec2(inner_width.value(), pad * 2.0 + row * 2.0)
}

/// 시안의 메뉴 폭은 테두리까지 포함한 border-box다. 두 행 중 넓은 내용에 맞추고
/// `banner-more-menu-min-width`..`banner-more-menu-max-width` 로 제한한 뒤 테두리를 뺀 셸 폭을 돌려준다.
/// 상한에 걸리면 프로그램 이름만 줄인다(`draw_menu_row`).
fn menu_inner_width(th: &Theme, widest_row: f32) -> LogicalPx {
    let pad = th.banner_more_menu_padding().value();
    let bw = th.border_width.value();
    let outer = (widest_row + pad * 2.0 + bw * 2.0).clamp(
        th.banner_more_menu_min_width().value(),
        th.banner_more_menu_max_width().value(),
    );
    LogicalPx(outer - bw * 2.0)
}

/// 등록 시점의 크기. 대상이 정해지기 전이라 하한 폭을 쓴다.
pub fn menu_default_size() -> egui::Vec2 {
    let th = theme::theme();
    let min = th.banner_more_menu_min_width().value() - th.border_width.value() * 2.0;
    menu_size_for(&th, LogicalPx(min))
}

/// `PopupDef.sizer` — 열 때 잰 폭을 쓴다. 매 프레임 호출되므로 글꼴 측정은 열 때 한 번만 한다.
pub fn menu_sizer(
    state: &MainViewState,
    _engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> egui::Vec2 {
    let th = theme::theme();
    match state.dialogs.mouse_capture_banner_menu_width {
        Some(width) => menu_size_for(&th, width),
        None => menu_default_size(),
    }
}

/// 한 행을 줄이지 않고 그리는 데 필요한 폭. `draw_menu_row` 와 같은 배치를 잰다.
fn row_content_width(
    ctx: &egui::Context,
    th: &Theme,
    prefix: &str,
    app_name: &str,
    suffix: &str,
) -> f32 {
    let body = th.font_size_body.value();
    let text_w = ctx.fonts(|f| {
        let w = |text: &str, font: egui::FontId| {
            f.layout_no_wrap(text.to_owned(), font, egui::Color32::PLACEHOLDER)
                .rect
                .width()
        };
        w(prefix, egui::FontId::proportional(body))
            + w(app_name, egui::FontId::monospace(body))
            + w(suffix, egui::FontId::proportional(body))
    });
    th.menu_item_padding_x().value() * 2.0
        + th.icon_glyph_size_md.value()
        + th.spacing_sm.value()
        + text_w
}

/// 두 행 중 넓은 쪽의 내용 폭.
fn widest_row(ctx: &egui::Context, th: &Theme, app_name: &str) -> f32 {
    let suppress = row_content_width(
        ctx,
        th,
        t("popup.mouse_capture_banner_menu.suppress_prefix"),
        app_name,
        t("popup.mouse_capture_banner_menu.suppress_suffix"),
    );
    let disable = row_content_width(
        ctx,
        th,
        t("popup.mouse_capture_banner_menu.disable_prefix"),
        app_name,
        t("popup.mouse_capture_banner_menu.disable_suffix"),
    );
    suppress.max(disable)
}

/// 배너의 더보기 버튼에 맞춰 팝업을 연다. 아래 공간이 부족하면 위에 배치한다.
pub fn open(
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
    ctx: &egui::Context,
    scope: &BannerScope,
    trigger_rect: egui::Rect,
) {
    let BannerScope::Surface(surface_id) = scope else {
        return;
    };
    state.dialogs.mouse_capture_banner_menu_target = Some(*surface_id);

    let th = theme::theme();
    let app_name = engine.foreground_name(*surface_id).unwrap_or("");
    let width = menu_inner_width(&th, widest_row(ctx, &th, app_name));
    state.dialogs.mouse_capture_banner_menu_width = Some(width);
    let size = menu_size_for(&th, width);
    // 셸 테두리는 바깥으로 그리므로 border-box 의 오른쪽 끝이 트리거 오른쪽에 맞도록 테두리만큼 당긴다.
    let bw = th.border_width.value();
    let offset = th.banner_more_menu_offset().value();
    let mut pos = egui::pos2(
        trigger_rect.right() - bw - size.x,
        trigger_rect.bottom() + offset + bw,
    );
    let screen = ctx.screen_rect();
    if pos.y + size.y + bw > screen.bottom() {
        pos.y = trigger_rect.top() - offset - bw - size.y;
    }

    state.dispatch_intent(
        UiIntent::OpenPopup {
            id: MOUSE_CAPTURE_BANNER_MENU_POPUP_ID,
            mode: OpenPopupMode::AtFocused(pos),
        }
        .from_user_menu("mouse_capture_banner"),
    );
}

/// `PopupDef.draw_fn` — 메뉴 콘텐츠만 그린다(셸은 headless popup 시스템이 그림).
pub fn draw_menu(
    ui: &mut egui::Ui,
    state: &mut MainViewState,
    engine: &crate::runtime::engine_read::EngineRead<'_>,
) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }
    let Some(surface_id) = state.dialogs.mouse_capture_banner_menu_target else {
        return PopupAction::Close;
    };
    let th = theme::theme();
    let app_name = engine.foreground_name(surface_id).unwrap_or("").to_string();
    let (suppress_resp, disable_resp) = draw_menu_rows(ui, &th, &app_name);

    if suppress_resp.clicked() {
        state.dispatch_intent(
            crate::intent::Intent::PatchSettings(
                crate::app::engine_action::SettingsPatch::SuppressMouseHint(app_name.clone()),
            )
            .from_user_context_menu(),
        );
        state.banners.close_shown_if_id(
            &BannerScope::Surface(surface_id),
            crate::adapters::ui::banner::defs::BANNER_MOUSE_CAPTURE,
        );
        return PopupAction::Close;
    }
    if disable_resp.clicked() {
        state.dispatch_intent(
            crate::intent::Intent::PatchSettings(
                crate::app::engine_action::SettingsPatch::DisableMouseCapture(app_name),
            )
            .from_user_context_menu(),
        );
        return PopupAction::Close;
    }
    PopupAction::None
}

/// 두 행을 그린다. 행은 menu-item-height 간격으로 붙인다. 셸 높이(`menu_size_for`)도 간격 없이 잰다.
fn draw_menu_rows(
    ui: &mut egui::Ui,
    th: &Theme,
    app_name: &str,
) -> (egui::Response, egui::Response) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let suppress = draw_menu_row(
        ui,
        th,
        icons::BELL,
        t("popup.mouse_capture_banner_menu.suppress_prefix"),
        app_name,
        t("popup.mouse_capture_banner_menu.suppress_suffix"),
    );
    let disable = draw_menu_row(
        ui,
        th,
        icons::MOUSE,
        t("popup.mouse_capture_banner_menu.disable_prefix"),
        app_name,
        t("popup.mouse_capture_banner_menu.disable_suffix"),
    );
    (suppress, disable)
}

/// 앞 문구·프로그램 이름·뒤 문구를 따로 배치한다. 이름이 잘리면 툴팁으로 보여 준다.
fn draw_menu_row(
    ui: &mut egui::Ui,
    theme: &Theme,
    icon: Icon,
    prefix: &str,
    app_name: &str,
    suffix: &str,
) -> egui::Response {
    let height = theme.menu_item_height().value();
    let pad_x = theme.menu_item_padding_x().value();
    let gap = theme.spacing_sm.value();
    let radius = theme.menu_item_radius().value();
    let body = theme.font_size_body.value();
    let icon_glyph = theme.icon_glyph_size_md.value();
    let width = ui.available_width();

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());

    if resp.hovered() {
        ui.painter().rect_filled(
            rect,
            radius,
            theme.menu_item_bg_hover().to_egui_premultiplied(),
        );
    }

    let mut x = rect.left() + pad_x;
    let irect = egui::Rect::from_center_size(
        egui::pos2(x + icon_glyph * 0.5, rect.center().y),
        egui::vec2(icon_glyph, icon_glyph),
    );
    icon.image(icon_glyph, theme.text_muted().to_egui())
        .paint_at(ui, irect);
    x += icon_glyph + gap;

    let right = rect.right() - pad_x;
    let avail_text_w = (right - x).max(0.0);

    let font = egui::FontId::proportional(body);
    let prefix_galley =
        ui.painter()
            .layout_no_wrap(prefix.to_owned(), font.clone(), egui::Color32::PLACEHOLDER);
    let suffix_galley =
        ui.painter()
            .layout_no_wrap(suffix.to_owned(), font, egui::Color32::PLACEHOLDER);
    let fixed_w = prefix_galley.rect.width() + suffix_galley.rect.width();
    let app_max_w = (avail_text_w - fixed_w).max(0.0);

    let app_font = egui::FontId::monospace(body);
    let mut app_text = app_name.to_string();
    let mut app_galley = ui.painter().layout_no_wrap(
        app_text.clone(),
        app_font.clone(),
        egui::Color32::PLACEHOLDER,
    );
    if app_galley.rect.width() > app_max_w {
        while app_text.chars().count() > 1 {
            app_text.pop();
            let candidate = format!("{app_text}…");
            let g = ui.painter().layout_no_wrap(
                candidate.clone(),
                app_font.clone(),
                egui::Color32::PLACEHOLDER,
            );
            if g.rect.width() <= app_max_w {
                app_galley = g;
                app_text = candidate;
                break;
            }
        }
    }
    let truncated = app_text != app_name;

    let y = rect.center().y;
    let fg = theme.text_primary().to_egui();
    let mut cx = x;
    if !prefix.is_empty() {
        ui.painter().galley(
            egui::pos2(cx, y - prefix_galley.rect.height() * 0.5),
            prefix_galley.clone(),
            fg,
        );
        cx += prefix_galley.rect.width();
    }
    ui.painter().galley(
        egui::pos2(cx, y - app_galley.rect.height() * 0.5),
        app_galley.clone(),
        fg,
    );
    cx += app_galley.rect.width();
    if !suffix.is_empty() {
        ui.painter().galley(
            egui::pos2(cx, y - suffix_galley.rect.height() * 0.5),
            suffix_galley.clone(),
            fg,
        );
    }

    if truncated {
        resp.on_hover_text(app_name.to_string())
    } else {
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테두리까지 포함한 바깥 폭. 셸 폭에 바깥으로 그리는 테두리 두 줄을 더한다.
    fn outer(th: &Theme, widest: f32) -> f32 {
        menu_inner_width(th, widest).value() + th.border_width.value() * 2.0
    }

    /// 기본 item_spacing 이 0 이 아닌 Ui 에서도 두 행이 붙어 셸 높이의 행 부분과 같다.
    #[test]
    fn the_two_rows_sit_flush_at_the_menu_item_height() {
        let th = theme::theme();
        let ctx = egui::Context::default();
        let mut rows = None;
        let _frame = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                assert!(ui.spacing().item_spacing.y > 0.0);
                let (a, b) = draw_menu_rows(ui, &th, "vim");
                rows = Some((a.rect, b.rect));
            });
        });
        let (a, b) = rows.expect("rows drawn");
        assert_eq!(b.top() - a.top(), th.menu_item_height().value());
        assert_eq!(b.bottom() - a.top(), th.menu_item_height().value() * 2.0);
    }

    #[test]
    fn a_short_row_takes_the_min_width() {
        let th = theme::theme();
        assert_eq!(outer(&th, 0.0), th.banner_more_menu_min_width().value());
    }

    #[test]
    fn a_long_row_stops_at_the_max_width() {
        let th = theme::theme();
        let past_max = th.banner_more_menu_max_width().value() * 2.0;
        assert_eq!(
            outer(&th, past_max),
            th.banner_more_menu_max_width().value()
        );
    }

    #[test]
    fn a_row_between_the_bounds_adds_the_padding_and_border() {
        let th = theme::theme();
        let min = th.banner_more_menu_min_width().value();
        let max = th.banner_more_menu_max_width().value();
        let widest = (min + max) * 0.5;
        let expected =
            widest + th.banner_more_menu_padding().value() * 2.0 + th.border_width.value() * 2.0;
        assert!(expected < max, "the sample row must stay under the cap");
        assert_eq!(outer(&th, widest), expected);
    }

    #[test]
    fn the_shell_keeps_the_width_and_fits_two_rows_and_the_padding() {
        let th = theme::theme();
        let width =
            LogicalPx(th.banner_more_menu_max_width().value() - th.border_width.value() * 2.0);
        let size = menu_size_for(&th, width);
        assert_eq!(size.x, width.value());
        assert_eq!(
            size.y,
            th.banner_more_menu_padding().value() * 2.0 + th.menu_item_height().value() * 2.0
        );
    }
}
