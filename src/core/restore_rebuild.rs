//! Closed item 복원의 *순수 engine* helper.
//!
//! `AppState::restore_closed_item` 의 rebuild_* helper 들이 본 모듈로 모인다.
//! 모두 `&mut CoreState` 만 받음 (AppState 의존 없음) — Core 도메인의 일부.
//!
//! 본 모듈의 함수들은 새 surface_id / tab_id / workspace_id / pane_id 를
//! `engine.next_ids` 에서 발급받고, PTY 를 spawn 하며, 새 surface tree 를
//! 구성한다. 호출 측 (state.rs 또는 Core::apply) 은 결과를 받아 적절한
//! 위치 (pane.tabs / engine.workspaces 등) 에 attach 한다.

use crate::core::CoreState;
use crate::model::closed_item::*;
use crate::model::{Pane, PaneNode, Surface, SurfaceLayout, Tab, TerminalSurface};

/// rebuild_surface 의 반환 — 단일 surface 인지 layout 인지.
pub(crate) enum RebuildResult {
    /// A single surface (Terminal, Markdown, Explorer, etc.)
    Single(Box<dyn Surface>),
    /// A full layout tree with focused_surface id
    Layout(SurfaceLayout, u32),
}

impl RebuildResult {
    /// Convert into a Tab.
    pub(crate) fn into_tab(self, tab_id: u32, name: String) -> Tab {
        match self {
            RebuildResult::Single(surface) => Tab::new_with_surface(tab_id, name, surface),
            RebuildResult::Layout(layout, focused_surface) => Tab {
                id: tab_id,
                name,
                explicit_name: None,
                osc_title: None,
                layout_opt: Some(layout),
                focused_surface,
                cached_display_name: None,
            },
        }
    }
}

pub(crate) fn rebuild_surface(
    engine: &mut CoreState,
    closed: ClosedPanel,
) -> Option<RebuildResult> {
    match closed {
        ClosedPanel::Terminal(surface) => {
            let node = rebuild_surface_node(engine, surface)?;
            Some(RebuildResult::Single(Box::new(node)))
        }
        ClosedPanel::Tab {
            layout,
            focused_surface: _,
        } => {
            let rebuilt_layout = rebuild_surface_layout(engine, layout)?;
            let first_id = rebuilt_layout.first_surface_id().unwrap_or(0);
            Some(RebuildResult::Layout(rebuilt_layout, first_id))
        }
        ClosedPanel::Generic { kind, snapshot } => {
            let id = engine.next_ids.next_surface();
            let def = engine.surface_registry.get(&kind)?;
            match (def.restore)(id, &snapshot) {
                Ok(surface) => Some(RebuildResult::Single(surface)),
                Err(e) => {
                    tracing::warn!("restore failed for kind '{}': {e}", kind);
                    None
                }
            }
        }
    }
}

pub(crate) fn rebuild_surface_node(
    engine: &mut CoreState,
    closed: ClosedSurface,
) -> Option<TerminalSurface> {
    let surface_id = engine.next_ids.next_surface();
    let cols = engine.default_cols;
    let rows = engine.default_rows;
    let shell = if engine.settings.general.shell.is_empty() {
        None
    } else {
        Some(engine.settings.general.shell.clone())
    };
    let shell_args_owned = engine.settings.general.effective_shell_args();
    let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
    let shell_envs_owned = engine.settings.general.effective_shell_envs();
    let shell_envs: Vec<(&str, &str)> = shell_envs_owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    let waker = engine.make_waker(surface_id);

    // PTY 의 첫 입력으로 cd + restore_command 를 합쳐 한 번에 주입한다. shell 이
    // stdin 을 처음 read 하는 순간 이 바이트가 들어가므로, GUI redraw / busy tick
    // 등 추가 트리거 없이 spawn 과 동시에 실행된다.
    let mut initial = String::new();
    if let Some(dir) = closed.cwd.as_deref() {
        initial.push_str(&format!("cd {}\r", shell_escape(dir)));
    }
    if let Some(cmd) = closed.restore_command.as_deref() {
        initial.push_str(&format!("{cmd}\r"));
    }
    let initial_input = if initial.is_empty() {
        None
    } else {
        Some(initial.as_str())
    };

    let mut terminal = tasty_terminal::Terminal::new(
        tasty_terminal::TerminalConfig {
            cols,
            rows,
            shell: shell.as_deref(),
            args: &shell_args,
            extra_env: &shell_envs,
            surface_id,
            working_dir: None,
            initial_input,
        },
        waker,
    )
    .ok()?;

    // Scrollback is persisted to disk at close time (see `push_closed_item`),
    // so read it back by reference here. A restore consumes the closed item, so
    // the backing file is deleted after the one-time read to avoid orphans.
    let scrollback_lines: Vec<tasty_terminal::ScrollbackLine> = match closed.scrollback {
        ClosedScrollback::Persisted(id) => {
            let lines = crate::scrollback_store::read(&id).unwrap_or_default();
            crate::scrollback_store::delete(&id);
            lines
        }
        ClosedScrollback::Inline(lines) => lines.into_iter().collect(),
        ClosedScrollback::Empty => Vec::new(),
    };
    if !scrollback_lines.is_empty() {
        terminal.inject_scrollback(scrollback_lines);
        // 새 prompt 가 화면 중간부터 시작하도록 visible 상단 절반에 옛
        // 라인을 미리 그려둔다.
        let prefill = terminal.rows() / 2;
        terminal.prefill_visible_from_scrollback(prefill);
    }

    engine.terminals.insert(surface_id, terminal);
    engine.send_fast_init(surface_id);

    Some(TerminalSurface { id: surface_id })
}

pub(crate) fn rebuild_surface_layout(
    engine: &mut CoreState,
    closed: ClosedSurfaceLayout,
) -> Option<SurfaceLayout> {
    match closed {
        ClosedSurfaceLayout::Single(surface) => {
            let node = rebuild_surface_node(engine, surface)?;
            Some(SurfaceLayout::Leaf(Box::new(node)))
        }
        ClosedSurfaceLayout::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let first = rebuild_surface_layout(engine, *first)?;
            let second = rebuild_surface_layout(engine, *second)?;
            Some(SurfaceLayout::Split {
                direction,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
                focus_second: false,
            })
        }
    }
}

pub(crate) fn rebuild_pane_node(
    engine: &mut CoreState,
    closed: ClosedPaneNode,
) -> Option<PaneNode> {
    match closed {
        ClosedPaneNode::Leaf(closed_pane) => {
            let pane = rebuild_pane(engine, closed_pane)?;
            Some(PaneNode::Leaf(pane))
        }
        ClosedPaneNode::Split {
            direction,
            ratio,
            first,
            second,
        } => {
            let first = rebuild_pane_node(engine, *first)?;
            let second = rebuild_pane_node(engine, *second)?;
            Some(PaneNode::Split {
                direction,
                ratio,
                first: Box::new(first),
                second: Box::new(second),
            })
        }
    }
}

pub(crate) fn rebuild_pane(engine: &mut CoreState, closed: ClosedPane) -> Option<Pane> {
    let pane_id = engine.next_ids.next_pane();
    let mut tabs = Vec::new();
    for closed_tab in closed.tabs {
        let result = rebuild_surface(engine, closed_tab.panel)?;
        let tab_id = engine.next_ids.next_tab();
        let name = closed_tab.explicit_name.unwrap_or(closed_tab.name);
        tabs.push(result.into_tab(tab_id, name));
    }
    if tabs.is_empty() {
        return None;
    }
    let active_tab = closed.active_tab.min(tabs.len() - 1);
    Some(Pane {
        id: pane_id,
        tabs,
        active_tab,
        tab_scroll_offset: 0.0,
    })
}

/// Escape a path for shell use.
fn shell_escape(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    if s.contains(' ') || s.contains('\'') || s.contains('"') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}
