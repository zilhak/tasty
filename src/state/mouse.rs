#[cfg(feature = "gui")]
use crate::model::{DividerInfo, LogicalPx, PhysicalPx, PhysicalRect, SplitDirection};

use super::AppState;
#[cfg(feature = "gui")]
use crate::core::CoreState;

/// 분할선 입력 영역의 논리 반폭. 드래그 시작·커서·앱 hover 보고가 같은 값을 사용한다.
#[cfg(feature = "gui")]
pub const DIVIDER_HIT_THRESHOLD: LogicalPx = LogicalPx(4.0);

/// 물리 마우스 좌표와 비교하기 위해 분할선 입력 영역을 한 번 변환한다.
#[cfg(feature = "gui")]
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
        // 전체화면 무대의 egui 커서를 아래 분할선·surface 커서로 덮지 않는다.
        if self.fullscreen_stage_active() {
            return None;
        }
        if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            return None;
        }

        let divider = self
            .find_pane_divider_at(engine, x, y, terminal_rect, scale_factor)
            .or_else(|| self.find_surface_divider_at(engine, x, y, terminal_rect, scale_factor));
        if let Some(info) = divider {
            return Some(match info.direction {
                SplitDirection::Vertical => egui::CursorIcon::ResizeHorizontal,
                SplitDirection::Horizontal => egui::CursorIcon::ResizeVertical,
            });
        }

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

    #[cfg(feature = "gui")]
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

    #[cfg(feature = "gui")]
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

    #[cfg(feature = "gui")]
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

    #[cfg(feature = "gui")]
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

/// hard 점유 또는 마우스 캡처 제한이 있으면 앱 트래킹 대신 로컬 선택을 사용한다.
/// GUI 입력과 surface.mouse_tracking 조회가 같은 판정을 사용한다.
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
    //! GUI와 헤드리스가 공통으로 사용하는 트래킹 제한 조건을 검사한다.
    use super::effective_click_tracking_decision;
    use tasty_terminal::MouseTrackingMode;

    #[test]
    fn hard_occupied_forces_tracking_none_even_if_actually_on() {
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
