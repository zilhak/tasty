//! 현재 포커스된 surface의 정보를 읽어 공용 상태바에 전달하고 클릭 동작을 처리한다.
//! 상태바 레이어·위치는 본체에서 정하며 갤러리는 같은 공용 화면 함수를 사용한다.
//! 표시 항목: docs/features/workspace-status-bar/index.md.

use egui::emath::GuiRounding as _;
use tasty_type_geometry::length::{LogicalPx, PhysicalPx};
use tasty_type_geometry::rect::PhysicalRect;
use tasty_ui_widgets::{StatusBarAction, StatusBarData, draw_status_bar_view};

use crate::core::state::HeadState;
use crate::state::AppState;
use crate::theme;

/// Area 생성과 egui_bridge의 레이어 정렬에서 공유하는 상태바 ID.
pub(crate) const STATUS_BAR_AREA_ID: &str = "workspace_status_bar";

/// 배너 위에 놓을 상태바 Foreground 레이어.
pub(crate) fn status_bar_layer_id() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new(STATUS_BAR_AREA_ID))
}

/// 터미널 영역에서 제외할 상태바 높이. Theme에서 읽고 물리 픽셀로 반환한다.
pub fn status_bar_bottom_inset(scale_factor: f32) -> PhysicalPx {
    theme::theme().status_bar_height.to_physical(scale_factor)
}

/// 터미널 영역 바로 아래에 상태바를 그리고 반환된 클릭 동작을 처리한다.
pub fn draw_status_bar(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    terminal_rect: PhysicalRect,
    scale_factor: f32,
) {
    let th = theme::theme();
    let bar_h_logical = th.status_bar_height.value();

    let logical = terminal_rect.to_logical(scale_factor);
    let x = logical.x.value().round_ui();
    let y = (logical.y + logical.height).value().round_ui();
    let w = logical.width.value().round_ui();
    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, bar_h_logical));

    let surface_id = state.focused_surface_id(engine);
    // 그리드·프로세스·Git 정보는 캐시에서 읽어 프레임마다 시스템·파일 조회를 반복하지 않는다.
    let grid = surface_id
        .and_then(|sid| engine.terminals.get(sid))
        .map(|term| (term.cols(), term.rows()));
    let shell = surface_id.and_then(|sid| engine.foreground_name(sid).map(str::to_owned));
    let branch = surface_id
        .and_then(|sid| engine.status_bar_branch(sid))
        .map(head_display);
    let pane_id = surface_id.and_then(|sid| engine.find_pane_for_surface(sid));
    let palette_keys = engine
        .settings
        .keybindings
        .toggle_command_palette
        .first()
        .map(|b| tasty_settings::KeybindingSettings::format_display(b, &engine.settings.general))
        .unwrap_or_default();

    let data = StatusBarData {
        branch,
        surface_id,
        pane_id,
        shell,
        grid,
        theme_is_light: th.is_light,
        palette_keys,
        palette_tooltip: crate::i18n::t("status_bar.palette_tooltip").to_owned(),
        theme_tooltip: crate::i18n::t("status_bar.theme_tooltip").to_owned(),
    };

    let result = egui::Area::new(egui::Id::new(STATUS_BAR_AREA_ID))
        .fixed_pos(rect.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            draw_status_bar_view(ui, &th, LogicalPx(rect.width()), &data)
        })
        .inner;
    state.resize_edge_widget_hovered |= result.resize_priority_hovered;

    for action in result.actions {
        match action {
            StatusBarAction::OpenPalette => {
                use crate::intent::{OpenPopupMode, UiIntent};
                state.dispatch_intent(
                    UiIntent::TogglePopup {
                        id: crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID,
                        mode: OpenPopupMode::CenteredFocused,
                    }
                    .from_user_menu("status_bar.palette"),
                );
            }
            StatusBarAction::ToggleTheme => {
                // 디자인 onTheme: latte ↔ mocha. 그 외 테마에서 누르면 latte 로.
                let target = if engine.settings.appearance.theme == tasty_themes::BUILTIN_LATTE_ID {
                    tasty_themes::BUILTIN_MOCHA_ID
                } else {
                    tasty_themes::BUILTIN_LATTE_ID
                };
                let mut new_settings = engine.settings.clone();
                tasty_themes::apply_theme(&mut new_settings.appearance, target);
                state.dispatch_intent(
                    crate::core::intent::DomainIntent::UpdateSettings(new_settings)
                        .from_user_menu("status_bar.theme_toggle"),
                );
            }
        }
    }
}

/// detached HEAD는 같은 이름의 브랜치와 구분하도록 @를 붙인다.
/// HeadState는 core 타입이라 공용 위젯에 넘기기 전에 표시 문자열로 바꾼다.
fn head_display(head: &HeadState) -> String {
    match head {
        HeadState::Branch(name) => name.clone(),
        HeadState::Detached(short_sha) => format!("@ {short_sha}"),
    }
}

#[cfg(test)]
mod tests {
    use super::{HeadState, head_display};

    #[test]
    fn detached_gets_the_marker_and_a_branch_does_not() {
        assert_eq!(head_display(&HeadState::Branch("main".into())), "main");
        assert_eq!(
            head_display(&HeadState::Detached("4af6ac9".into())),
            "@ 4af6ac9"
        );
    }

    /// 표지처럼 생긴 **이름**은 이름 그대로 나간다. 표지가 덧붙지 않는다.
    #[test]
    fn a_branch_that_looks_like_the_marker_is_not_decorated() {
        assert_eq!(
            head_display(&HeadState::Branch("@4af6ac9".into())),
            "@4af6ac9"
        );
    }
}
