//! 마우스 캡처 배너의 더보기 메뉴. 대상 surface는 dialogs에 보관한다.
//! 알림 끄기는 배너를 닫고, 캡처 비활성화는 배너를 남긴다.
//! 프로그램 이름만 줄일 수 있도록 라벨과 이름을 나누어 배치한다.

use crate::adapters::ui::banner::BannerScope;
use crate::adapters::ui::icons;
use crate::adapters::ui::popup::PopupAction;
use crate::i18n::t;
use crate::intent::{OpenPopupMode, UiIntent};
use crate::settings::GeneralSettings;
use crate::state::AppState;
use crate::theme::{self, Theme};
use tasty_icons::Icon;

pub const MOUSE_CAPTURE_BANNER_MENU_POPUP_ID: crate::adapters::ui::popup::PopupId =
    "mouse_capture_banner_menu";

/// 고정된 두 항목의 높이와 패딩으로 메뉴 크기를 정한다.
pub fn menu_default_size() -> egui::Vec2 {
    let th = theme::theme();
    let row = th.menu_item_height().value();
    let pad = th.spacing_xs.value();
    egui::vec2(240.0, pad * 2.0 + row * 2.0)
}

/// 배너의 더보기 버튼에 맞춰 팝업을 연다. 아래 공간이 부족하면 위에 배치한다.
pub fn open(
    state: &mut AppState,
    ctx: &egui::Context,
    scope: &BannerScope,
    trigger_rect: egui::Rect,
) {
    let BannerScope::Surface(surface_id) = scope else {
        return;
    };
    state.dialogs.mouse_capture_banner_menu_target = Some(*surface_id);

    let size = menu_default_size();
    let offset = 4.0;
    let mut pos = egui::pos2(
        trigger_rect.right() - size.x,
        trigger_rect.bottom() + offset,
    );
    let screen = ctx.screen_rect();
    if pos.y + size.y > screen.bottom() {
        pos.y = trigger_rect.top() - size.y - offset;
    }

    state.dispatch_intent(
        UiIntent::OpenPopup {
            id: MOUSE_CAPTURE_BANNER_MENU_POPUP_ID,
            mode: OpenPopupMode::AtFocused(pos),
        }
        .from_user_menu("mouse_capture_banner"),
    );
}

/// 알림 억제 목록에 이름을 추가한다. 배너 닫기는 호출부에서 처리한다.
pub(crate) fn suppress_banner_action(settings: &mut GeneralSettings, app_name: &str) {
    settings
        .mouse_capture_banner_blacklist
        .push(app_name.to_string());
}

/// 캡처 비활성화 목록에 이름을 추가한다. 사용자가 확인할 수 있도록 배너는 남긴다.
pub(crate) fn disable_capture_action(settings: &mut GeneralSettings, app_name: &str) {
    settings.mouse_capture_blacklist.push(app_name.to_string());
}

/// 별도 저장 버튼이 없는 메뉴이므로 클릭한 설정을 즉시 저장한다.
fn persist_settings(engine: &mut crate::core::CoreState) {
    if let Err(e) = engine.settings.save() {
        tracing::warn!("failed to persist mouse capture blacklist: {e}");
    }
}

/// `PopupDef.draw_fn` — 메뉴 콘텐츠만 그린다(셸은 headless popup 시스템이 그림).
pub fn draw_menu(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }
    let Some(surface_id) = state.dialogs.mouse_capture_banner_menu_target else {
        return PopupAction::Close;
    };
    let th = theme::theme();
    let app_name = engine.foreground_name(surface_id).unwrap_or("").to_string();

    let suppress_resp = draw_menu_row(
        ui,
        &th,
        icons::BELL,
        t("popup.mouse_capture_banner_menu.suppress_prefix"),
        &app_name,
        t("popup.mouse_capture_banner_menu.suppress_suffix"),
    );
    let disable_resp = draw_menu_row(
        ui,
        &th,
        icons::MOUSE,
        t("popup.mouse_capture_banner_menu.disable_prefix"),
        &app_name,
        t("popup.mouse_capture_banner_menu.disable_suffix"),
    );

    if suppress_resp.clicked() {
        suppress_banner_action(&mut engine.settings.general, &app_name);
        persist_settings(engine);
        state.banners.close_shown_if_id(
            &BannerScope::Surface(surface_id),
            crate::adapters::ui::banner::defs::BANNER_MOUSE_CAPTURE,
        );
        return PopupAction::Close;
    }
    if disable_resp.clicked() {
        disable_capture_action(&mut engine.settings.general, &app_name);
        persist_settings(engine);
        return PopupAction::Close;
    }
    PopupAction::None
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

    #[test]
    fn mouse_capture_banner_menu_suppress_action_pushes_banner_blacklist() {
        let mut settings = GeneralSettings::default();
        suppress_banner_action(&mut settings, "vim");
        assert_eq!(
            settings.mouse_capture_banner_blacklist,
            vec!["vim".to_string()]
        );
        assert!(settings.mouse_capture_blacklist.is_empty());
    }

    #[test]
    fn mouse_capture_banner_menu_disable_action_pushes_capture_blacklist() {
        let mut settings = GeneralSettings::default();
        disable_capture_action(&mut settings, "vim");
        assert_eq!(settings.mouse_capture_blacklist, vec!["vim".to_string()]);
        assert!(settings.mouse_capture_banner_blacklist.is_empty());
    }
}
