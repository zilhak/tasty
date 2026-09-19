//! 작업영역(작업 컬럼) 하단 StatusBar 의 **본체 wrapper** — 디자인
//! `gallery/layouts.jsx` 의 **Workspace status bar** 섹션 대응. 위치·크기·구조(하단
//! 24px 바, 좌/우 클러스터)와 **표시 항목·축소 순서**가 모두 확정이다 — 상세
//! `docs/features/workspace-status-bar/index.md`.
//!
//! ## focus 의존성 (원칙 3)
//! surfaceId·셸·그리드·브랜치는 **현재 focus surface 를 read** 해서 표시한다. 이는
//! "활성 상태 정보를 조회로 제공"하는 허용된 read 용도이며, 표시 *대상 결정* 이
//! focus 에 의존한다(동작이 아니라 표시이므로 focus 독립성 원칙에 위배되지 않음).
//!
//! ## view / wrapper 분리
//! 순수 view 는 공용 crate `tasty_ui_widgets::draw_status_bar_view` 에 있고
//! (`StatusBarData` + `Theme` 만 받아 `StatusBarAction` 을 반환, 본체 비의존 —
//! 갤러리 specimen 이 **같은 함수를 호출**한다), 이 모듈의 wrapper
//! [`draw_status_bar`] 가 ① 부유 레이어(`egui::Area`) 생성 ② state/engine 에서
//! 데이터 추출 ③ i18n 라벨과 **표시 표지** 주입 ④ action 적용을 담당한다.
//!
//! ③ 의 "표시 표지" 는 [`head_display`] 다 — core 는 HEAD 의 갈래를 값으로만 주고,
//! detached 를 나타내는 `@ ` 는 여기서 붙는다.

use egui::emath::GuiRounding as _;
use tasty_type_geometry::length::{LogicalPx, PhysicalPx};
use tasty_type_geometry::rect::PhysicalRect;
use tasty_ui_widgets::{StatusBarAction, StatusBarData, draw_status_bar_view};

use crate::core::state::HeadState;
use crate::state::AppState;
use crate::theme;

/// StatusBar 부유 레이어(`egui::Area`)의 Id 문자열.
///
/// Area 생성과 z-order 는 **본체 정책**이라 view crate 로 내보내지 않는다. 이 상수가
/// 유일한 진실원이며 `gfx/gpu/egui_bridge.rs` 의 z-order 강제
/// (`set_sublayer(banner, status_bar)`)도 [`status_bar_layer_id`] 를 통해 이걸 읽는다 —
/// 문자열을 양쪽에 하드코딩하면 한쪽만 바꿔도 컴파일은 통과하고 z-order 만 조용히 깨진다.
pub(crate) const STATUS_BAR_AREA_ID: &str = "workspace_status_bar";

/// StatusBar Area 의 `LayerId` — z-order 배선(`enforce_foreground_z_order`)이 참조한다.
/// `Order::Foreground` 는 `docs/architecture/input-layer.md` 의 Banner(5) < egui위젯(4)
/// 관계를 만드는 전제라 여기서만 결정한다.
pub(crate) fn status_bar_layer_id() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, egui::Id::new(STATUS_BAR_AREA_ID))
}

/// StatusBar 가 작업 컬럼 하단에 차지하는 inset (physical px) —
/// `compute_terminal_rect` 의 `bottom_inset` 인자의 단일 진실원.
/// 항상 그려지므로 항상 실제 높이를 반환한다(titlebar `top_inset` 과 대칭).
///
/// 글로벌 `theme()` 를 읽으므로 view crate 로 옮기지 않는다(그 crate 는 글로벌 theme
/// 접근 금지 — 모든 함수가 `&Theme` 을 명시적으로 받는다).
pub fn status_bar_bottom_inset(scale_factor: f32) -> PhysicalPx {
    theme::theme().status_bar_height.to_physical(scale_factor)
}

/// wrapper — Area 를 띄우고 state/engine 에서 데이터를 추출해 view 를 그린 뒤, view 가
/// 보고한 클릭 액션을 state mutation(팔레트 오픈 / 테마 토글)으로 변환한다.
///
/// `terminal_rect` 는 `bottom_inset` 이 이미 반영된 작업 컬럼 사각형(physical).
/// StatusBar 는 그 바로 아래 strip 을 차지한다.
pub fn draw_status_bar(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    terminal_rect: PhysicalRect,
    scale_factor: f32,
) {
    let th = theme::theme();
    let bar_h_logical = th.status_bar_height.value();

    // 작업 컬럼 하단 strip 의 logical 사각형.
    let logical = terminal_rect.to_logical(scale_factor);
    let x = logical.x.value().round_ui();
    let y = (logical.y + logical.height).value().round_ui();
    let w = logical.width.value().round_ui();
    let rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, bar_h_logical));

    // ── 데이터 추출 (immutable read) ──
    let surface_id = state.focused_surface_id(engine);
    // Grid (cols/rows) is a lock-free handle-cache read. The foreground process
    // name comes from the 1Hz Tick::Busy cache (`foreground_name`) rather than a
    // per-frame system snapshot — re-snapshotting every frame both cost ≈6ms on
    // the main thread and made the name flicker while agents churned helpers.
    // The git branch comes from the same 1Hz tick (`status_bar_branch`) for the
    // same reason — resolving it here re-opened `.git/HEAD` on every repaint
    // (`core/state/branch.rs`).
    let grid = surface_id
        .and_then(|sid| engine.terminals.get(sid))
        .map(|term| (term.cols(), term.rows()));
    let shell = surface_id.and_then(|sid| engine.foreground_name(sid).map(str::to_owned));
    let branch = surface_id
        .and_then(|sid| engine.status_bar_branch(sid))
        .map(head_display);
    // surface 를 담은 pane — 디자인의 `s3·p1` 표기에서 뒷마디다. 못 찾으면 앞마디만
    // 나간다(자리를 비워 두지 않는다).
    let pane_id = surface_id.and_then(|sid| engine.find_pane_for_surface(sid));
    // 팔레트 단축키는 **키캡으로** 그린다 — 라벨 단어를 붙이지 않는다. 바인딩이 없으면
    // 빈 문자열이고, 그때는 값이 없는 항목이라 view 가 자리째 뺀다.
    let palette_keys = engine
        .settings
        .keybindings
        .toggle_command_palette
        .first()
        .map(|b| tasty_settings::KeybindingSettings::format_display(b, &engine.settings.general))
        .unwrap_or_default();

    // i18n 은 본체 소유 — view crate 는 `tasty-i18n` 을 의존하지 않으므로 tooltip 을
    // 여기서 주입한다.
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

    // ── view ── Area 는 본체가 소유한다(위 STATUS_BAR_AREA_ID 주석).
    let result = egui::Area::new(egui::Id::new(STATUS_BAR_AREA_ID))
        .fixed_pos(rect.min)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            draw_status_bar_view(ui, &th, LogicalPx(rect.width()), &data)
        })
        .inner;
    state.resize_edge_widget_hovered |= result.resize_priority_hovered;

    // ── action 적용 ──
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

/// 브랜치 슬롯에 그릴 문자열 — [`HeadState`] 에 **표시 표지를 입히는 유일한 자리**.
///
/// detached 에 `@ ` 를 붙이는 이유는 그것 없이 sha 만 놓으면 브랜치 글리프 옆의
/// `4f9c1ab` 가 *그 이름의 브랜치*로 읽히기 때문이다. 값 자체는 디자인 성분이 그대로
/// 들고 있는 것이라 여기서 정하지 않는다 —
/// `docs/features/workspace-status-bar/index.md` 의 "표시 데이터".
///
/// **이 표지가 core 가 아니라 여기 사는 이유.** 그것은 파싱 결과가 아니라 그리는 쪽의
/// 어휘다. 같은 바의 `s3·p1`(surface·pane)과 `120×32`(grid)도 같은 성질이라 구조를
/// 받아 기호를 조립하는데, 그 둘은 view crate 안에서 조립된다. 브랜치만 여기인 것은
/// [`HeadState`] 가 core 타입이고 `tasty-ui-widgets` 는 core 를 의존하지 않기 때문이다
/// — 갈래를 view 까지 내리려면 두 쪽이 함께 보는 크레이트가 필요하고, 그것은 이 한
/// 타입이 치를 값이 아니다. 그래서 경계를 wrapper 에 둔다. wrapper 가 표시 재료를
/// 주입하는 자리라는 것은 바로 위 i18n 툴팁 주입과 같다.
///
/// 이 문자열은 `t()` 대상이 아니다 — `@ ` 는 자연어가 아니라 고정 기호이고, 같은 바의
/// `·` 와 `×` 도 리터럴이다.
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
