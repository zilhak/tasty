//! 저장 레이아웃을 engine에 복원한다. plugin kind는 실제 등록된 생성기가 있어야 복원할 수 있다.
//! 호출자가 required_plugin_kinds로 필요한 종류를 확인하고 plugin 준비를 기다린다.

#[cfg(any(feature = "gui", test))]
use std::path::PathBuf;

#[cfg(any(feature = "gui", test))]
use crate::core::CoreState;
#[cfg(any(feature = "gui", test))]
use crate::core::state::ShellConfig;
#[cfg(any(feature = "gui", test))]
use crate::model::{Pane, PaneNode, Surface, SurfaceLayout, Tab, TerminalSurface, Workspace};

use super::schema::{
    SavedLayout, SavedPane, SavedPaneNode, SavedSurface, SavedSurfaceLayout, SavedTab,
    SavedWorkspace,
};
#[cfg(any(feature = "gui", test))]
use super::scrollback::queue_scrollback_for_surface;

impl SavedLayout {
    /// 호출자가 plugin 준비 여부를 확인할 Generic 종류 목록.
    #[cfg(feature = "gui")]
    pub fn required_plugin_kinds(&self) -> Vec<String> {
        let mut kinds = std::collections::HashSet::new();
        for ws in &self.workspaces {
            Self::collect_kinds_in_pane(&ws.pane_layout, &mut kinds);
        }
        kinds.into_iter().collect()
    }

    /// 이 슬롯의 scrollback 참조만 모은다. GC는 다른 창의 파일을 지우지 않도록 모든 슬롯과 합쳐야 한다.
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

    #[cfg(feature = "gui")]
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

    #[cfg(feature = "gui")]
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

    /// workspace를 하나라도 복원하면 true다. 실패한 workspace는 생략한다.
    /// false여도 이미 발급한 ID·생성한 터미널·메타데이터 등의 변경을 되돌리지는 않는다.
    #[cfg(any(feature = "gui", test))]
    pub fn restore(self, engine: &mut CoreState) -> bool {
        if self.workspaces.is_empty() {
            return false;
        }

        let active_idx = self.active_workspace.min(self.workspaces.len() - 1);
        // categories가 없던 저장 형식도 정상 분류에 넣도록 마지막에 정규화한다.
        let categories: Vec<crate::model::WorkspaceCategory> = self
            .categories
            .into_iter()
            .map(|c| crate::model::WorkspaceCategory {
                id: c.id,
                name: c.name,
                collapsed: c.collapsed,
            })
            .collect();
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
        engine.categories = categories;
        engine.ensure_normal_category();
        engine.restored_active_workspace = Some(active);
        true
    }
}

impl SavedWorkspace {
    #[cfg(any(feature = "gui", test))]
    fn restore(self, engine: &mut CoreState, is_active: bool) -> Option<Workspace> {
        let ws_id = engine.next_ids.next_workspace();
        let pane_layout = self.pane_layout.restore(engine, is_active)?;

        let all_ids = pane_layout.all_pane_ids();
        let focused_pane = all_ids
            .get(self.focused_pane_index)
            .copied()
            .or_else(|| all_ids.first().copied())
            .unwrap_or(0);

        let mut ws =
            Workspace::from_restored(ws_id, self.name, self.subtitle, pane_layout, focused_pane);
        ws.set_attach_mapping(self.attach_mapping);
        // 없는 category ID는 전체 복원 뒤 normal로 바꾼다.
        ws.set_category(self.category);
        Some(ws)
    }
}

impl SavedPaneNode {
    #[cfg(any(feature = "gui", test))]
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
    #[cfg(any(feature = "gui", test))]
    fn restore(self, engine: &mut CoreState, is_active_workspace: bool) -> Option<Pane> {
        let pane_id = engine.next_ids.next_pane();
        let saved_active_tab = self.active_tab.min(self.tabs.len().saturating_sub(1));
        let tab_count = self.tabs.len();
        let mut tabs = Vec::new();
        for (idx, saved_tab) in self.tabs.into_iter().enumerate() {
            // 활성 workspace의 활성 탭만 PTY를 즉시 만들고 나머지는 선택될 때까지 미룬다.
            let tab_is_active = is_active_workspace && idx == saved_active_tab;
            match saved_tab.restore(engine, tab_is_active) {
                Some(tab) => tabs.push(tab),
                None => {
                    tracing::warn!(
                        "failed to restore tab {}/{} in pane {pane_id} — skipping it; \
                         the surface kind may be unregistered (plugin not loaded?)",
                        idx + 1,
                        tab_count,
                    );
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
    #[cfg(any(feature = "gui", test))]
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
    /// 비활성 탭의 Terminal leaf는 placeholder로 남긴다. Generic은 활성 여부와 무관하게 복원을 시도한다.
    #[cfg(any(feature = "gui", test))]
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
    #[cfg(any(feature = "gui", test))]
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
                // 이후 실제 터미널을 capture할 때 사용할 복원 명령도 메타데이터에 기록한다.
                // 아직 deferred인 동안의 capture는 DeferredSpawn 값을 읽는다.
                if let Some(cmd) = restore_command.as_deref() {
                    let mut guard = crate::poison::recover_mutex(
                        engine.memory.lock(),
                        crate::core::MEMORY_WHAT,
                        &crate::core::MEMORY_POISONED,
                    );
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
                // PTY 생성 뒤 적용할 scrollback을 준비하고 저장 ID도 다음 capture에 이어 쓴다.
                if let Some(persist_id) = scrollback_ref.as_deref() {
                    queue_scrollback_for_surface(engine, surface_id, persist_id);
                }
                let spawn = crate::model::DeferredSpawn {
                    shell: sh.shell_ref().map(|s| s.to_string()),
                    shell_args: sh.args_ref().iter().map(|s| s.to_string()).collect(),
                    extra_env: sh
                        .envs_ref()
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    cols: engine.default_cols,
                    rows: engine.default_rows,
                    waker,
                    working_dir: cwd.as_ref().map(PathBuf::from),
                    restore_command,
                    scrollback_persist_id: scrollback_ref,
                };
                let placeholder = crate::model::EmptySurface::new_deferred(surface_id, spawn);
                Some(Box::new(placeholder))
            }
            other => other.restore_immediate_inner(engine, surface_id),
        }
    }

    #[cfg(any(feature = "gui", test))]
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
                restore_terminal_immediate(engine, surface_id, cwd, restore_command, scrollback_ref)
            }
            SavedSurface::Generic { kind, data } => {
                restore_generic_immediate(engine, surface_id, kind, data)
            }
        }
    }
}

#[cfg(any(feature = "gui", test))]
fn restore_terminal_immediate(
    engine: &mut CoreState,
    surface_id: u32,
    cwd: Option<String>,
    restore_command: Option<String>,
    scrollback_ref: Option<String>,
) -> Option<Box<dyn Surface>> {
    let sh = ShellConfig::from_settings(&engine.settings);
    let waker = engine.make_waker(surface_id);
    let working_dir = cwd.as_ref().map(PathBuf::from);
    // 복원 명령을 생성 시 초기 입력으로 전달한다. 자식의 첫 read나 명령 실행 성공을 보장하지는 않는다.
    let initial = restore_command.as_deref().map(|c| format!("{c}\r"));
    let initial_input = initial.as_deref();
    let mut terminal = match tasty_terminal::Terminal::new(
        tasty_terminal::TerminalConfig {
            cols: engine.default_cols,
            rows: engine.default_rows,
            shell: sh.shell_ref(),
            args: &sh.args_ref(),
            extra_env: &sh.envs_ref(),
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
    // 저장된 scrollback과 ID를 이어 받아 다음 capture에서 같은 파일을 사용할 수 있게 한다.
    if let Some(persist_id) = scrollback_ref.as_deref()
        && let crate::scrollback_store::ScrollbackRead::Loaded(lines) =
            crate::scrollback_store::read(persist_id)
        && !lines.is_empty()
    {
        terminal.inject_scrollback(lines);
        // 화면 위쪽 절반을 이전 내용으로 채워 새 prompt와 구별한다.
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

#[cfg(any(feature = "gui", test))]
fn restore_generic_immediate(
    engine: &mut CoreState,
    surface_id: u32,
    kind: String,
    data: serde_json::Value,
) -> Option<Box<dyn Surface>> {
    let registry = engine.surface_registry.clone();
    let def = match registry.get_live(&kind) {
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
