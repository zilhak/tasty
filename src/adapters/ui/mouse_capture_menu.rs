//! 마우스 캡처 배너 "더보기"(⋯) 컨텍스트 메뉴 — headless `PopupDef`.
//!
//! 배너 우상단 ⋯ 트리거(`BannerManager::draw`)가 클릭되면 `notification::draw_popups`
//! 가 [`open`] 을 호출해 이 popup 을 연다. 대상 surface 는 `AppState.dialogs.
//! mouse_capture_banner_menu_target` 에 실린다(`RenameTarget` 과 동일 패턴 — popup
//! 자신은 `PopupDef.draw_fn` 시그니처상 대상 정보를 갖지 못한다).
//!
//! 메뉴 항목 2개(순서 고정, 디자인 시안 §메뉴 항목):
//! 1. "{app}에 대해 이 알림 끄기" → `mouse_capture_banner_blacklist` push + 배너 즉시 close.
//! 2. "{app}에 대해 마우스 캡처 비활성화" → `mouse_capture_blacklist` push (배너는 남음).
//!
//! 라벨은 고정 텍스트(줄바꿈/truncate 없음) + 프로그램 이름(mono, 축소+ellipsis) 두
//! 조각으로 렌더한다 — `t_fmt` 로 한 문자열에 합치면 en 로케일에서 프로그램 이름부터
//! 잘리므로(디자인 시안 근거) 두 조각을 독립 레이아웃한다.

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

/// 메뉴 한 줄 높이 × 2 + 내부 링 패딩(4px, `spacing_xs`) × 2. 폭은 디자인 시안의
/// min/max(200~288px) 범위 중간값 — 항목이 고정 2개뿐이라 동적 사이저 불필요.
pub fn menu_default_size() -> egui::Vec2 {
    let th = theme::theme();
    let row = th.menu_item_height().value();
    let pad = th.spacing_xs.value();
    egui::vec2(240.0, pad * 2.0 + row * 2.0)
}

/// 배너 "더보기" 트리거 클릭 시 호출 — 대상 surface 를 채우고, 트리거 버튼 rect
/// 기준으로 앵커(아래 4px, 우측 정렬 — 뷰포트 하단 공간 부족 시 위로 flip)를 계산해
/// popup 을 연다. mouse-capture 배너는 항상 `BannerScope::Surface`.
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

/// "이 알림 끄기" 액션 — 배너 억제 블랙리스트에 foreground 이름을 추가한다.
/// 배너를 닫는 건 호출자(`draw_menu`) 몫 — 이 함수는 설정 변경만 순수하게 담당해
/// 단위 테스트가 `AppState`/popup 없이도 직접 검증할 수 있다.
pub(crate) fn suppress_banner_action(settings: &mut GeneralSettings, app_name: &str) {
    settings
        .mouse_capture_banner_blacklist
        .push(app_name.to_string());
}

/// "마우스 캡처 비활성화" 액션 — 캡처 블랙리스트에 foreground 이름을 추가한다.
/// 배너는 닫지 않는다(디자인 확정값 — 캡처가 풀렸다는 걸 사용자가 읽고 직접 닫음).
pub(crate) fn disable_capture_action(settings: &mut GeneralSettings, app_name: &str) {
    settings.mouse_capture_blacklist.push(app_name.to_string());
}

/// Settings 모달의 명시적 Save 버튼(draft 커밋)과 달리, 배너 퀵 엔트리는 클릭 즉시
/// 확정되는 액션이라 `egui_panels.rs::A::SetViewMode` 와 동일하게 그 자리에서 디스크에
/// 반영한다 — 아니면 다음 재시작에 블랙리스트 추가가 사라진다.
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

/// 메뉴 한 줄 — `tasty_ui_widgets::menu_item` 과 동일한 시각 계약(28 control-height,
/// hover overlay, leading 아이콘)이지만 라벨을 [prefix][app(mono, ellipsis)][suffix]
/// 세 조각으로 독립 레이아웃한다. 전체 이름이 잘리면 tooltip 으로 보완한다.
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
