#[cfg(feature = "gui")]
use crate::model::{DividerInfo, LogicalPx, PhysicalPx, PhysicalRect, SplitDirection};

use super::RequestContext;
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

/// native WebView가 leaf 안에서 비워 두는 변별 여백(물리 px). 왼쪽·오른쪽·아래 순서다.
/// native 창은 마우스를 직접 받으므로 pane 콘텐츠 영역 외곽에 닿는 변에는 분할선 입력 영역만큼
/// 비워 둔다. 내부 leaf 사이에는 divider gap만 있고, 위쪽은 탭 바와 닿아 여백이 없다.
#[cfg(feature = "gui")]
pub fn webview_edge_inset(
    leaf: PhysicalRect,
    content: PhysicalRect,
    scale_factor: f32,
) -> [PhysicalPx; 3] {
    let inset = PhysicalPx(divider_hit_threshold_physical(scale_factor));
    let on_edge = |a: PhysicalPx, b: PhysicalPx| (a - b).abs() < PhysicalPx(0.5);
    let pick = |touches: bool| {
        if touches {
            inset
        } else {
            PhysicalPx::default()
        }
    };
    [
        pick(on_edge(leaf.x, content.x)),
        pick(on_edge(leaf.x + leaf.width, content.x + content.width)),
        pick(on_edge(leaf.y + leaf.height, content.y + content.height)),
    ]
}

impl RequestContext {
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
        self.layout_previews.find_divider(
            engine,
            super::layout_preview::LayoutTarget::Workspace(ws.id),
            ws.pane_layout(),
            (x, y),
            terminal_rect,
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
        let focused_id = self.navigation.pane_id(ws).unwrap_or(0);
        let pane_rects = self.pane_rects(engine, ws, terminal_rect, scale_factor);

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

        let tab = pane.tabs.get(self.navigation.tab_index(pane))?;
        self.layout_previews.find_divider(
            engine,
            super::layout_preview::LayoutTarget::Tab(tab.id),
            tab.layout(),
            (x, y),
            content_rect,
            scale_factor,
        )
    }

    #[cfg(feature = "gui")]
    pub(crate) fn begin_pane_divider(
        &mut self,
        engine: &CoreState,
        info: DividerInfo,
        rect: PhysicalRect,
        scale: f32,
    ) -> Option<u64> {
        let workspace = self.active_workspace(engine);
        self.layout_previews.begin(
            engine,
            super::layout_preview::LayoutTarget::Workspace(workspace.id),
            workspace.pane_layout(),
            info,
            rect,
            scale,
        )
    }

    #[cfg(feature = "gui")]
    pub(crate) fn begin_surface_divider(
        &mut self,
        engine: &CoreState,
        info: DividerInfo,
        rect: PhysicalRect,
        scale: f32,
    ) -> Option<u64> {
        let workspace = self.active_workspace(engine);
        let pane_id = self.navigation.pane_id(workspace)?;
        let pane_rect = self
            .pane_rects(engine, workspace, rect, scale)
            .into_iter()
            .find(|(id, _)| *id == pane_id)?
            .1;
        let pane = workspace.pane_layout().find_pane(pane_id)?;
        let tab = pane.tabs.get(self.navigation.tab_index(pane))?;
        let content = PhysicalRect {
            x: pane_rect.x,
            y: pane_rect.y + self.tab_bar_height,
            width: pane_rect.width,
            height: (pane_rect.height - self.tab_bar_height).max(PhysicalPx(1.0)),
        };
        self.layout_previews.begin(
            engine,
            super::layout_preview::LayoutTarget::Tab(tab.id),
            tab.layout(),
            info,
            content,
            scale,
        )
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
