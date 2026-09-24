use crate::core::CoreState;
#[cfg(feature = "gui")]
use crate::model::{PaneId, PhysicalPx, PhysicalRect, SurfaceRegion};

use super::AppState;

impl AppState {
    /// Compute all surface regions for the active workspace.
    /// Returns: for each pane, the pane rect and all surface regions within it.
    #[cfg(feature = "gui")]
    pub fn surface_regions<'a>(
        &self,
        engine: &'a CoreState,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> Vec<(PaneId, PhysicalRect, Vec<SurfaceRegion<'a>>)> {
        let ws = self.active_workspace(engine);
        let pane_rects = ws.pane_layout().compute_rects(terminal_rect, scale_factor);

        let mut result = Vec::new();
        for (pane_id, pane_rect) in pane_rects {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                let tab_bar_h = self.tab_bar_height;
                let content_rect = PhysicalRect {
                    x: pane_rect.x,
                    y: pane_rect.y + tab_bar_h,
                    width: pane_rect.width,
                    height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
                };
                let regions = match pane.tabs.get(pane.active_tab) {
                    Some(tab) => tab.surface_regions(content_rect),
                    None => Vec::new(),
                };
                result.push((pane_id, pane_rect, regions));
            }
        }
        result
    }

    /// 보이지 않는 탭도 포함해 모든 egui-mesh surface의 ID와 소유 플러그인을 반환한다.
    /// 텍스처·전송 상태는 가시성이 아니라 surface 수명에 맞춰 유지해야 한다.
    #[cfg(feature = "gui")]
    pub fn egui_mesh_surfaces_existing(&self, engine: &CoreState) -> Vec<(u32, String)> {
        use crate::core::egui_mesh_surface::EguiMeshSurface;
        let mut out: Vec<(u32, String)> = Vec::new();
        for ws in &engine.workspaces {
            for pane_id in ws.pane_layout().all_pane_ids() {
                let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                    continue;
                };
                for tab in &pane.tabs {
                    let Some(layout) = tab.layout_if_initialized() else {
                        continue;
                    };
                    for sid in layout.all_surface_ids() {
                        if let Some(s) = layout.find_surface(sid)
                            && let Some(ms) = s.as_any().downcast_ref::<EguiMeshSurface>()
                        {
                            out.push((sid, ms.plugin_id.clone()));
                        }
                    }
                }
            }
        }
        out
    }

    /// 모든 탭에 있는 attach mesh mirror의 로컬 ID. 로컬 플러그인 프로세스는 조회하지 않는다.
    #[cfg(feature = "gui")]
    pub fn attach_mesh_surfaces_existing(&self, engine: &CoreState) -> Vec<u32> {
        use crate::model::AttachMeshSurface;
        let mut out: Vec<u32> = Vec::new();
        for ws in &engine.workspaces {
            for pane_id in ws.pane_layout().all_pane_ids() {
                let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                    continue;
                };
                for tab in &pane.tabs {
                    let Some(layout) = tab.layout_if_initialized() else {
                        continue;
                    };
                    for sid in layout.all_surface_ids() {
                        if let Some(s) = layout.find_surface(sid)
                            && s.as_any().downcast_ref::<AttachMeshSurface>().is_some()
                        {
                            out.push(sid);
                        }
                    }
                }
            }
        }
        out
    }

    /// 활성 워크스페이스의 각 활성 탭에서 지연된 surface 초기화를 시도한다.
    /// 입력 경로마다 복원 처리를 넣는 대신 그리기 전에 한 번 순회한다.
    pub fn reify_displayed_surfaces(&self, engine: &mut CoreState) {
        if engine.workspaces.is_empty() {
            return;
        }
        let idx = self
            .active_workspace
            .min(engine.workspaces.len().saturating_sub(1));
        let mut deferred: Vec<u32> = Vec::new();
        {
            let ws = &engine.workspaces[idx];
            for pane_id in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id)
                    && let Some(tab) = pane.tabs.get(pane.active_tab)
                {
                    deferred.extend(tab.deferred_surface_ids());
                }
            }
        }
        for sid in deferred {
            if !engine.ensure_surface_initialized(sid) {
                engine.reify_plugin_surface(sid);
            }
        }
    }

    /// Get the actual content rect for the focused surface (accounting for tab bar).
    /// Returns None if no surface is focused.
    #[cfg(feature = "gui")]
    pub fn focused_surface_rect(
        &self,
        engine: &CoreState,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> Option<PhysicalRect> {
        let surface_id = self.focused_surface_id(engine)?;
        for (_pane_id, _pane_rect, regions) in
            &self.surface_regions(engine, terminal_rect, scale_factor)
        {
            for r in regions {
                if r.id == surface_id {
                    return Some(r.rect);
                }
            }
        }
        None
    }

    /// Get the physical pixel rect of a specific terminal cell within a surface.
    #[allow(clippy::too_many_arguments)] // reason: 셀 위치 계산에 surface·행·열과 화면 좌표 정보가 함께 필요하다
    #[cfg(feature = "gui")]
    pub fn surface_cell_rect(
        &self,
        engine: &CoreState,
        terminal_rect: PhysicalRect,
        surface_id: u32,
        col: usize,
        row: usize,
        cell_w: f32,
        cell_h: f32,
        scale_factor: f32,
    ) -> Option<PhysicalRect> {
        for (_pane_id, _pane_rect, regions) in
            &self.surface_regions(engine, terminal_rect, scale_factor)
        {
            for r in regions {
                if r.id == surface_id {
                    return Some(PhysicalRect {
                        x: r.rect.x + PhysicalPx(col as f32 * cell_w),
                        y: r.rect.y + PhysicalPx(row as f32 * cell_h),
                        width: PhysicalPx(cell_w.max(1.0)),
                        height: PhysicalPx(cell_h.max(1.0)),
                    });
                }
            }
        }
        None
    }

    /// Get the rect of a specific surface by id.
    #[cfg(feature = "gui")]
    pub fn surface_rect_by_id(
        &self,
        engine: &CoreState,
        surface_id: u32,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> Option<PhysicalRect> {
        for (_pane_id, _pane_rect, regions) in
            &self.surface_regions(engine, terminal_rect, scale_factor)
        {
            for r in regions {
                if r.id == surface_id {
                    return Some(r.rect);
                }
            }
        }
        None
    }

    /// Find the surface ID at the given physical pixel position.
    #[cfg(feature = "gui")]
    pub fn surface_id_at_position(
        &self,
        engine: &CoreState,
        x: f32,
        y: f32,
        terminal_rect: PhysicalRect,
        scale_factor: f32,
    ) -> Option<u32> {
        for (_pane_id, _pane_rect, regions) in
            &self.surface_regions(engine, terminal_rect, scale_factor)
        {
            for r in regions {
                if r.rect.contains(PhysicalPx(x), PhysicalPx(y)) {
                    return Some(r.id);
                }
            }
        }
        None
    }

    /// 점유·mirror 처리를 포함한 Core::resize_all_terminals에 위임한다.
    /// PTY resize는 미뤄지므로 호출자가 resize 이벤트 처리 후 flush_all_pty_resizes를 호출해야 한다.
    #[cfg(feature = "gui")]
    pub fn resize_all(
        &mut self,
        engine: &mut CoreState,
        terminal_rect: PhysicalRect,
        cell_width: f32,
        cell_height: f32,
        scale_factor: f32,
    ) {
        crate::core::Core::resize_all_terminals(
            self.tab_bar_height,
            engine,
            terminal_rect,
            cell_width,
            cell_height,
            scale_factor,
        );
    }
}
