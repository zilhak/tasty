//! 닫힌 항목의 surface·pane을 새 ID로 다시 만든다. 필요한 PTY도 생성한다.
//! 결과를 트리에 붙이는 일은 호출자가 맡으며 부분 생성 뒤 실패를 되돌리지는 않는다.

use crate::core::CoreState;
use crate::model::closed_item::*;
use crate::model::{
    DeferredPlugin, EmptySurface, Pane, PaneNode, Surface, SurfaceLayout, Tab, TerminalSurface,
};

pub(crate) enum RebuildResult {
    Single(Box<dyn Surface>),
    Layout(SurfaceLayout, u32),
}

impl RebuildResult {
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
            // 미등록 kind는 원래 정보의 placeholder로 남긴다. None을 반환하면 ? 전파로 형제 tab·pane까지 버릴 수 있다.
            match engine.surface_registry.get_live(&kind) {
                None => {
                    let ph =
                        EmptySurface::new_deferred_plugin(id, DeferredPlugin { kind, snapshot });
                    Some(RebuildResult::Single(Box::new(ph)))
                }
                Some(def) => match (def.restore)(id, &snapshot) {
                    Ok(surface) => Some(RebuildResult::Single(surface)),
                    Err(e) => {
                        tracing::warn!("restore failed for kind '{}': {e}", kind);
                        None
                    }
                },
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

    // cd와 복원 명령을 초기 입력으로 넘긴다. 자식의 첫 read나 명령 실행 성공을 보장하지는 않는다.
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

    // 저장된 scrollback은 읽기에 성공한 뒤에만 삭제한다. 읽지 못한 원본은 이 경로에서 남겨 둔다.
    let scrollback_lines: Vec<tasty_terminal::ScrollbackLine> = match closed.scrollback {
        ClosedScrollback::Persisted(id) => match crate::scrollback_store::read(&id) {
            crate::scrollback_store::ScrollbackRead::Loaded(lines) => {
                crate::scrollback_store::delete(&id);
                lines
            }
            crate::scrollback_store::ScrollbackRead::Absent => Vec::new(),
            crate::scrollback_store::ScrollbackRead::Unreadable => {
                tracing::warn!(
                    "restore: scrollback {id} could not be read; continuing without it and leaving the file in place"
                );
                Vec::new()
            }
        },
        ClosedScrollback::Inline(lines) => lines.into_iter().collect(),
        ClosedScrollback::Empty => Vec::new(),
    };
    if !scrollback_lines.is_empty() {
        terminal.inject_scrollback(scrollback_lines);
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

/// 공백·따옴표가 있는 경로를 작은따옴표로 감싼다. 그 밖의 셸 특수문자를 모두 처리하는 함수는 아니다.
fn shell_escape(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    if s.contains(' ') || s.contains('\'') || s.contains('"') {
        format!("'{}'", s.replace('\'', "'\\''"))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod deferred_plugin_tests {
    use super::*;
    use crate::model::closed_item::{ClosedPane, ClosedPaneNode, ClosedPanel, ClosedTab};

    // plugin kind가 등록되지 않은 상태에서 placeholder와 형제 보존을 확인한다.
    // 실제 부팅 deadline이나 plugin 준비 대기 전체를 실행하는 검사는 아니다.

    fn engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    fn generic_tab(id: u32, kind: &str) -> ClosedTab {
        ClosedTab {
            id,
            name: kind.to_string(),
            explicit_name: None,
            panel: ClosedPanel::Generic {
                kind: kind.to_string(),
                snapshot: serde_json::json!({ "k": kind }),
            },
        }
    }

    #[test]
    fn missing_plugin_kind_rebuilds_as_deferred_placeholder() {
        let mut e = engine();
        let panel = ClosedPanel::Generic {
            kind: "no_such_plugin".to_string(),
            snapshot: serde_json::json!({ "a": 1 }),
        };
        let r = rebuild_surface(&mut e, panel).expect("miss must yield a placeholder, not None");
        match r {
            RebuildResult::Single(s) => {
                let es = s
                    .as_any()
                    .downcast_ref::<EmptySurface>()
                    .expect("placeholder is an EmptySurface");
                let p = es.deferred_plugin().expect("carries deferred plugin info");
                assert_eq!(p.kind, "no_such_plugin");
                assert_eq!(p.snapshot, serde_json::json!({ "a": 1 }));
            }
            _ => panic!("expected a single placeholder surface"),
        }
    }

    // 실제 PTY 생성 없이 형제 보존을 확인하려고 두 탭 모두 Generic으로 만든다.
    #[test]
    fn missing_plugin_kind_preserves_sibling_tabs() {
        let mut e = engine();
        let pane = ClosedPane {
            id: 0,
            tabs: vec![
                generic_tab(1, "no_such_plugin"),
                generic_tab(2, "also_missing"),
            ],
            active_tab: 0,
        };
        let rebuilt = rebuild_pane(&mut e, pane).expect("pane must survive a missing plugin kind");
        assert_eq!(
            rebuilt.tabs.len(),
            2,
            "미등록 plugin 종류가 있어도 형제 tab을 유지해야 한다"
        );
    }

    #[test]
    fn deadline_zero_apply_preserves_sibling_panes() {
        let mut e = engine();
        let node = ClosedPaneNode::Split {
            direction: crate::model::SplitDirection::Horizontal,
            ratio: 0.5,
            first: Box::new(ClosedPaneNode::Leaf(ClosedPane {
                id: 0,
                tabs: vec![generic_tab(1, "no_such_plugin")],
                active_tab: 0,
            })),
            second: Box::new(ClosedPaneNode::Leaf(ClosedPane {
                id: 0,
                tabs: vec![generic_tab(2, "sibling_pane")],
                active_tab: 0,
            })),
        };
        let rebuilt =
            rebuild_pane_node(&mut e, node).expect("split must survive a missing kind in one leaf");
        match rebuilt {
            PaneNode::Split { first, second, .. } => {
                assert!(
                    matches!(*first, PaneNode::Leaf(_)),
                    "miss leaf 가 placeholder pane 으로 남아야 한다"
                );
                assert!(
                    matches!(*second, PaneNode::Leaf(_)),
                    "미등록 plugin 종류가 있어도 형제 pane을 유지해야 한다"
                );
            }
            _ => panic!("expected a split node"),
        }
    }

    // 등록된 종류는 placeholder가 아닌 생성기 결과를 사용하는지 확인할 대조군이다.
    fn register_ok_kind(e: &mut CoreState, kind: &'static str) {
        use crate::core::surface_registry::{KindSource, RegisteredRendering, SurfaceKindDef};
        use std::collections::HashMap;
        use std::sync::Arc;
        e.surface_registry.register(SurfaceKindDef {
            kind,
            rendering: RegisteredRendering::HostEgui,
            source: KindSource::HostBuiltin,
            display_name_i18n_key: "test.dummy",
            icon: None,
            create: Arc::new(|_, _, _| Err(anyhow::anyhow!("dummy"))),
            restore: Arc::new(|id, _| Ok(Box::new(EmptySurface::new(id)) as Box<dyn Surface>)),
            snapshot: Arc::new(|_| None),
            preset_fields: Vec::new(),
            param_aliases: HashMap::new(),
            default_params: HashMap::new(),
            consumes_egui_input: false,
            zoomable: false,
            egui_copy: false,
            copy_path: false,
            egui_paste: false,
            name_from_param: None,
            records_recent: false,
            convert_requires_input: false,
            convert_input_popup: None,
        });
    }

    #[test]
    fn registered_kind_restores_real_surface_not_placeholder() {
        let mut e = engine();
        register_ok_kind(&mut e, "present_plugin");
        let panel = ClosedPanel::Generic {
            kind: "present_plugin".to_string(),
            snapshot: serde_json::json!({ "a": 1 }),
        };
        let r = rebuild_surface(&mut e, panel).expect("registered kind restores");
        match r {
            RebuildResult::Single(s) => {
                let es = s
                    .as_any()
                    .downcast_ref::<EmptySurface>()
                    .expect("our test restore returns an EmptySurface");
                assert!(
                    es.deferred_plugin().is_none(),
                    "등록된 kind 는 deferred placeholder 가 아니라 실제 복원이어야 한다"
                );
            }
            _ => panic!("expected a single surface"),
        }
    }
}
