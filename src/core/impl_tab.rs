//! 탭 생성·이동·제목 변경을 처리한다.

use super::*;
use crate::core::engine_access::EngineMut;

impl Core {
    /// ID로 대상 탭을 찾는다. 사용자가 명시한 이름은 유지하고 선택된 surface의 제목만 반영한다.
    #[cfg(any(feature = "gui", test))]
    pub(super) fn apply_update_tab_name(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        name: String,
    ) -> CoreEvent {
        let trimmed = name.trim();
        if trimmed.is_empty() {
            return CoreEvent::TabNameUpdated {
                skipped_explicit: false,
            };
        }
        for ws in &mut engine.workspaces {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    for tab in &mut pane.tabs {
                        if tab.all_surface_ids().contains(&surface_id) {
                            let skipped_explicit = tab.explicit_name.is_some();
                            tab.surface_titles.entry(surface_id).or_default().osc_title =
                                Some(name);
                            return CoreEvent::TabNameUpdated { skipped_explicit };
                        }
                    }
                }
            }
        }
        CoreEvent::TabNameUpdated {
            skipped_explicit: false,
        }
    }

    /// 비터미널은 activate에 따라 선택하고 terminal은 항상 배경 탭으로 만든다.
    pub(super) fn apply_create_tab(
        engine: &mut EngineMut<'_>,
        pane_id: u32,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        explicit_name: Option<String>,
        surface_params: serde_json::Value,
        activate: bool,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let is_terminal = kind == "terminal";

        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(surface_id);

        let prepared_non_terminal = if !is_terminal {
            let surface = engine.create_surface_via_registry(
                &kind,
                surface_id,
                cwd.as_deref(),
                &surface_params,
            )?;
            let name = crate::core::surface_registry::default_tab_name_for_kind(
                &kind,
                &surface_params,
                engine.surface_registry.get(&kind).as_deref(),
            );
            Some((surface, name))
        } else {
            None
        };

        // pane을 가변 참조하기 전에 Terminal을 store에 넣는다. 이후 pane 조회 실패가 이를 되돌리지는 않는다.
        let prepared_terminal = if is_terminal {
            let spawn = crate::core::terminal_spawn::ShellSpawnOpts {
                cols,
                rows,
                shell: sh.shell_ref(),
                shell_args: &sh.args_ref(),
                extra_env: &sh.envs_ref(),
                waker,
                working_dir: cwd.as_deref(),
            };
            let terminal = crate::core::terminal_spawn::spawn_shell_terminal(surface_id, spawn)?;
            engine.runtime.terminals.insert(surface_id, terminal);
            true
        } else {
            false
        };

        {
            let pane = engine
                .find_pane_by_id_mut(pane_id)
                .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found"))?;
            if is_terminal {
                debug_assert!(prepared_terminal);
                pane.add_terminal_marker_tab_background(tab_id, surface_id, explicit_name);
            } else {
                let (surface, name) = prepared_non_terminal.unwrap();
                if activate {
                    pane.add_surface_tab(tab_id, name, explicit_name, surface);
                } else {
                    pane.add_surface_tab_background(tab_id, name, explicit_name, surface);
                }
            }
        }

        if activate && !is_terminal {
            engine
                .attach
                .presentation
                .selected_tabs
                .insert(pane_id, tab_id);
        }
        if is_terminal {
            engine.send_fast_init(surface_id);
        }
        engine.mark_layout_dirty();

        if let Some(ws_idx) = engine.find_workspace_index_for_pane(pane_id) {
            let ws_id = engine.workspaces[ws_idx].id;
            engine.tap_new_workspace_member(ws_id, surface_id, is_terminal);
        }

        let tab_count = engine
            .find_pane_by_id(pane_id)
            .map(|p| p.tabs.len())
            .unwrap_or(0);

        Ok(vec![CoreEvent::TabCreated {
            pane_id,
            tab_id,
            surface_id,
            tab_count,
            activate: activate && !is_terminal,
        }])
    }

    /// 없는 탭이면 오류다. 이름을 지우면 선택된 surface의 OSC 제목을 다시 반영한다.
    pub(super) fn apply_rename_tab(
        engine: &mut crate::core::CoreState,
        tab_id: u32,
        name: Option<String>,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let tab = engine
            .find_pane_for_tab(tab_id)
            .and_then(|pane_id| engine.find_pane_by_id_mut(pane_id))
            .and_then(|pane| pane.tabs.iter_mut().find(|t| t.id == tab_id))
            .ok_or_else(|| anyhow::anyhow!("Tab id {tab_id} not found"))?;
        tab.explicit_name = name;
        engine.mark_layout_dirty();
        Ok(Vec::new())
    }

    pub(super) fn apply_move_tab(
        engine: &mut crate::core::CoreState,
        pane_id: u32,
        tab_id: u32,
        to_index: usize,
    ) -> CoreEvent {
        let moved = engine.find_pane_by_id_mut(pane_id).is_some_and(|pane| {
            let Some(from_index) = pane.tabs.iter().position(|tab| tab.id == tab_id) else {
                return false;
            };
            pane.move_tab(from_index, to_index)
        });
        if moved {
            engine.mark_layout_dirty();
        }
        CoreEvent::TabMoved { moved }
    }
}

#[cfg(test)]
mod create_tab_selection_tests {
    use super::*;

    fn create(engine: &mut EngineMut<'_>, pane_id: u32, kind: &str, activate: bool) -> usize {
        let events = Core::apply_create_tab(
            engine,
            pane_id,
            None,
            kind.to_string(),
            None,
            serde_json::json!({}),
            activate,
        )
        .expect("create tab");
        let Some(CoreEvent::TabCreated { activate, .. }) = events.into_iter().next() else {
            panic!("expected TabCreated");
        };
        usize::from(activate)
    }

    fn engine_and_pane() -> (crate::runtime::engine_session::EngineSession, u32) {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine");
        let engine = engine_session.borrow_mut();
        let sid = engine.workspaces[0].all_surface_ids()[0];
        let pane_id = engine.find_pane_for_surface(sid).expect("pane");
        (engine_session, pane_id)
    }

    #[test]
    fn background_creation_emits_no_activation() {
        let (mut engine_session, pane_id) = engine_and_pane();
        let mut engine = engine_session.borrow_mut();
        assert_eq!(create(&mut engine, pane_id, "empty", false), 0);
        let pane = engine.find_pane_by_id(pane_id).unwrap();
        assert_eq!(pane.tabs.len(), 2);
    }

    #[test]
    fn activated_creation_returns_a_user_continuation() {
        let (mut engine_session, pane_id) = engine_and_pane();
        let mut engine = engine_session.borrow_mut();
        assert_eq!(create(&mut engine, pane_id, "empty", true), 1);
        assert_eq!(engine.find_pane_by_id(pane_id).unwrap().tabs.len(), 2);
    }
}

#[cfg(test)]
mod tab_title_tests {
    use super::*;
    use crate::model::SplitDirection;
    use tasty_terminal::Terminal;

    fn test_engine() -> crate::runtime::engine_session::EngineSession {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine")
    }

    fn split_tab_engine() -> (crate::runtime::engine_session::EngineSession, u32, u32, u32) {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine
            .runtime
            .terminals
            .insert(a, Terminal::new_detached(80, 24));
        let b = 7777;
        let (ws_idx, pane_id) = engine.find_workspace_index_for_surface(a).unwrap();
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .split_surface_by_id_marker(a, SplitDirection::Horizontal, b)
            .unwrap();
        engine
            .runtime
            .terminals
            .insert(b, Terminal::new_detached(80, 24));
        (engine_session, pane_id, a, b)
    }

    fn set_title(engine: &mut EngineMut<'_>, sid: u32, title: &str) {
        engine
            .runtime
            .terminals
            .get_mut(sid)
            .unwrap()
            .feed_bytes(format!("\x1b]2;{title}\x07").as_bytes());
    }

    fn display_name(engine: &CoreState, pane_id: u32, selected: u32) -> String {
        engine.workspaces[0]
            .pane_layout()
            .find_pane(pane_id)
            .unwrap()
            .tabs[0]
            .display_name(Some(selected))
    }

    #[test]
    fn non_focused_surface_title_does_not_change_tab_name() {
        let (mut engine_session, pane_id, a, b) = split_tab_engine();
        let mut engine = engine_session.borrow_mut();
        let ev = Core::apply_update_tab_name(&mut engine, b, "TITLE-FROM-B".to_string());
        assert!(matches!(
            ev,
            CoreEvent::TabNameUpdated {
                skipped_explicit: false,
                ..
            }
        ));
        assert_ne!(display_name(&engine, pane_id, a), "TITLE-FROM-B");

        Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert_eq!(display_name(&engine, pane_id, a), "TITLE-A");
    }

    #[test]
    fn explicit_name_survives_focused_title() {
        let (mut engine_session, pane_id, a, _b) = split_tab_engine();
        let mut engine = engine_session.borrow_mut();
        engine.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane_id)
            .unwrap()
            .tabs[0]
            .explicit_name = Some("FIXED".to_string());
        let ev = Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert!(matches!(
            ev,
            CoreEvent::TabNameUpdated {
                skipped_explicit: true,
                ..
            }
        ));
        assert_eq!(display_name(&engine, pane_id, a), "FIXED");
    }

    #[test]
    fn refresh_projects_new_focused_surface_title() {
        let (mut engine_session, pane_id, a, b) = split_tab_engine();
        let mut engine = engine_session.borrow_mut();
        set_title(&mut engine, a, "TITLE-A");
        set_title(&mut engine, b, "TITLE-B");
        Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert_eq!(display_name(&engine, pane_id, a), "TITLE-A");

        engine.refresh_tab_osc_title(b);
        assert_eq!(display_name(&engine, pane_id, b), "TITLE-B");
    }

    #[test]
    fn refresh_clears_when_focused_has_no_title() {
        let (mut engine_session, pane_id, a, b) = split_tab_engine();
        let mut engine = engine_session.borrow_mut();
        set_title(&mut engine, a, "TITLE-A");
        Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert_eq!(display_name(&engine, pane_id, a), "TITLE-A");

        engine.refresh_tab_osc_title(b);
        assert_ne!(display_name(&engine, pane_id, b), "TITLE-A");
    }

    #[test]
    fn closing_focused_surface_reprojects_to_survivor() {
        let (mut engine_session, pane_id, a, b) = split_tab_engine();
        let mut engine = engine_session.borrow_mut();
        set_title(&mut engine, a, "TITLE-A");
        set_title(&mut engine, b, "TITLE-B");
        Core::apply_update_tab_name(&mut engine, a, "TITLE-A".to_string());
        assert_eq!(display_name(&engine, pane_id, a), "TITLE-A");

        let ev = Core::apply_close_surface(&mut engine, a, None);
        assert!(matches!(ev, CoreEvent::SurfaceClosed { closed: true, .. }));
        assert_eq!(display_name(&engine, pane_id, b), "TITLE-B");
    }

    #[test]
    fn moving_surface_reprojects_target_tab_title() {
        let (mut engine_session, pane_id, a, b) = split_tab_engine();
        let mut engine = engine_session.borrow_mut();
        set_title(&mut engine, a, "TITLE-A");
        set_title(&mut engine, b, "TITLE-B");
        engine.refresh_tab_osc_title(b);
        assert_eq!(display_name(&engine, pane_id, b), "TITLE-B");

        engine.pending_move = Some(crate::core::state::PendingMove::Surface(a));
        let ev = Core::apply_move_surface(&mut engine, a, b);
        assert!(matches!(
            ev,
            CoreEvent::MoveSurfaceApplied { moved: true, .. }
        ));
        assert_eq!(display_name(&engine, pane_id, a), "TITLE-A");
    }
}
