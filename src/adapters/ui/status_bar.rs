//! 작업영역(작업 컬럼) 하단 StatusBar 의 **본체 wrapper** — 디자인
//! `ui_kits/terminal/work.jsx` 의 `StatusBar` 컴포넌트 대응. 위치·크기·구조(하단 24px
//! 바, 좌/우 클러스터)는 확정이나 표시 항목은 잠정이다 — 상세
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
//! 데이터 추출 ③ i18n 라벨 주입 ④ action 적용을 담당한다.

use egui::emath::GuiRounding as _;
use tasty_type_geometry::length::{LogicalPx, PhysicalPx};
use tasty_type_geometry::rect::PhysicalRect;
use tasty_ui_widgets::{StatusBarAction, StatusBarData, draw_status_bar_view};

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
    let x = (terminal_rect.x.value() / scale_factor).round_ui();
    let y = ((terminal_rect.y.value() + terminal_rect.height.value()) / scale_factor).round_ui();
    let w = (terminal_rect.width.value() / scale_factor).round_ui();
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
    let branch = surface_id.and_then(|sid| engine.status_bar_branch(sid).map(str::to_owned));
    let palette_binding = engine
        .settings
        .keybindings
        .toggle_command_palette
        .first()
        .map(|b| tasty_settings::KeybindingSettings::format_display(b, &engine.settings.general))
        .unwrap_or_default();

    // i18n 은 본체 소유 — view crate 는 `tasty-i18n` 을 의존하지 않으므로 라벨/tooltip
    // 을 여기서 완성해 주입한다. 팔레트 라벨은 그리기와 폭 계산이 같은 문자열을 써야
    // 우측 클러스터 정렬이 맞으므로 한 번만 조립한다.
    let palette_word = crate::i18n::t("status_bar.palette");
    let palette_label = if palette_binding.is_empty() {
        palette_word.to_owned()
    } else {
        format!("{palette_binding} {palette_word}")
    };

    let data = StatusBarData {
        branch,
        surface_id,
        shell,
        grid,
        theme_id: engine.settings.appearance.theme.clone(),
        theme_is_light: th.is_light,
        palette_label,
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
