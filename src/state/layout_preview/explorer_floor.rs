//! 탐색기 칸의 분할 하한. 디자인 Short cell 결정은 분할 경계 드래그를 `explorer-min-height` 에서 멈추게 한다.
//! 하한은 드래그로 칸을 줄일 때만 막는다. 창 크기 변경 등으로 이미 하한보다 낮은 칸은 더 줄지만 않게 한다.

use super::LayoutPreviews;
use crate::core::CoreState;
use crate::model::{PhysicalPx, PhysicalRect, SurfaceId};

/// 하한 직전 비율을 찾는 이분 탐색 횟수. 2^-16 비율은 4K 화면에서도 1px 보다 작다.
const FLOOR_SEARCH_STEPS: usize = 16;

impl LayoutPreviews {
    fn ratio(&self, sequence: u64) -> Option<f32> {
        self.entries
            .iter()
            .find(|entry| entry.commit.sequence == sequence)
            .map(|entry| entry.commit.ratio)
    }
}

impl crate::state::MainViewState {
    /// 드래그 비율을 미리보기에 반영한다. 보이는 탐색기 칸이 하한 아래로 줄어드는 비율이면
    /// 하한을 지키는 마지막 비율에서 멈춘다.
    pub(crate) fn update_divider_preview(
        &mut self,
        core: &CoreState,
        sequence: u64,
        ratio: f32,
        terminal_rect: PhysicalRect,
        scale: f32,
    ) -> bool {
        let Some(from) = self.layout_previews.ratio(sequence) else {
            return false;
        };
        let floor = crate::theme::theme()
            .explorer_min_height()
            .to_physical(scale);
        let before = self.explorer_heights(core, terminal_rect, scale);
        if !self.layout_previews.update(core, sequence, ratio) {
            return false;
        }
        if before.is_empty()
            || self.explorer_floor_holds(core, &before, floor, terminal_rect, scale)
        {
            return true;
        }
        let Some(target) = self.layout_previews.ratio(sequence) else {
            return false;
        };
        let (mut held, mut broken) = (from, target);
        for _ in 0..FLOOR_SEARCH_STEPS {
            let mid = (held + broken) * 0.5;
            self.layout_previews.update(core, sequence, mid);
            if self.explorer_floor_holds(core, &before, floor, terminal_rect, scale) {
                held = mid;
            } else {
                broken = mid;
            }
        }
        self.layout_previews.update(core, sequence, held)
    }

    /// 현재 미리보기 비율에서 각 탐색기 칸이 하한 또는 드래그 전 높이 중 작은 값 이상인가.
    fn explorer_floor_holds(
        &self,
        core: &CoreState,
        before: &[(SurfaceId, PhysicalPx)],
        floor: PhysicalPx,
        terminal_rect: PhysicalRect,
        scale: f32,
    ) -> bool {
        self.explorer_heights(core, terminal_rect, scale)
            .into_iter()
            .all(|(id, height)| {
                before
                    .iter()
                    .find(|(prev, _)| *prev == id)
                    .is_none_or(|(_, prev)| height >= floor.min(*prev))
            })
    }

    /// 각 pane 의 활성 탭에 보이는 탐색기 surface 의 높이.
    fn explorer_heights(
        &self,
        core: &CoreState,
        terminal_rect: PhysicalRect,
        scale: f32,
    ) -> Vec<(SurfaceId, PhysicalPx)> {
        let mut heights = Vec::new();
        for workspace in core.workspaces().iter() {
            for (pane_id, pane_rect) in self.pane_rects(core, workspace, terminal_rect, scale) {
                let Some(pane) = workspace.pane_layout().find_pane(pane_id) else {
                    continue;
                };
                let Some(tab) = pane.tabs.get(self.navigation.tab_index(pane)) else {
                    continue;
                };
                if tab.layout_if_initialized().is_none() {
                    continue;
                }
                let content = PhysicalRect {
                    x: pane_rect.x,
                    y: pane_rect.y + self.tab_bar_height,
                    width: pane_rect.width,
                    height: (pane_rect.height - self.tab_bar_height).max(PhysicalPx(1.0)),
                };
                heights.extend(
                    self.tab_surface_regions(core, tab, content, scale)
                        .into_iter()
                        .filter(|region| region.surface.kind == "explorer")
                        .map(|region| (region.id, region.rect.height)),
                );
            }
        }
        heights
    }
}

#[cfg(test)]
mod tests;
