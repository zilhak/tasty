//! `Core` — pane/surface split. `src/core/mod.rs` 의 `impl Core` 분할.

use super::*;

impl Core {
    /// `DomainIntent::SplitPane` 본문. 4-phase borrow 분리.
    pub(super) fn apply_split_pane(
        engine: &mut crate::core::CoreState,
        target_pane_id: u32,
        direction: crate::model::SplitDirection,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        surface_params: serde_json::Value,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let ws_idx = engine
            .find_workspace_index_for_pane(target_pane_id)
            .ok_or_else(|| anyhow::anyhow!("pane {} not found", target_pane_id))?;

        let new_pane_id = engine.next_ids.next_pane();
        let new_tab_id = engine.next_ids.next_tab();
        let new_surface_id = engine.next_ids.next_surface();
        let is_terminal = kind == "terminal";

        // Phase 1: engine 의 불변 의존 추출
        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(new_surface_id);

        // Phase 2: 새 pane 구성
        let new_pane = if is_terminal {
            let terminal = crate::model::Pane::spawn_terminal(
                new_surface_id,
                crate::model::ShellSpawnOpts {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    extra_env: &sh.envs_ref(),
                    waker,
                    working_dir: cwd.as_deref(),
                },
            )?;
            engine.terminals.insert(new_surface_id, terminal);
            crate::model::Pane::new_with_terminal_marker(new_pane_id, new_tab_id, new_surface_id)
        } else {
            let surface = engine.create_surface_via_registry(
                &kind,
                new_surface_id,
                cwd.as_deref(),
                &surface_params,
            )?;
            let name = crate::state::pane::default_tab_name_for_kind(
                &kind,
                &surface_params,
                engine.surface_registry.get(&kind).as_deref(),
            );
            crate::model::Pane::new_with_surface(new_pane_id, new_tab_id, name, surface)
        };

        // Phase 3: workspace pane tree mutate
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);

        // Phase 4: engine mutate (pane borrow 끝)
        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();

        // attach 점유 중인 workspace 에 새로 생긴 멤버라면 편입 + 즉시 tap
        // (로컬 생성 경로 gap — forward-op 경로와 대칭으로 점유를 상속해야 한다).
        let ws_id = engine.workspaces[ws_idx].id;
        engine.tap_new_workspace_member(ws_id, new_surface_id, is_terminal);

        Ok(vec![CoreEvent::PaneSplit {
            workspace_index: ws_idx,
            original_pane_id: target_pane_id,
            new_pane_id,
            new_surface_id,
            direction,
        }])
    }

    /// `DomainIntent::SplitSurface` 본문. tab 안에서 surface 추가 (pane tree 변경 X).
    pub(super) fn apply_split_surface(
        engine: &mut crate::core::CoreState,
        target_surface_id: u32,
        direction: crate::model::SplitDirection,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        surface_params: serde_json::Value,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let new_surface_id = engine.next_ids.next_surface();
        let is_terminal = kind == "terminal";

        // Phase 1: 새 surface 생성. terminal 은 store 에 직접 insert 후 marker leaf 만,
        // 그 외는 registry.
        let new_surface: Box<dyn crate::model::Surface> = if is_terminal {
            let cols = engine.default_cols;
            let rows = engine.default_rows;
            let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
            let waker = engine.make_waker(new_surface_id);
            let terminal = tasty_terminal::Terminal::new(
                tasty_terminal::TerminalConfig {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    args: &sh.args_ref(),
                    extra_env: &sh.envs_ref(),
                    surface_id: new_surface_id,
                    working_dir: cwd.as_deref(),
                    initial_input: None,
                },
                waker,
            )?;
            engine.terminals.insert(new_surface_id, terminal);
            Box::new(crate::model::TerminalSurface { id: new_surface_id })
        } else {
            engine.create_surface_via_registry(
                &kind,
                new_surface_id,
                cwd.as_deref(),
                &surface_params,
            )?
        };

        // Phase 2: tab 안 split
        let (ws_idx, pane_id) = engine
            .find_workspace_index_for_surface(target_surface_id)
            .ok_or_else(|| anyhow::anyhow!("surface {} not found", target_surface_id))?;
        {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws
                .pane_layout_mut()
                .find_pane_mut(pane_id)
                .ok_or_else(|| anyhow::anyhow!("pane {} not found", pane_id))?;
            pane.split_surface_by_id_with_surface(target_surface_id, direction, new_surface)?;
        }

        // Phase 3: engine mutate (pane borrow 끝)
        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();

        // attach 점유 중인 workspace 에 새로 생긴 멤버라면 편입 + 즉시 tap
        // (로컬 생성 경로 gap — forward-op 경로와 대칭으로 점유를 상속해야 한다).
        let ws_id = engine.workspaces[ws_idx].id;
        engine.tap_new_workspace_member(ws_id, new_surface_id, is_terminal);

        Ok(vec![CoreEvent::SurfaceSplit {
            workspace_index: ws_idx,
            pane_id,
            new_surface_id,
        }])
    }
}
