//! Preset → live workspace/tab/pane 적용.
//!
//! `tasty-presets` 의 데이터 모델을 받아 엔진의 mutable 상태에 instance 를 끼워넣는다.
//! `state/restore.rs` 의 rebuild_* helpers 와 같은 패턴 (initial_input 기반 startup
//! command 주입, send_fast_init, ratio clamp 등). 단, focus 정책은 `ApplyOptions.focus`
//! 로 명시 — CLI/IPC 는 false (포커스 독립), 단축키 호출만 true.

use tasty_presets::{
    PanePreset, PresetPane, PresetPaneNode, PresetSurface, PresetSurfaceLayout, PresetTab,
    TabPreset, WorkspacePreset,
};

use crate::engine_state::EngineState;
use crate::model::{
    Pane, PaneNode, SplitDirection, Surface, SurfaceLayout, Tab, TerminalSurface, Workspace,
};

use super::AppState;

/// Apply 호출자의 의도. focus 가 true 면 새 ws/tab/pane 으로 활성 전환.
#[derive(Debug, Clone, Copy)]
pub struct ApplyOptions {
    pub focus: bool,
}

#[derive(Debug)]
pub enum ApplyError {
    UnknownKind(String),
    PaneNotFound(u32),
    WorkspaceNotFound(u32),
    Empty,
    NoActiveWorkspace,
    TerminalSpawn(String),
    Other(anyhow::Error),
}

impl std::fmt::Display for ApplyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownKind(k) => write!(f, "unknown surface kind: {k}"),
            Self::PaneNotFound(id) => write!(f, "target pane not found: {id}"),
            Self::WorkspaceNotFound(id) => write!(f, "target workspace not found: {id}"),
            Self::Empty => write!(f, "preset has no usable leaves"),
            Self::NoActiveWorkspace => {
                write!(f, "no active workspace to apply tab/pane preset")
            }
            Self::TerminalSpawn(e) => write!(f, "terminal spawn failed: {e}"),
            Self::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for ApplyError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Other(e) => Some(e.as_ref()),
            _ => None,
        }
    }
}

impl From<anyhow::Error> for ApplyError {
    fn from(e: anyhow::Error) -> Self {
        Self::Other(e)
    }
}

impl AppState {
    /// WorkspacePreset 적용 — 새 워크스페이스를 push.
    /// 반환은 새 워크스페이스의 인덱스.
    pub fn apply_workspace_preset(
        &mut self,
        engine: &mut EngineState,
        preset: &WorkspacePreset,
        opts: ApplyOptions,
    ) -> Result<usize, ApplyError> {
        let ws_id = engine.next_ids.next_workspace();
        let pane_node = self.build_pane_node(engine, &preset.layout)?;

        let all_pane_ids = pane_node.all_pane_ids();
        let focused = *all_pane_ids.first().ok_or(ApplyError::Empty)?;

        let name = if preset.name.is_empty() {
            format!("Workspace {}", engine.workspaces.len() + 1)
        } else {
            preset.name.clone()
        };

        let mut ws =
            Workspace::from_restored(ws_id, name, preset.subtitle.clone(), pane_node, focused);
        ws.description = preset.description.clone();
        engine.workspaces.push(ws);
        let idx = engine.workspaces.len() - 1;

        if opts.focus {
            self.active_workspace = idx;
        }
        engine.mark_layout_dirty();
        Ok(idx)
    }

    /// TabPreset 적용 — `target_pane_id` 가 Some 이면 해당 pane, None 이면 active workspace
    /// 의 focused_pane 에 새 탭을 push. 반환은 새 tab_id.
    pub fn apply_tab_preset(
        &mut self,
        engine: &mut EngineState,
        preset: &TabPreset,
        target_pane_id: Option<u32>,
        opts: ApplyOptions,
    ) -> Result<u32, ApplyError> {
        let (ws_idx, pane_id) = self.resolve_target_pane(engine, target_pane_id)?;

        let tab = self.build_tab(engine, &preset.tab)?;
        let tab_id = tab.id;

        let ws = &mut engine.workspaces[ws_idx];
        let pane = ws
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .ok_or(ApplyError::PaneNotFound(pane_id))?;
        pane.tabs.push(tab);
        let new_idx = pane.tabs.len() - 1;
        if opts.focus {
            pane.active_tab = new_idx;
        }

        engine.mark_layout_dirty();
        Ok(tab_id)
    }

    /// PanePreset 적용 — `target_workspace_id` 가 Some 이면 해당 ws, None 이면 active.
    /// 현재 ws 의 focused_pane 오른쪽에 vertical split 으로 새 pane 추가.
    /// 반환은 새 pane_id.
    pub fn apply_pane_preset(
        &mut self,
        engine: &mut EngineState,
        preset: &PanePreset,
        target_workspace_id: Option<u32>,
        opts: ApplyOptions,
    ) -> Result<u32, ApplyError> {
        let ws_idx = match target_workspace_id {
            Some(id) => engine
                .find_workspace_index_for_id(id)
                .ok_or(ApplyError::WorkspaceNotFound(id))?,
            None => {
                if engine.workspaces.is_empty() {
                    return Err(ApplyError::NoActiveWorkspace);
                }
                self.active_workspace.min(engine.workspaces.len() - 1)
            }
        };

        let new_pane = self.build_pane(engine, &preset.pane)?;
        let new_pane_id = new_pane.id;

        let ws = &mut engine.workspaces[ws_idx];
        let target_pane_id = ws.focused_pane;
        let remaining = ws.pane_layout_mut().split_pane_in_place(
            target_pane_id,
            SplitDirection::Vertical,
            new_pane,
        );
        if remaining.is_some() {
            // focused_pane 이 stale 이면 첫 leaf 로 fallback.
            let fallback = ws.pane_layout().first_pane().map(|p| p.id);
            if let (Some(pane), Some(fb_id)) = (remaining, fallback) {
                let remaining2 =
                    ws.pane_layout_mut()
                        .split_pane_in_place(fb_id, SplitDirection::Vertical, pane);
                if remaining2.is_some() {
                    return Err(ApplyError::PaneNotFound(target_pane_id));
                }
            } else {
                return Err(ApplyError::PaneNotFound(target_pane_id));
            }
        }

        if opts.focus {
            ws.focused_pane = new_pane_id;
        }

        engine.mark_layout_dirty();
        Ok(new_pane_id)
    }

    // ── 내부 helpers ─────────────────────────────────────────────────────

    fn resolve_target_pane(
        &self,
        engine: &EngineState,
        target_pane_id: Option<u32>,
    ) -> Result<(usize, u32), ApplyError> {
        if engine.workspaces.is_empty() {
            return Err(ApplyError::NoActiveWorkspace);
        }
        if let Some(pid) = target_pane_id {
            let ws_idx = engine
                .find_workspace_index_for_pane(pid)
                .ok_or(ApplyError::PaneNotFound(pid))?;
            return Ok((ws_idx, pid));
        }
        let ws_idx = self.active_workspace.min(engine.workspaces.len() - 1);
        let ws = &engine.workspaces[ws_idx];
        let pid = ws.focused_pane;
        if ws.pane_layout().find_pane(pid).is_some() {
            return Ok((ws_idx, pid));
        }
        let first = ws
            .pane_layout()
            .first_pane()
            .map(|p| p.id)
            .ok_or(ApplyError::Empty)?;
        Ok((ws_idx, first))
    }

    fn build_pane_node(
        &mut self,
        engine: &mut EngineState,
        node: &PresetPaneNode,
    ) -> Result<PaneNode, ApplyError> {
        match node {
            PresetPaneNode::Leaf { pane } => {
                let p = self.build_pane(engine, pane)?;
                Ok(PaneNode::Leaf(p))
            }
            PresetPaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let f = self.build_pane_node(engine, first)?;
                let s = self.build_pane_node(engine, second)?;
                Ok(PaneNode::Split {
                    direction: SplitDirection::from(*direction),
                    ratio: ratio.clamp(0.05, 0.95),
                    first: Box::new(f),
                    second: Box::new(s),
                })
            }
        }
    }

    fn build_pane(
        &mut self,
        engine: &mut EngineState,
        preset: &PresetPane,
    ) -> Result<Pane, ApplyError> {
        if preset.tabs.is_empty() {
            return Err(ApplyError::Empty);
        }
        let pane_id = engine.next_ids.next_pane();
        let mut tabs = Vec::with_capacity(preset.tabs.len());
        for preset_tab in &preset.tabs {
            tabs.push(self.build_tab(engine, preset_tab)?);
        }
        let active_tab = preset.active_tab.min(tabs.len() - 1);
        Ok(Pane {
            id: pane_id,
            tabs,
            active_tab,
            tab_scroll_offset: 0.0,
        })
    }

    fn build_tab(
        &mut self,
        engine: &mut EngineState,
        preset: &PresetTab,
    ) -> Result<Tab, ApplyError> {
        let tab_id = engine.next_ids.next_tab();
        let layout = self.build_surface_layout(engine, &preset.layout)?;
        let focused_surface = layout.first_surface_id().ok_or(ApplyError::Empty)?;

        let auto_name = preset_default_tab_name(&preset.layout);
        let name = preset
            .explicit_name
            .clone()
            .unwrap_or_else(|| auto_name.clone());

        match layout {
            SurfaceLayout::Leaf(surface) => {
                let mut tab = Tab::new_with_surface(tab_id, name, surface);
                tab.explicit_name = preset.explicit_name.clone();
                Ok(tab)
            }
            split @ SurfaceLayout::Split { .. } => Ok(Tab {
                id: tab_id,
                name,
                explicit_name: preset.explicit_name.clone(),
                layout_opt: Some(split),
                focused_surface,
                cached_display_name: None,
            }),
        }
    }

    fn build_surface_layout(
        &mut self,
        engine: &mut EngineState,
        preset: &PresetSurfaceLayout,
    ) -> Result<SurfaceLayout, ApplyError> {
        match preset {
            PresetSurfaceLayout::Leaf { surface } => {
                let s = self.build_leaf_surface(engine, surface)?;
                Ok(SurfaceLayout::Leaf(s))
            }
            PresetSurfaceLayout::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let f = self.build_surface_layout(engine, first)?;
                let s = self.build_surface_layout(engine, second)?;
                Ok(SurfaceLayout::Split {
                    direction: SplitDirection::from(*direction),
                    ratio: ratio.clamp(0.05, 0.95),
                    first: Box::new(f),
                    second: Box::new(s),
                    focus_second: false,
                })
            }
        }
    }

    fn build_leaf_surface(
        &mut self,
        engine: &mut EngineState,
        preset: &PresetSurface,
    ) -> Result<Box<dyn Surface>, ApplyError> {
        let surface_id = engine.next_ids.next_surface();
        if preset.kind == "terminal" {
            let terminal = self.build_terminal(engine, surface_id, preset)?;
            engine.send_fast_init(surface_id);
            return Ok(Box::new(TerminalSurface {
                id: surface_id,
                terminal,
                deferred_spawn: None,
                scrollback_persist_id: None,
            }));
        }

        if !engine.surface_registry.contains(&preset.kind) {
            return Err(ApplyError::UnknownKind(preset.kind.clone()));
        }
        self.create_surface_via_registry(engine, &preset.kind, surface_id, &preset.params)
            .map_err(ApplyError::Other)
    }

    fn build_terminal(
        &self,
        engine: &EngineState,
        surface_id: u32,
        preset: &PresetSurface,
    ) -> Result<tasty_terminal::Terminal, ApplyError> {
        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let shell_string = engine.settings.general.shell.clone();
        let shell = if shell_string.is_empty() {
            None
        } else {
            Some(shell_string)
        };
        let shell_args_owned = engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let waker = engine.make_waker(surface_id);

        // restore.rs:163-184 와 동일 — cwd 와 startup_command 를 합쳐 PTY 첫 입력으로 주입.
        let mut initial = String::new();
        if let Some(dir) = preset.cwd.as_deref() {
            initial.push_str(&format!("cd {}\r", shell_escape(dir)));
        }
        if let Some(cmd) = preset.startup_command.as_deref() {
            let trimmed = cmd.trim();
            if !trimmed.is_empty() {
                initial.push_str(trimmed);
                initial.push('\r');
            }
        }
        let initial_input = if initial.is_empty() {
            None
        } else {
            Some(initial.as_str())
        };

        tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols,
                rows,
                shell: shell.as_deref(),
                args: &shell_args,
                surface_id,
                working_dir: None,
                initial_input,
            },
            waker,
        )
        .map_err(|e| ApplyError::TerminalSpawn(e.to_string()))
    }
}

/// preset leaf 의 kind 로부터 자동 탭 이름 도출.
/// `state/pane.rs::default_tab_name_for_kind` 와 동일 정책이지만 PresetSurfaceLayout
/// 트리에서 첫 leaf 를 직접 찾는다.
fn preset_default_tab_name(layout: &PresetSurfaceLayout) -> String {
    let first = first_preset_leaf(layout);
    super::pane::default_tab_name_for_kind(&first.kind, &first.params)
}

fn first_preset_leaf(layout: &PresetSurfaceLayout) -> &PresetSurface {
    match layout {
        PresetSurfaceLayout::Leaf { surface } => surface,
        PresetSurfaceLayout::Split { first, .. } => first_preset_leaf(first),
    }
}

/// shell 안전 escape (restore.rs:282 와 동일).
fn shell_escape(s: &str) -> String {
    if s.contains(' ') || s.contains('\'') || s.contains('"') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}
