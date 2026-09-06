use crate::model::{DividerInfo, LogicalPx, PhysicalPx, PhysicalRect, SplitDirection};

use super::AppState;
use crate::core::CoreState;

/// divider 히트 판정 밴드의 반폭. press 로 드래그를 시작하는 경로, 커서 아이콘 경로,
/// 트래킹 앱 hover 보고 가드가 **같은 값**을 봐야 "커서는 ↔ 인데 TUI 는 hover 를 받는"
/// 식의 어긋남이 생기지 않는다. 여러 곳에 리터럴로 흩어두면 드리프트한다.
///
/// **논리 길이다.** 이 값이 물리였을 때는 DPI 배율 2 화면에서 드래그 표적의 실제
/// 크기가 절반이 됐다 — 물리 픽셀은 배율이 오를수록 작아지므로, 조작 표적을 물리로
/// 고정하면 고배율일수록 집기 어려워진다. 배율 1 에서는 논리=물리라 그 회귀가
/// 드러나지 않는다. 비교 좌표계로 내리는 것은 [`divider_hit_threshold_physical`].
pub const DIVIDER_HIT_THRESHOLD: LogicalPx = LogicalPx(4.0);

/// 히트 밴드를 비교 좌표계(물리)로 내린다.
///
/// 마우스 좌표가 물리라 비교 직전에 한 번만 변환한다. 호출부마다 `to_physical` 을
/// 적으면 그것이 곧 위 doc 이 경고하는 드리프트의 다음 형태이므로, 변환도 이 한
/// 곳에만 둔다.
pub fn divider_hit_threshold_physical(scale_factor: f32) -> f32 {
    DIVIDER_HIT_THRESHOLD.to_physical(scale_factor).value()
}

impl AppState {
    /// Determine the cursor icon for the winit (non-egui) area at the given position.
    /// Checks dividers first, then asks the surface. Returns None if not over any winit area.
    #[cfg(feature = "gui")]
    pub fn winit_cursor_icon_at(
        &self,
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> Option<egui::CursorIcon> {
        // 전체화면 무대 중에는 뒤의 divider/surface 커서를 절대 돌려주지 않는다.
        // 무대는 화면 전체를 덮으므로 그 아래 좌표로 커서를 정하는 것은 유령 판정이고,
        // 무대 프레임에서는 egui 의 `platform_output.cursor_icon` 이 커서를 정한다
        // (`Gpu::render_fullscreen_stage`). 무대 콘텐츠가 정한 커서를 뒤 세계의 ↔/I-beam
        // 이 덮어쓰면 안 된다.
        if self.fullscreen_stage_active() {
            return None;
        }
        if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            return None;
        }

        // 1. Divider check
        let divider = self
            .find_pane_divider_at(engine, x, y, terminal_rect, scale_factor)
            .or_else(|| self.find_surface_divider_at(engine, x, y, terminal_rect, scale_factor));
        if let Some(info) = divider {
            return Some(match info.direction {
                SplitDirection::Vertical => egui::CursorIcon::ResizeHorizontal,
                SplitDirection::Horizontal => egui::CursorIcon::ResizeVertical,
            });
        }

        // 2. Surface check — terminal surface는 텍스트 커서, 그 외는 기본.
        for (_pane_id, _pane_rect, regions) in
            &self.surface_regions(engine, terminal_rect, scale_factor)
        {
            for r in regions {
                if r.rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                    let _local = (x - r.rect.x.value(), y - r.rect.y.value());
                    return if r.surface.kind() == "terminal" {
                        Some(egui::CursorIcon::Text)
                    } else {
                        None
                    };
                }
            }
        }

        None
    }

    /// Find a pane-level divider at the given position.
    pub fn find_pane_divider_at(
        &self,
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> Option<DividerInfo> {
        let ws = self.active_workspace(engine);
        ws.pane_layout().find_divider_at(
            x,
            y,
            terminal_rect,
            divider_hit_threshold_physical(scale_factor),
            scale_factor,
        )
    }

    /// Find a surface-level divider at the given position (within the focused pane's panel).
    pub fn find_surface_divider_at(
        &self,
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> Option<DividerInfo> {
        let ws = self.active_workspace(engine);
        let focused_id = ws.focused_pane;
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect, scale_factor);

        let pane_rect = pane_rects.into_iter().find(|(id, _)| *id == focused_id);
        let pane_rect = match pane_rect {
            Some((_, r)) => r,
            None => return None,
        };

        let pane = ws.pane_layout().find_pane(focused_id)?;
        let tab_bar_h = self.tab_bar_height;
        let content_rect = PhysicalRect {
            x: pane_rect.x,
            y: pane_rect.y + tab_bar_h,
            width: pane_rect.width,
            height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
        };

        let tab = pane.tabs.get(pane.active_tab)?;
        tab.layout().find_divider_at(
            x,
            y,
            content_rect,
            divider_hit_threshold_physical(scale_factor),
            scale_factor,
        )
    }

    /// Update a pane-level split ratio based on a divider drag.
    pub fn update_pane_divider(
        &mut self,
        engine: &mut CoreState,
        divider: &DividerInfo,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> bool {
        let new_ratio = match divider.direction {
            SplitDirection::Vertical => {
                (PhysicalPx(x) - divider.split_rect.x).value() / divider.split_rect.width.value()
            }
            SplitDirection::Horizontal => {
                (PhysicalPx(y) - divider.split_rect.y).value() / divider.split_rect.height.value()
            }
        };
        let ws = self.active_workspace_mut(engine);
        let updated = ws.pane_layout_mut().update_ratio_for_rect(
            divider.split_rect,
            new_ratio,
            terminal_rect,
            scale_factor,
        );
        if updated {
            engine.mark_layout_dirty();
        }
        updated
    }

    /// Update a surface-level split ratio based on a divider drag.
    pub fn update_surface_divider(
        &mut self,
        engine: &mut CoreState,
        divider: &DividerInfo,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> bool {
        let new_ratio = match divider.direction {
            SplitDirection::Vertical => {
                (PhysicalPx(x) - divider.split_rect.x).value() / divider.split_rect.width.value()
            }
            SplitDirection::Horizontal => {
                (PhysicalPx(y) - divider.split_rect.y).value() / divider.split_rect.height.value()
            }
        };

        let tab_bar_h = self.tab_bar_height;
        let ws = self.active_workspace_mut(engine);
        let focused_id = ws.focused_pane;
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect, scale_factor);

        let pane_rect = pane_rects.into_iter().find(|(id, _)| *id == focused_id);
        let pane_rect = match pane_rect {
            Some((_, r)) => r,
            None => return false,
        };

        let pane = match ws.pane_layout_mut().find_pane_mut(focused_id) {
            Some(p) => p,
            None => return false,
        };
        let content_rect = PhysicalRect {
            x: pane_rect.x,
            y: pane_rect.y + tab_bar_h,
            width: pane_rect.width,
            height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
        };

        let tab = match pane.active_tab_mut() {
            Some(t) => t,
            None => return false,
        };

        let updated = tab.layout_mut().update_ratio_for_rect(
            divider.split_rect,
            new_ratio,
            content_rect,
            scale_factor,
        );
        if updated {
            engine.mark_layout_dirty();
        }
        updated
    }
}

/// 클릭 축 트래킹의 **순수 결정 로직** — 터미널이 켜 둔 트래킹을 마우스 핸들러가
/// 실제로 존중할지 정한다. hard 점유(readonly)이거나 마우스 캡처 블랙리스트에 걸리면
/// 실제 모드와 무관하게 `None` 으로 격하한다. 그래야 `left_click_local_select` 가 항상
/// 로컬 선택으로 떨어진다.
///
/// **왜 gui 게이트 밖에 있나.** 소비자가 둘이다 — `view::main::mouse`(실제 라우팅)와
/// `surface.mouse_tracking` IPC(관측면). 관측면이 이 격하를 못 보면 "터미널이 켰다" 를
/// "핸들러가 존중한다" 로 읽게 되고, 그 둘은 블랙리스트가 빈 기계에서만 우연히 같다.
/// 복제하지 않고 함수를 게이트 밖으로 올린 것은 `cell_palette` 와 같은 처방이다
/// (`docs/dev-guide/debug-ipc.md` — 렌더러와 **같은 함수**를 부르는 것이 정의라 복제는 답이 아니다).
pub fn effective_click_tracking_decision(
    is_hard_occupied: bool,
    capture_disabled: bool,
    actual: tasty_terminal::MouseTrackingMode,
) -> tasty_terminal::MouseTrackingMode {
    if is_hard_occupied || capture_disabled {
        tasty_terminal::MouseTrackingMode::None
    } else {
        actual
    }
}

#[cfg(test)]
mod effective_click_tracking_tests {
    //! 격하 판정의 대조. **함수와 같은 자리에 둔다** — 함수는 gui 게이트 밖으로 올렸는데
    //! 시험만 `view::main::mouse`(gui) 에 남기면 헤드리스 조합의 모수가 이 대조를 안 담아,
    //! 그 조합에서는 무대조인 채로 초록이 된다.
    use super::effective_click_tracking_decision;
    use tasty_terminal::MouseTrackingMode;

    #[test]
    fn hard_occupied_forces_tracking_none_even_if_actually_on() {
        // hard 점유(readonly): live 트래킹이 켜져 있어도(AllMotion 등) 항상 None 으로
        // 격하해 로컬 선택으로 떨어져야 한다 — 조용한 무동작(앱 보고 스킵)을 방지.
        assert_eq!(
            effective_click_tracking_decision(true, false, MouseTrackingMode::AllMotion),
            MouseTrackingMode::None
        );
        assert_eq!(
            effective_click_tracking_decision(true, false, MouseTrackingMode::CellMotion),
            MouseTrackingMode::None
        );
    }

    #[test]
    fn hard_occupied_and_capture_disabled_both_force_none() {
        // 두 조건은 or — 어느 한쪽만 참이어도 None.
        assert_eq!(
            effective_click_tracking_decision(true, true, MouseTrackingMode::Click),
            MouseTrackingMode::None
        );
        assert_eq!(
            effective_click_tracking_decision(false, true, MouseTrackingMode::Click),
            MouseTrackingMode::None
        );
    }

    #[test]
    fn not_occupied_and_not_disabled_keeps_actual_tracking() {
        assert_eq!(
            effective_click_tracking_decision(false, false, MouseTrackingMode::CellMotion),
            MouseTrackingMode::CellMotion
        );
        assert_eq!(
            effective_click_tracking_decision(false, false, MouseTrackingMode::None),
            MouseTrackingMode::None
        );
    }
}
