use crate::model::{DividerInfo, PhysicalPx, PhysicalRect, SplitDirection};

use super::AppState;
use crate::core::CoreState;

/// divider 히트 판정 밴드의 반폭(px). press 로 드래그를 시작하는 경로, 커서 아이콘
/// 경로, 트래킹 앱 hover 보고 가드가 **같은 값**을 봐야 "커서는 ↔ 인데 TUI 는 hover 를
/// 받는" 식의 어긋남이 생기지 않는다. 세 곳에 리터럴로 흩어두면 드리프트한다.
pub const DIVIDER_HIT_THRESHOLD: f32 = 4.0;

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
        divider_threshold: f32,
    ) -> Option<egui::CursorIcon> {
        if !terminal_rect.contains(PhysicalPx(x), PhysicalPx(y)) {
            return None;
        }

        // 1. Divider check
        let divider = self
            .find_pane_divider_at(engine, x, y, terminal_rect, divider_threshold)
            .or_else(|| {
                self.find_surface_divider_at(engine, x, y, terminal_rect, divider_threshold)
            });
        if let Some(info) = divider {
            return Some(match info.direction {
                SplitDirection::Vertical => egui::CursorIcon::ResizeHorizontal,
                SplitDirection::Horizontal => egui::CursorIcon::ResizeVertical,
            });
        }

        // 2. Surface check — terminal surface는 텍스트 커서, 그 외는 기본.
        for (_pane_id, _pane_rect, regions) in &self.surface_regions(engine, terminal_rect) {
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
        threshold: f32,
    ) -> Option<DividerInfo> {
        let ws = self.active_workspace(engine);
        ws.pane_layout()
            .find_divider_at(x, y, terminal_rect, threshold)
    }

    /// Find a surface-level divider at the given position (within the focused pane's panel).
    pub fn find_surface_divider_at(
        &self,
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
        threshold: f32,
    ) -> Option<DividerInfo> {
        let ws = self.active_workspace(engine);
        let focused_id = ws.focused_pane;
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);

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
        tab.layout().find_divider_at(x, y, content_rect, threshold)
    }

    /// Update a pane-level split ratio based on a divider drag.
    pub fn update_pane_divider(
        &mut self,
        engine: &mut CoreState,
        divider: &DividerInfo,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
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
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect);

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

        let updated =
            tab.layout_mut()
                .update_ratio_for_rect(divider.split_rect, new_ratio, content_rect);
        if updated {
            engine.mark_layout_dirty();
        }
        updated
    }
}
