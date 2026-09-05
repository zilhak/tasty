use crate::core::CoreState;
use crate::model::{PaneId, PhysicalPx, PhysicalRect, SurfaceRegion};

use super::AppState;

impl AppState {
    /// Compute all surface regions for the active workspace.
    /// Returns: for each pane, the pane rect and all surface regions within it.
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

    /// 전체 workspace 의 모든 tab(비활성 탭 포함)에 **존재**하는 egui-mesh surface 일람
    /// (surface_id, plugin_id).
    ///
    /// [`Self::surface_regions`] 가 "활성 workspace 의 활성 탭"(=화면에 보이는 surface)만
    /// 순회하는 것과 달리, 이 함수는 layout 존재 기반이다 — egui-mesh 텍스처 상태의
    /// surface 수명 귀속(렌더 prepare 의 retain / 비가시 디코드)과 forward 추적 상태
    /// retain 에 쓴다. 탭 전환/workspace 전환으로 안 보이게 된 surface 의 텍스처 상태를
    /// 파괴하지 않기 위한 열거다.
    pub fn egui_mesh_surfaces_existing(&self, engine: &CoreState) -> Vec<(u32, String)> {
        use crate::plugin_bridge::egui_mesh_surface::EguiMeshSurface;
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

    /// 전체 workspace 의 모든 tab(비활성 탭 포함)에 **존재**하는 attach mesh mirror
    /// surface(`AttachMeshSurface`) local id 일람. [`Self::egui_mesh_surfaces_existing`]의
    /// attach 대응 — plugin_id 는 `PluginManager` 조회에 쓰이지 않으므로(로컬에 plugin
    /// 프로세스가 없다) id 만 반환한다.
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

    /// Reify (lazy PTY spawn) every deferred placeholder that is about to be
    /// drawn this frame: the active workspace's panes, each pane's active tab,
    /// and every deferred leaf within it.
    ///
    /// This is the single display-point that enforces the invariant "a surface
    /// visible on screen has a live PTY". Calling it once per frame (before the
    /// render passes) covers every exposure path at once — keyboard tab switch
    /// (next/prev/goto), tab close moving active_tab onto a deferred tab, pane
    /// focus change, workspace switch, window restore — without scattering reify
    /// hooks across each input handler.
    ///
    /// Cheap when nothing is deferred: it only walks the active workspace's
    /// active-tab layout trees (`deferred_surface_ids` returns early per tab).
    /// `ensure_surface_initialized` is a no-op for already-reified surfaces, so
    /// there is no double-spawn with the existing eager reify paths (mouse tab
    /// click / workspace switch / boot).
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
            // terminal placeholder 먼저 시도. false 면 terminal 이 아니라는 뜻이므로
            // plugin placeholder 실제화를 시도한다(둘 다 아니면 no-op — 이미 reify됨).
            if !engine.ensure_surface_initialized(sid) {
                engine.reify_plugin_surface(sid);
            }
        }
    }

    /// Get the actual content rect for the focused surface (accounting for tab bar).
    /// Returns None if no surface is focused.
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
    #[allow(clippy::too_many_arguments)] // reason: cell geometry lookup 컨텍스트
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

    /// Resize all terminals in all workspaces and all tabs to match a given terminal rect.
    ///
    /// **단일 authority 위임**: 터미널 grid 리사이즈 로직은 `Core::resize_all_terminals`
    /// 한 곳으로 통일한다(attach 의미론 — detached mirror 는 원격 forward, hard-점유
    /// surface 는 skip — 을 정본 한 벌만 보유). 과거 이 메서드가 자체 sweep 을 갖고
    /// 있었으나 정본과 갈라져(occupancy 가드 누락) GUI-hosted 서버에서 점유 surface 를
    /// 매 프레임 창 grid 로 되돌리는 레터박스를 유발했다(ADR-0045). 렌더/입력 경로의
    /// 진입점만 유지하고 구현은 위임한다.
    ///
    /// PTY resize 는 Terminal 내부에서 deferred(`pending_pty_resize`) — 호출자는
    /// resize 이벤트가 settle 되면 `flush_all_pty_resizes()` 를 별도로 호출한다.
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
            self,
            engine,
            terminal_rect,
            cell_width,
            cell_height,
            scale_factor,
        );
    }
}
