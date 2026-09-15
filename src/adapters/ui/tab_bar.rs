//! Pane tab bars — pane 별 상단 탭 표시 + 사용자 입력 (focus / drag / context menu / 새 탭).
//!
//! ## Split: wrapper / view / action
//!
//! 순수 시각 `draw_pane_tab_bars_view` 는 [`PaneTabBarsProps`] 만 받고
//! [`PaneTabBarsOutput`] (collected actions + measured height) 만 반환한다.
//! AppState/CoreState/`theme::theme()` 비의존. Gallery (`tasty-gallery`) 는
//! view 를 mock props 로 mirror 해서 시각 검증.
//!
//! wrapper `draw_pane_tab_bars` 는 (a) state/engine 에서 props 추출,
//! (b) view 호출, (c) 반환된 [`TabBarAction`] 리스트를 state mutation 으로 변환,
//! (d) measured height 를 `state.tab_bar_height` 에 기록.

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
    /// 이 액션이 유래한 pane. `Some` 이면 처리 전에 그 pane 으로 focus 를 옮긴다
    /// (탭바 primary-click 계열 — 탭 클릭/닫기/스크롤/빈 영역 클릭/+·split·search 버튼).
    /// 우클릭 컨텍스트 메뉴는 대상 `pane_id`/`tab_index` 를 메뉴 항목에 그대로 실어
    /// 나르므로 focus 이동이 필요 없다(조회/메뉴-오픈이지 조작 commit 이 아님).
    ///
    /// `DragStart` 도 focus 이동 대상에 포함한다 — egui 0.31.1 의 `clicked()`/
    /// `drag_started_by()` 는 같은 press-release 상호작용에서 발생 프레임이 겹치지
    /// 않고 상호 배타적이라(`clicked()` 는 pointer-up 프레임에서만, `drag_started_by()`
    /// 는 그 이전에 drag threshold 를 넘는 프레임에서만 세팅됨 — vendored
    /// `egui-0.31.1/src/{context.rs,interaction.rs}` 확인), 비-focused pane 의 탭을
    /// 클릭 없이 곧장 드래그하면 `SwitchTab` 없이 `DragStart` 만 단독으로 발생한다.
    /// 이 경우에도 "탭바 조작은 그 pane 을 조작하는 행위"라는 원칙(위 문단)을 그대로
    /// 적용해 focus 가 따라가야 한다. `DragUpdate`/`DragEnd` 는 이미 `DragStart` 에서
    /// focus 가 이동한 뒤에 오는 후속 프레임이라 별도 이동이 불필요.
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
    /// 첫 pane 의 탭 바 높이. 측정 못 했으면 None. 좌표계는 주석이 아니라 **타입**이
    /// 보증한다 — egui 가 준 logical 높이를 `to_physical(scale_factor)` 로 변환해 담는다.
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

/// Wrapper — state/engine 에서 props 추출 → view 호출 → action 적용.
///
/// 시그니처는 기존과 동일 (외부 호출처 무영향).
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
                        // kind→아이콘: registry 의 SurfaceKindDef.icon 이름을 host
                        // 아이콘 세트로 해석(하드코딩 없음). 미선언/미등록은 FILE.
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

    // switch-number overlay — `switch_overlay()` 스냅샷(사용자 입력 ModifiersChanged 로만
    // 갱신)에서 Tab 대상 + 그릴 focused pane id 를 읽는다. 그 pane 의 탭바에서만 키캡을
    // 그리므로 비-focused pane 에는 거짓 안내가 뜨지 않는다. 스냅샷은 egui raw_input 의
    // 사용자 키 입력만 반영 → IPC/CLI/에이전트로는 강제 표시될 수 없다(순수 미리보기).
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

    /// [`run_view`] 와 동일하되 호출자가 `egui::Context` 를 직접 제공한다 — 활성
    /// 탭 추종 스크롤은 프레임 간 상태를 `ctx` persistent memory 에 추적하므로,
    /// "여러 프레임에 걸친 변화"를 검증하려면 같은 ctx 를 재사용해 여러 번 호출해야 한다.
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

    // 아래 스크롤 보정 테스트들의 공통 지오메트리 (tab_w=160, separator_w=1,
    // plus_w=28, right_icons_w=56, arrow_w=20 — `draw_pane_tab_bars_view` 상수와 동일):
    // pane 폭 800 · 탭 8개 → content_w=1316, avail_w=744, needs_scroll,
    // viewport_w=704, max_scroll=612.

    #[test]
    fn switching_to_offscreen_tab_scrolls_it_into_view() {
        // 탭 8개, 화면에는 앞쪽 몇 개만 보이는 좁은 pane 폭. 마지막 탭(인덱스 7)으로
        // 전환 — 현재 뷰포트(scroll=0) 밖.
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
        // 마지막 탭에서 스크롤이 오른쪽 끝까지 밀려난 상태(scroll=max_scroll)에서
        // 첫 탭(인덱스 0)으로 wrap-around 전환 — 왼쪽으로 다시 보정돼야 한다.
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
        // 같은 ctx 로 두 프레임 연속 렌더 — active_tab 도 pane 지오메트리도 바뀌지
        // 않았다면, 활성 탭(0)이 사용자가 화살표로 스크롤해 가려 놓은 상태(scroll=400,
        // 탭 0 은 뷰포트 밖)라도 보정을 강제하면 안 된다.
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
        // 1프레임: 넓은 pane(2000px) — 스크롤 불필요, 탭 7 전체가 이미 보임.
        // 2프레임: 같은 ctx, 같은 active_tab(7) 이지만 pane 이 800px 로 좁아져
        // 스크롤이 필요해짐 — active_tab 은 안 바뀌었어도 지오메트리 변화로 보정돼야 한다.
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
        // 첫 탭(인덱스 0)이 활성이고 scroll=0 이면 이미 뷰포트 안 — 아무 보정도
        // 필요 없다(경계값: 스크롤 화살표/"+" 버튼 폭을 뺀 뒤에도 첫 탭은 항상 보임).
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
        // drag overlay 자체는 actions 를 추가하지 않음
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

    /// mirror surface(로컬 PTY 없음, `set_mirror_surface_busy` 로만 채워짐)를 담은
    /// 탭도 `compute_tab_is_busy` 가 busy 로 판정해야 한다 — `busy_surfaces` 를 직접
    /// 참조하던 예전 코드는 mirror surface 를 절대 못 봐서 dot 이 안 떴던 버그의
    /// 회귀 테스트.
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
