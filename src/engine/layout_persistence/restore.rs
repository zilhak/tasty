//! `SavedLayout` → live `CoreState` 복원.
//!
//! Plugin surface (`SavedSurface::Generic`) 는 그 kind 가 registry 에 등록된 후에만
//! 복원 가능. 호출자가 `required_plugin_kinds()` 로 미리 필요한 kind 목록을 받아 plugin
//! pump 를 기다린 뒤 `restore()` 를 호출한다.

use std::path::PathBuf;

use crate::core::CoreState;
use crate::core::state::ShellConfig;
use crate::model::{Pane, PaneNode, Surface, SurfaceLayout, Tab, TerminalSurface, Workspace};

use super::schema::{
    SavedLayout, SavedPane, SavedPaneNode, SavedSurface, SavedSurfaceLayout, SavedTab,
    SavedWorkspace,
};
use super::scrollback::queue_scrollback_for_surface;

impl SavedLayout {
    /// Layout 안의 모든 Generic surface kind 토큰을 수집. 호출자는 첫 plugin pump
    /// 후에 registry에 이 kind들이 등록됐는지 확인하여 복원 시점을 결정한다.
    pub fn required_plugin_kinds(&self) -> Vec<String> {
        let mut kinds = std::collections::HashSet::new();
        for ws in &self.workspaces {
            Self::collect_kinds_in_pane(&ws.pane_layout, &mut kinds);
        }
        kinds.into_iter().collect()
    }

    /// layout.json 안의 모든 `SavedSurface::Terminal { scrollback_ref: Some(_) }` 값을
    /// 모은다. 앱 시작 시 `scrollback_store::gc_orphans` 의 입력으로 사용 — 이 집합에
    /// 없는 디스크 파일은 모두 orphan 으로 간주해 삭제된다.
    pub fn collect_scrollback_refs(&self) -> std::collections::HashSet<String> {
        let mut refs = std::collections::HashSet::new();
        for ws in &self.workspaces {
            Self::collect_scrollback_refs_in_pane(&ws.pane_layout, &mut refs);
        }
        refs
    }

    fn collect_scrollback_refs_in_pane(
        node: &SavedPaneNode,
        out: &mut std::collections::HashSet<String>,
    ) {
        match node {
            SavedPaneNode::Leaf(pane) => {
                for tab in &pane.tabs {
                    Self::collect_scrollback_refs_in_layout(&tab.surface, out);
                }
            }
            SavedPaneNode::Split { first, second, .. } => {
                Self::collect_scrollback_refs_in_pane(first, out);
                Self::collect_scrollback_refs_in_pane(second, out);
            }
        }
    }

    fn collect_scrollback_refs_in_layout(
        layout: &SavedSurfaceLayout,
        out: &mut std::collections::HashSet<String>,
    ) {
        match layout {
            SavedSurfaceLayout::Leaf(SavedSurface::Terminal {
                scrollback_ref: Some(id),
                ..
            }) => {
                out.insert(id.clone());
            }
            SavedSurfaceLayout::Leaf(_) => {}
            SavedSurfaceLayout::Split { first, second, .. } => {
                Self::collect_scrollback_refs_in_layout(first, out);
                Self::collect_scrollback_refs_in_layout(second, out);
            }
        }
    }

    fn collect_kinds_in_pane(node: &SavedPaneNode, out: &mut std::collections::HashSet<String>) {
        match node {
            SavedPaneNode::Leaf(pane) => {
                for tab in &pane.tabs {
                    Self::collect_kinds_in_layout(&tab.surface, out);
                }
            }
            SavedPaneNode::Split { first, second, .. } => {
                Self::collect_kinds_in_pane(first, out);
                Self::collect_kinds_in_pane(second, out);
            }
        }
    }

    fn collect_kinds_in_layout(
        layout: &SavedSurfaceLayout,
        out: &mut std::collections::HashSet<String>,
    ) {
        match layout {
            SavedSurfaceLayout::Leaf(SavedSurface::Generic { kind, .. }) => {
                out.insert(kind.clone());
            }
            SavedSurfaceLayout::Leaf(_) => {}
            SavedSurfaceLayout::Split { first, second, .. } => {
                Self::collect_kinds_in_layout(first, out);
                Self::collect_kinds_in_layout(second, out);
            }
        }
    }

    /// Restore layout into engine state. Returns true on success.
    /// On failure, engine state is left unchanged (caller should create default workspace).
    pub fn restore(self, engine: &mut CoreState) -> bool {
        if self.workspaces.is_empty() {
            return false;
        }

        let active_idx = self.active_workspace.min(self.workspaces.len() - 1);
        let mut workspaces = Vec::new();
        for (i, saved_ws) in self.workspaces.into_iter().enumerate() {
            let name = saved_ws.name.clone();
            let is_active = i == active_idx;
            match saved_ws.restore(engine, is_active) {
                Some(ws) => workspaces.push(ws),
                None => {
                    tracing::warn!("Failed to restore workspace '{}', skipping", name);
                }
            }
        }

        if workspaces.is_empty() {
            return false;
        }

        let active = self.active_workspace.min(workspaces.len() - 1);
        engine.workspaces = workspaces;
        engine.restored_active_workspace = Some(active);
        true
    }
}

impl SavedWorkspace {
    fn restore(self, engine: &mut CoreState, is_active: bool) -> Option<Workspace> {
        let ws_id = engine.next_ids.next_workspace();
        let pane_layout = self.pane_layout.restore(engine, is_active)?;

        // Resolve focused pane by index.
        let all_ids = pane_layout.all_pane_ids();
        let focused_pane = all_ids
            .get(self.focused_pane_index)
            .copied()
            .or_else(|| all_ids.first().copied())
            .unwrap_or(0);

        let mut ws =
            Workspace::from_restored(ws_id, self.name, self.subtitle, pane_layout, focused_pane);
        // 단계 7 — 매핑 복원(재시작 후 활성화 시 자동 재attach). 생성자 churn 0(setter).
        ws.set_attach_mapping(self.attach_mapping);
        Some(ws)
    }
}

impl SavedPaneNode {
    fn restore(self, engine: &mut CoreState, is_active: bool) -> Option<PaneNode> {
        match self {
            SavedPaneNode::Leaf(saved_pane) => {
                let pane = saved_pane.restore(engine, is_active)?;
                Some(PaneNode::Leaf(pane))
            }
            SavedPaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let first = first.restore(engine, is_active)?;
                let second = second.restore(engine, is_active)?;
                Some(PaneNode::Split {
                    direction: direction.into(),
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                })
            }
        }
    }
}

impl SavedPane {
    fn restore(self, engine: &mut CoreState, is_active_workspace: bool) -> Option<Pane> {
        let pane_id = engine.next_ids.next_pane();
        let saved_active_tab = self.active_tab.min(self.tabs.len().saturating_sub(1));
        let mut tabs = Vec::new();
        for (idx, saved_tab) in self.tabs.into_iter().enumerate() {
            // 활성 workspace 안에서도 사용자가 보고 있는 active_tab만 즉시 PTY spawn.
            // 나머지 tab은 비활성 workspace와 동일하게 deferred — tab 전환 시 깨워짐.
            let tab_is_active = is_active_workspace && idx == saved_active_tab;
            match saved_tab.restore(engine, tab_is_active) {
                Some(tab) => tabs.push(tab),
                None => {
                    tracing::warn!("Failed to restore tab, skipping");
                }
            }
        }
        if tabs.is_empty() {
            return None;
        }
        let active_tab = saved_active_tab.min(tabs.len() - 1);
        Some(Pane {
            id: pane_id,
            tabs,
            active_tab,
            tab_scroll_offset: 0.0,
        })
    }
}

impl SavedTab {
    fn restore(self, engine: &mut CoreState, is_active: bool) -> Option<Tab> {
        let tab_id = engine.next_ids.next_tab();
        let layout = self.surface.restore(engine, is_active)?;
        let focused_surface = layout.first_surface_id().unwrap_or(0);
        Some(Tab {
            id: tab_id,
            name: self.name,
            explicit_name: self.explicit_name,
            layout_opt: Some(layout),
            focused_surface,
            osc_title: None,
            cached_display_name: None,
        })
    }
}

impl SavedSurfaceLayout {
    /// is_active=false면 Terminal leaf를 deferred EmptySurface placeholder로 변환한다.
    /// is_active=true면 모든 leaf를 즉시 spawn한다. Split 노드는 재귀적으로 처리해
    /// 비활성 split 내부의 Terminal들도 deferred로 남는다.
    fn restore(self, engine: &mut CoreState, is_active: bool) -> Option<SurfaceLayout> {
        match self {
            SavedSurfaceLayout::Leaf(saved) => {
                let surface = saved.restore_leaf(engine, is_active)?;
                Some(SurfaceLayout::Leaf(surface))
            }
            SavedSurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let first = first.restore(engine, is_active)?;
                let second = second.restore(engine, is_active)?;
                Some(SurfaceLayout::Split {
                    direction: direction.into(),
                    ratio,
                    first: Box::new(first),
                    second: Box::new(second),
                    focus_second: false,
                })
            }
        }
    }
}

impl SavedSurface {
    /// 단일 leaf 복원. Terminal이면 is_active에 따라 즉시 spawn 또는 deferred placeholder.
    /// Generic surface는 is_active와 관계없이 즉시 복원 (PTY가 아니므로 cheap).
    fn restore_leaf(self, engine: &mut CoreState, is_active: bool) -> Option<Box<dyn Surface>> {
        let surface_id = engine.next_ids.next_surface();
        match self {
            SavedSurface::Terminal {
                cwd,
                restore_command,
                scrollback_ref,
            } if !is_active => {
                let sh = ShellConfig::from_settings(&engine.settings);
                let waker = engine.make_waker(surface_id);
                // capture 단계가 surface_meta 의 restore.command 를 읽으므로
                // (capture_surface 의 deferred 분기 참조), DeferredSpawn 으로 옮기기
                // 전에 동일 값을 meta 에도 mirror 한다.
                if let Some(cmd) = restore_command.as_deref() {
                    let mut guard = match engine.memory.lock() {
                        Ok(g) => g,
                        Err(p) => p.into_inner(),
                    };
                    if let Err(e) = crate::surface_meta::SurfaceMetaStore::set(
                        &mut *guard,
                        surface_id,
                        "restore.command",
                        cmd,
                    ) {
                        tracing::warn!(
                            "restore: failed to mirror restore.command for surface {surface_id}: {e}"
                        );
                    }
                }
                // scrollback_ref 가 있으면 PTY spawn 시점에 inject 할 라인을 큐에 쌓고,
                // 동일한 persist_id 를 DeferredSpawn 에도 들고 있어 spawn 후 새
                // TerminalSurface 의 scrollback_persist_id 필드로 이관한다.
                if let Some(persist_id) = scrollback_ref.as_deref() {
                    queue_scrollback_for_surface(engine, surface_id, persist_id);
                }
                let spawn = crate::model::DeferredSpawn {
                    shell: sh.shell_ref().map(|s| s.to_string()),
                    shell_args: sh.args_ref().iter().map(|s| s.to_string()).collect(),
                    cols: engine.default_cols,
                    rows: engine.default_rows,
                    waker,
                    working_dir: cwd.as_ref().map(PathBuf::from),
                    // PTY 가 실제로 spawn 되는 순간 inline 으로 send_key 된다 (ensure_initialized).
                    restore_command,
                    scrollback_persist_id: scrollback_ref,
                };
                let placeholder = crate::model::EmptySurface::new_deferred(surface_id, spawn);
                Some(Box::new(placeholder))
            }
            other => other.restore_immediate_inner(engine, surface_id),
        }
    }

    /// 항상 즉시 PTY를 spawn하거나 generic surface를 만들어 반환.
    fn restore_immediate_inner(
        self,
        engine: &mut CoreState,
        surface_id: u32,
    ) -> Option<Box<dyn Surface>> {
        match self {
            SavedSurface::Terminal {
                cwd,
                restore_command,
                scrollback_ref,
            } => {
                let sh = ShellConfig::from_settings(&engine.settings);
                let waker = engine.make_waker(surface_id);
                let working_dir = cwd.as_ref().map(PathBuf::from);
                // PTY master 의 첫 입력으로 restore_command 를 미리 적재한다.
                // Terminal::new 가 writer thread spawn 전에 동기 write 하므로,
                // child shell 이 stdin 을 처음 read 하는 순간 이 바이트가 들어간다.
                let initial = restore_command.as_deref().map(|c| format!("{c}\r"));
                let initial_input = initial.as_deref();
                let mut terminal = match tasty_terminal::Terminal::new(
                    tasty_terminal::TerminalConfig {
                        cols: engine.default_cols,
                        rows: engine.default_rows,
                        shell: sh.shell_ref(),
                        args: &sh.args_ref(),
                        surface_id,
                        working_dir: working_dir.as_deref(),
                        initial_input,
                    },
                    waker,
                ) {
                    Ok(t) => t,
                    Err(e) => {
                        tracing::warn!("Failed to create terminal for restored surface: {e}");
                        return None;
                    }
                };
                // 즉시 복원 경로 — scrollback 을 inline 으로 inject. persist_id 는
                // 새 TerminalSurface 의 필드에 직접 들어가 (surface_meta mirror 없이)
                // 다음 capture 가 같은 ID 를 재사용한다.
                if let Some(persist_id) = scrollback_ref.as_deref()
                    && let Some(lines) = crate::scrollback_store::read(persist_id)
                    && !lines.is_empty()
                {
                    terminal.inject_scrollback(lines);
                    // 새 prompt 가 화면 중간부터 시작하도록 visible 상단
                    // 절반에 옛 라인을 미리 그려둔다.
                    let prefill = terminal.rows() / 2;
                    terminal.prefill_visible_from_scrollback(prefill);
                }
                engine.terminals.insert(surface_id, terminal);
                if let Some(pid) = scrollback_ref {
                    engine.terminals.set_scrollback_persist_id(surface_id, pid);
                }
                engine.send_fast_init(surface_id);
                Some(Box::new(TerminalSurface { id: surface_id }))
            }
            SavedSurface::Generic { kind, data } => {
                let registry = engine.surface_registry.clone();
                let def = match registry.get(&kind) {
                    Some(d) => d,
                    None => {
                        tracing::warn!(
                            "Generic restore skipped: unknown kind '{}' (plugin not loaded?)",
                            kind
                        );
                        return None;
                    }
                };
                match (def.restore)(surface_id, &data) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        tracing::warn!("Generic restore failed (kind={kind}): {e}");
                        None
                    }
                }
            }
        }
    }
}
