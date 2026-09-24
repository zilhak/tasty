//! pane별 탭바. 앱 상태에서 화면 입력을 만들고 반환된 동작과 측정 높이를 반영한다.
//! 공용 화면 함수는 앱 상태 없이 갤러리에서도 사용할 수 있다.

mod apply;
mod tab;
mod view;

pub use apply::apply_tab_bar_actions;
pub use view::{compute_drop_index, draw_pane_tab_bars_view};

use tasty_type_appearance::theme::Theme;
use tasty_type_geometry::length::PhysicalPx;
use tasty_type_geometry::rect::PhysicalRect;

use crate::adapters::ui::icons;
use crate::core::AttentionKind;
use crate::state::AppState;
use crate::theme;

/// View 입력 — pane 한 개 분의 탭 데이터.
#[derive(Clone, Debug)]
pub struct PaneTabBarView {
    pub pane_id: u32,
    /// Pane 의 *물리* 좌표 사각형 (view 가 scale_factor 로 logical 변환).
    pub rect: PhysicalRect,
    pub tab_names: Vec<String>,
    /// 탭별 leading 아이콘. wrapper 가 registry(kind→`SurfaceKindDef.icon`)에서 해석해
    /// 담는다 — view 는 kind 를 모른 채 아이콘만 그린다(엔진 비의존 유지).
    pub tab_icons: Vec<icons::Icon>,
    /// 탭별 attention kind — 탭에 속한 surface 들의 dominant kind
    /// (`CoreState::attention_dominant_kind`). `Some(NeedsInput)`=노랑,
    /// `Some(Completion)`=파랑, `None`=attention 없음(active/평상시 색으로 폴백).
    pub tab_attention_kind: Vec<Option<AttentionKind>>,
    /// 탭별 busy(녹색 점) 여부.
    pub tab_is_busy: Vec<bool>,
    pub active_tab: usize,
    /// 이 pane 이 현재 focus 인지 — 배경 (surface0 vs mantle) 결정.
    pub is_focused: bool,
    /// 가로 스크롤 오프셋 (logical px).
    pub scroll_offset: f32,
}

/// View 입력 — drag 진행 중인 탭의 상태. None 이면 drag overlay 미표시.
#[derive(Clone, Debug)]
pub struct TabDragView {
    pub pane_id: u32,
    pub tab_index: usize,
    /// 현재 마우스 x (logical pane 좌표).
    pub current_x: f32,
}

/// View 입력 — 전체 pane 의 탭 바 + drag 상태 + appearance 옵션.
pub struct PaneTabBarsProps<'a> {
    pub theme: &'a Theme,
    /// 탭 slot 키캡 문자를 읽을 키바인딩 설정 (switch-number overlay 표시=동작 일치).
    pub kb: &'a crate::settings::KeybindingSettings,
    pub panes: &'a [PaneTabBarView],
    pub scale_factor: f32,
    /// 사용자 옵션 — 탭 1 개의 가로 너비 (logical px).
    pub tab_width: f32,
    /// 사용자 옵션 — 탭 라벨 폰트 크기 (logical px).
    pub tab_font_size: f32,
    /// 사용자 옵션 — 활성 탭 인디케이터 스타일 (Underline / Fill / Dot).
    pub active_tab_indicator: crate::settings::ActiveTabIndicator,
    /// 현재 drag 진행 상태 (None 이면 overlay 미표시).
    pub drag: Option<TabDragView>,
    /// switch-number overlay — 키캡을 그릴 **focused pane id**.
    /// 사용자가 `tab_switch_modifier`(대상=Tab)를 누르고 있는 동안만 `Some(focused_pane)`,
    /// 그 외엔 `None`. 이 pane 의 탭바에서만 leading 아이콘을 숫자 키캡(`Ctrl+1`…`0`)으로
    /// in-place 교체한다(비-focused pane 은 held 여도 아이콘 유지). release 시 `None` → 원복.
    pub switch_overlay_pane: Option<u32>,
}

/// View 가 발생시킨 사용자 의도. wrapper 가 state/engine 으로 반영.
#[derive(Clone, Debug, PartialEq)]
pub enum TabBarAction {
    SwitchTab {
        pane_id: u32,
        tab_index: usize,
    },
    CloseTab {
        pane_id: u32,
        tab_index: usize,
    },
    AddTab {
        pane_id: u32,
    },
    /// 탭스트립 우측 Split 아이콘 — 해당 pane 을 분할 (기존 split_pane 경로 재사용).
    RequestSplit {
        pane_id: u32,
    },
    /// 탭스트립 우측 Search 아이콘 — 해당 pane 활성 surface 검색 (기존 find 경로 재사용).
    OpenSearch {
        pane_id: u32,
    },
    ScrollLeft {
        pane_id: u32,
    },
    ScrollRight {
        pane_id: u32,
    },
    /// 활성 탭 전환(또는 pane 리사이즈)으로 활성 탭이 뷰포트 밖으로 밀려났을 때
    /// view 가 계산한 보정 스크롤 오프셋. 사용자가 직접 누른 액션이 아니라 뷰
    /// 렌더링 결과에 대한 보정이므로 `focus_target_pane` 대상에서 제외한다.
    AutoScrollToActiveTab {
        pane_id: u32,
        offset: f32,
    },
    /// 탭바의 탭 없는 빈 영역(뷰포트) primary click — 탭 전환 없이 그 pane 으로
    /// focus 만 이동한다.
    FocusPane {
        pane_id: u32,
    },
    OpenContextMenu {
        pane_id: u32,
        tab_index: usize,
        pos: egui::Pos2,
    },
    OpenPaneContextMenu {
        pane_id: u32,
        pos: egui::Pos2,
    },
    /// 탭 "+" 버튼 우클릭 — 프리셋으로 탭/페인 생성 진입점.
    OpenNewTabButtonContextMenu {
        pane_id: u32,
        pos: egui::Pos2,
    },
    DragStart {
        pane_id: u32,
        tab_index: usize,
    },
    DragUpdate {
        pane_id: u32,
        mouse_x: f32,
    },
    DragEnd {
        pane_id: u32,
    },
}

impl TabBarAction {
    /// 직접 조작할 pane. 클릭·드래그 시작은 먼저 포커스를 옮긴다.
    /// 드래그는 클릭 없이 시작할 수 있어 별도로 포함한다. 후속 드래그 프레임은 다시 옮기지 않는다.
    /// 우클릭 메뉴는 대상 ID를 전달하며 열 때 포커스를 바꾸지 않는다.
    fn focus_target_pane(&self) -> Option<u32> {
        match *self {
            TabBarAction::SwitchTab { pane_id, .. }
            | TabBarAction::CloseTab { pane_id, .. }
            | TabBarAction::AddTab { pane_id }
            | TabBarAction::RequestSplit { pane_id }
            | TabBarAction::OpenSearch { pane_id }
            | TabBarAction::ScrollLeft { pane_id }
            | TabBarAction::ScrollRight { pane_id }
            | TabBarAction::FocusPane { pane_id }
            | TabBarAction::DragStart { pane_id, .. } => Some(pane_id),
            TabBarAction::OpenContextMenu { .. }
            | TabBarAction::OpenPaneContextMenu { .. }
            | TabBarAction::OpenNewTabButtonContextMenu { .. }
            | TabBarAction::DragUpdate { .. }
            | TabBarAction::DragEnd { .. }
            | TabBarAction::AutoScrollToActiveTab { .. } => None,
        }
    }
}

/// View 의 출력 — 사용자 의도 리스트 + 측정된 탭 바 높이.
#[derive(Default)]
pub struct PaneTabBarsOutput {
    pub actions: Vec<TabBarAction>,
    /// 첫 pane에서 측정한 탭바 높이를 물리 픽셀로 변환한 값. 측정하지 못했으면 None이다.
    pub measured_height_physical: Option<PhysicalPx>,
}

/// 탭별 busy(녹색 점) 여부 계산. `is_surface_busy()`(로컬 ∪ mirror busy 합집합)를
/// 거쳐야 원격 attach mirror surface 를 담은 탭도 dot 이 뜬다.
fn compute_tab_is_busy(engine: &crate::core::CoreState, tabs: &[crate::model::Tab]) -> Vec<bool> {
    tabs.iter()
        .map(|t| {
            let sids = t.all_surface_ids();
            sids.iter().any(|sid| engine.is_surface_busy(*sid))
        })
        .collect()
}

/// 화면 입력을 만들고 결과를 앱 상태에 반영한다.
pub fn draw_pane_tab_bars(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    pane_rects: &[(u32, PhysicalRect)],
    scale_factor: f32,
) {
    let th = theme::theme();
    let focused_pane_id = state.focused_pane_id(engine);

    let mut panes: Vec<PaneTabBarView> = Vec::new();
    {
        let ws = state.active_workspace(engine);
        for &(pane_id, pane_rect) in pane_rects {
            let pane = match ws.pane_layout().find_pane(pane_id) {
                Some(p) => p,
                None => continue,
            };
            let tab_attention_kind: Vec<Option<AttentionKind>> = pane
                .tabs
                .iter()
                .map(|t| {
                    let sids = t.all_surface_ids();
                    engine.attention_dominant_kind(&sids)
                })
                .collect();
            let tab_is_busy = compute_tab_is_busy(engine, &pane.tabs);
            panes.push(PaneTabBarView {
                pane_id,
                rect: pane_rect,
                tab_names: pane.tabs.iter().map(|t| t.display_name()).collect(),
                tab_icons: pane
                    .tabs
                    .iter()
                    .map(|t| {
                        let kind = engine
                            .find_surface_by_id(t.focused_surface)
                            .map(|s| s.kind())
                            .unwrap_or("terminal");
                        engine
                            .surface_registry
                            .get(kind)
                            .and_then(|d| d.icon.clone())
                            .map(|n| icons::from_name(&n))
                            .unwrap_or(icons::FILE)
                    })
                    .collect(),
                tab_attention_kind,
                tab_is_busy,
                active_tab: pane.active_tab,
                is_focused: pane_id == focused_pane_id,
                scroll_offset: pane.tab_scroll_offset,
            });
        }
    }

    let appearance = &engine.settings.appearance;
    let tab_w = appearance.tab_width;
    let tab_font_size = appearance.tab_font_size;

    let drag = state.dialogs.tab_drag.as_ref().map(|d| TabDragView {
        pane_id: d.pane_id,
        tab_index: d.tab_index,
        current_x: d.current_x,
    });

    // 사용자 modifier 입력으로 갱신한 스냅샷에서 키캡을 표시할 pane을 읽는다.
    let switch_overlay_pane = state.switch_overlay().and_then(|o| match o.target {
        crate::adapters::ui::switch_overlay::SwitchTarget::Tab => o.pane_id,
        crate::adapters::ui::switch_overlay::SwitchTarget::Workspace
        | crate::adapters::ui::switch_overlay::SwitchTarget::Category => None,
    });

    let props = PaneTabBarsProps {
        theme: &th,
        kb: &engine.settings.keybindings,
        panes: &panes,
        scale_factor,
        tab_width: tab_w,
        tab_font_size,
        active_tab_indicator: appearance.active_tab_indicator,
        drag,
        switch_overlay_pane,
    };

    let output = draw_pane_tab_bars_view(ctx, &props);

    if let Some(h_phys) = output.measured_height_physical {
        state.tab_bar_height = h_phys;
    }

    apply_tab_bar_actions(state, engine, output.actions, &panes, tab_w, scale_factor);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_target_pane_primary_click_actions_carry_pane_id() {
        let primary = [
            TabBarAction::SwitchTab {
                pane_id: 7,
                tab_index: 0,
            },
            TabBarAction::CloseTab {
                pane_id: 7,
                tab_index: 0,
            },
            TabBarAction::AddTab { pane_id: 7 },
            TabBarAction::RequestSplit { pane_id: 7 },
            TabBarAction::OpenSearch { pane_id: 7 },
            TabBarAction::ScrollLeft { pane_id: 7 },
            TabBarAction::ScrollRight { pane_id: 7 },
            TabBarAction::FocusPane { pane_id: 7 },
            TabBarAction::DragStart {
                pane_id: 7,
                tab_index: 0,
            },
        ];
        for action in primary {
            assert_eq!(
                action.focus_target_pane(),
                Some(7),
                "{action:?} 는 그 pane 으로 focus 를 옮겨야 한다"
            );
        }
    }

    #[test]
    fn focus_target_pane_context_menu_and_drag_update_end_actions_are_none() {
        let non_focus = [
            TabBarAction::OpenContextMenu {
                pane_id: 7,
                tab_index: 0,
                pos: egui::Pos2::ZERO,
            },
            TabBarAction::OpenPaneContextMenu {
                pane_id: 7,
                pos: egui::Pos2::ZERO,
            },
            TabBarAction::OpenNewTabButtonContextMenu {
                pane_id: 7,
                pos: egui::Pos2::ZERO,
            },
            TabBarAction::DragUpdate {
                pane_id: 7,
                mouse_x: 0.0,
            },
            TabBarAction::DragEnd { pane_id: 7 },
            TabBarAction::AutoScrollToActiveTab {
                pane_id: 7,
                offset: 0.0,
            },
        ];
        for action in non_focus {
            assert_eq!(
                action.focus_target_pane(),
                None,
                "{action:?} 는 focus 를 옮기면 안 된다(우클릭은 조작 commit 이 아니고, DragUpdate/DragEnd 는 DragStart 에서 이미 focus 가 이동한 뒤의 후속 프레임)"
            );
        }
    }

    #[test]
    fn compute_drop_index_first_slot() {
        let idx = compute_drop_index(100.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 0);
    }

    #[test]
    fn compute_drop_index_middle_slot() {
        // mouse_x=281, pane_x=100 → content_x=181 → slot=181/121 ≈ 1.496 → round 1
        let idx = compute_drop_index(281.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 1);
    }

    #[test]
    fn compute_drop_index_last_slot_clamped() {
        let idx = compute_drop_index(10_000.0, 100.0, 0.0, 3, 120.0, 1.0, 400.0);
        assert_eq!(idx, 2);
    }

    #[test]
    fn compute_drop_index_accounts_for_scroll() {
        let idx0 = compute_drop_index(100.0, 100.0, 0.0, 5, 120.0, 1.0, 400.0);
        let idx_scroll = compute_drop_index(100.0, 100.0, 121.0, 5, 120.0, 1.0, 400.0);
        assert_eq!(idx0, 0);
        assert_eq!(idx_scroll, 1);
    }

    fn test_theme() -> Theme {
        tasty_themes::mocha_fallback()
    }

    fn run_view(panes: Vec<PaneTabBarView>, drag: Option<TabDragView>) -> PaneTabBarsOutput {
        run_view_on(&egui::Context::default(), panes, drag)
    }

    /// 같은 Context로 여러 프레임을 그려 활성 탭의 자동 스크롤을 검사한다.
    fn run_view_on(
        ctx: &egui::Context,
        panes: Vec<PaneTabBarView>,
        drag: Option<TabDragView>,
    ) -> PaneTabBarsOutput {
        let theme = test_theme();
        let kb = crate::settings::KeybindingSettings::default();
        let mut out = PaneTabBarsOutput::default();
        drop(ctx.run(egui::RawInput::default(), |ctx| {
            let props = PaneTabBarsProps {
                theme: &theme,
                kb: &kb,
                panes: &panes,
                scale_factor: 1.0,
                tab_width: 160.0,
                tab_font_size: 12.0,
                active_tab_indicator: crate::settings::ActiveTabIndicator::default(),
                drag: drag.clone(),
                switch_overlay_pane: None,
            };
            out = draw_pane_tab_bars_view(ctx, &props);
        }));
        out
    }

    fn mk_pane(pane_id: u32, names: &[&str], active: usize, focused: bool) -> PaneTabBarView {
        mk_pane_w(pane_id, names, active, focused, 800.0)
    }

    fn mk_pane_w(
        pane_id: u32,
        names: &[&str],
        active: usize,
        focused: bool,
        width: f32,
    ) -> PaneTabBarView {
        let n = names.len();
        PaneTabBarView {
            pane_id,
            rect: PhysicalRect {
                x: PhysicalPx(0.0),
                y: PhysicalPx(0.0),
                width: PhysicalPx(width),
                height: PhysicalPx(600.0),
            },
            tab_names: names.iter().map(|s| s.to_string()).collect(),
            tab_icons: vec![icons::TERM; n],
            tab_attention_kind: vec![None; n],
            tab_is_busy: vec![false; n],
            active_tab: active,
            is_focused: focused,
            scroll_offset: 0.0,
        }
    }

    /// [`TabBarAction::AutoScrollToActiveTab`] 중 주어진 pane 대상인 것의 offset.
    fn auto_scroll_offset(out: &PaneTabBarsOutput, pane_id: u32) -> Option<f32> {
        out.actions.iter().find_map(|a| match *a {
            TabBarAction::AutoScrollToActiveTab { pane_id: p, offset } if p == pane_id => {
                Some(offset)
            }
            _ => None,
        })
    }

    #[test]
    fn switching_to_offscreen_tab_scrolls_it_into_view() {
        let pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 7, true);
        let out = run_view(vec![pane], None);

        let tab_w = 160.0;
        let separator_w = 1.0;
        let viewport_w = 704.0;
        let tab_start = 7.0 * (tab_w + separator_w);
        let tab_end = tab_start + tab_w;

        let offset = auto_scroll_offset(&out, 1).expect("offscreen 전환은 보정을 emit 해야 한다");
        assert!(offset <= tab_start);
        assert!(offset + viewport_w >= tab_end);
    }

    #[test]
    fn switching_to_first_tab_wraps_scroll_back_into_view() {
        let ctx = egui::Context::default();
        let mut pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 7, true);
        pane.scroll_offset = 612.0; // max_scroll
        run_view_on(&ctx, vec![pane], None);

        let mut pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 0, true);
        pane.scroll_offset = 612.0;
        let out = run_view_on(&ctx, vec![pane], None);

        let offset = auto_scroll_offset(&out, 1).expect("wrap-around 전환도 보정을 emit 해야 한다");
        assert_eq!(offset, 0.0, "첫 탭은 뷰포트 좌측 끝(0)에서 보여야 한다");
    }

    #[test]
    fn active_tab_unchanged_does_not_override_manual_scroll() {
        // 활성 탭·크기가 그대로면 사용자가 이동한 스크롤을 덮어쓰지 않는다.
        let ctx = egui::Context::default();
        let pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 0, true);
        run_view_on(&ctx, vec![pane], None);

        let mut pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 0, true);
        pane.scroll_offset = 400.0;
        let out = run_view_on(&ctx, vec![pane], None);

        assert_eq!(
            auto_scroll_offset(&out, 1),
            None,
            "활성 탭이 그대로면 수동 스크롤 상태를 덮어쓰면 안 된다"
        );
    }

    #[test]
    fn pane_resize_reveals_correction_even_without_active_change() {
        // 활성 탭은 그대로이고 pane만 좁아진 경우에도 필요한 스크롤을 보정한다.
        let ctx = egui::Context::default();
        let wide = mk_pane_w(
            1,
            &["A", "B", "C", "D", "E", "F", "G", "H"],
            7,
            true,
            2000.0,
        );
        run_view_on(&ctx, vec![wide], None);

        let narrow = mk_pane_w(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 7, true, 800.0);
        let out = run_view_on(&ctx, vec![narrow], None);

        let tab_w = 160.0;
        let separator_w = 1.0;
        let viewport_w = 704.0;
        let tab_start = 7.0 * (tab_w + separator_w);
        let tab_end = tab_start + tab_w;
        let offset =
            auto_scroll_offset(&out, 1).expect("resize 로 out-of-view 가 된 경우도 보정해야 한다");
        assert!(offset <= tab_start);
        assert!(offset + viewport_w >= tab_end);
    }

    #[test]
    fn viewport_that_already_shows_active_tab_emits_no_correction() {
        let pane = mk_pane(1, &["A", "B", "C", "D", "E", "F", "G", "H"], 0, true);
        let out = run_view(vec![pane], None);
        assert_eq!(auto_scroll_offset(&out, 1), None);
    }

    #[test]
    fn view_idle_emits_no_actions() {
        let panes = vec![mk_pane(1, &["A", "B"], 0, true)];
        let out = run_view(panes, None);
        assert!(out.actions.is_empty());
        assert!(out.measured_height_physical.is_some());
    }

    #[test]
    fn view_measures_bar_height_for_first_pane() {
        let panes = vec![
            mk_pane(1, &["A"], 0, true),
            mk_pane(2, &["X", "Y"], 0, false),
        ];
        let out = run_view(panes, None);
        assert!(out.measured_height_physical.unwrap_or_default().value() > 0.0);
    }

    #[test]
    fn view_empty_panes_returns_default_output() {
        let out = run_view(vec![], None);
        assert!(out.actions.is_empty());
        assert!(out.measured_height_physical.is_none());
    }

    #[test]
    fn view_with_drag_does_not_panic() {
        let panes = vec![mk_pane(1, &["A", "B", "C"], 1, true)];
        let drag = Some(TabDragView {
            pane_id: 1,
            tab_index: 1,
            current_x: 240.0,
        });
        let out = run_view(panes, drag);
        assert!(out.actions.is_empty());
    }

    fn test_engine() -> crate::core::CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::core::CoreState::new(80, 24, waker).expect("engine")
    }

    fn tab_with_surface(sid: crate::model::SurfaceId) -> crate::model::Tab {
        let surface: Box<dyn crate::model::Surface> =
            Box::new(crate::model::EmptySurface::new(sid));
        crate::model::Tab::new_with_surface(1, "t".to_string(), surface)
    }

    /// 로컬 PTY가 없는 mirror surface도 원격 busy 상태로 탭 표시를 갱신한다.
    #[test]
    fn compute_tab_is_busy_true_for_mirror_only_surface() {
        let mut engine = test_engine();
        let sid = 4242;
        engine.set_mirror_surface_busy(sid, true);
        let tabs = vec![tab_with_surface(sid)];

        let result = compute_tab_is_busy(&engine, &tabs);

        assert_eq!(result, vec![true]);
    }

    #[test]
    fn compute_tab_is_busy_false_when_idle() {
        let engine = test_engine();
        let sid = 4343;
        let tabs = vec![tab_with_surface(sid)];

        let result = compute_tab_is_busy(&engine, &tabs);

        assert_eq!(result, vec![false]);
    }
}
