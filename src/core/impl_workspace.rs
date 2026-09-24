//! workspace 생성·이동·메타데이터와 레이아웃 저장·닫힌 항목 복원을 처리한다.

use super::*;
use crate::core::pty_registry::PTY_ID_BASE;

impl Core {
    /// scope에 맞는 항목을 꺼내 engine에 복원한다. App의 활성 workspace 변경은 호출자가 맡는다.
    /// 복원 중 실패해도 꺼낸 항목이나 이미 만든 자원을 되돌리는 처리는 여기서 하지 않는다.
    pub(super) fn apply_restore_closed_item(
        engine: &mut crate::core::CoreState,
        target_pane_id: Option<u32>,
        scope: crate::core::intent::RestoreScope,
    ) -> CoreEvent {
        use crate::core::intent::RestoredKind;
        use crate::core::restore_rebuild;
        use crate::model::Surface;
        use crate::model::Tab;
        use crate::model::Workspace;
        use crate::model::closed_item::ClosedItem;

        let nothing = || CoreEvent::ClosedItemRestored {
            restored: false,
            kind: RestoredKind::Nothing,
        };

        let Some(item) = pop_for_scope(engine, scope) else {
            return nothing();
        };

        let kind = match item {
            ClosedItem::Surface { surface, tab_name } => {
                let Some(node) = restore_rebuild::rebuild_surface_node(engine, surface) else {
                    return nothing();
                };
                let Some(pane_id) = target_pane_id else {
                    return nothing();
                };
                let tab_id = engine.next_ids.next_tab();
                let surface_box: Box<dyn Surface> = Box::new(node);
                let tab = Tab::new_with_surface(tab_id, tab_name, surface_box);
                if !push_tab_to_pane(engine, pane_id, tab) {
                    return nothing();
                }
                RestoredKind::TabIntoPane
            }
            ClosedItem::Tab(closed_tab) => {
                let Some(result) = restore_rebuild::rebuild_surface(engine, closed_tab.panel)
                else {
                    return nothing();
                };
                let Some(pane_id) = target_pane_id else {
                    return nothing();
                };
                let tab_id = engine.next_ids.next_tab();
                let name = closed_tab.explicit_name.unwrap_or(closed_tab.name);
                let tab = result.into_tab(tab_id, name);
                if !push_tab_to_pane(engine, pane_id, tab) {
                    return nothing();
                }
                RestoredKind::TabIntoPane
            }
            ClosedItem::Pane {
                pane,
                sibling_pane_id,
                direction,
                ratio,
                was_first,
            } => {
                let Some(rebuilt) = restore_rebuild::rebuild_pane(engine, pane) else {
                    return nothing();
                };
                // 닫힐 당시 위치 대신 호출자가 지정한 대상 pane의 workspace에 복원한다.
                let Some(pane_id) = target_pane_id else {
                    return nothing();
                };
                let Some(ws) = engine
                    .workspaces
                    .iter_mut()
                    .find(|ws| ws.pane_layout().find_pane(pane_id).is_some())
                else {
                    return nothing();
                };
                let restored_pane_id = rebuilt.id;
                let leftover = ws.pane_layout_mut().insert_pane_beside(
                    sibling_pane_id,
                    direction,
                    ratio,
                    rebuilt,
                    was_first,
                );
                if let Some(rebuilt) = leftover {
                    // 옛 sibling이 없으면 대상 pane을 분할해 넣는다.
                    if ws
                        .pane_layout_mut()
                        .split_pane_in_place(pane_id, direction, rebuilt)
                        .is_some()
                    {
                        tracing::warn!(
                            "restore pane: fallback split_pane_in_place unexpectedly missed pane {pane_id}"
                        );
                        return nothing();
                    }
                }
                RestoredKind::PaneIntoWorkspace {
                    pane_id: restored_pane_id,
                }
            }
            ClosedItem::Workspace {
                name,
                subtitle,
                pane_layout,
                focused_pane,
                ..
            } => {
                let ws_id = engine.next_ids.next_workspace();
                let Some(pane_node) = restore_rebuild::rebuild_pane_node(engine, pane_layout)
                else {
                    return nothing();
                };
                let all_pane_ids = pane_node.all_pane_ids();
                let actual_focused = if all_pane_ids.contains(&focused_pane) {
                    focused_pane
                } else {
                    *all_pane_ids.first().unwrap_or(&0)
                };
                let ws = Workspace::from_restored(ws_id, name, subtitle, pane_node, actual_focused);
                engine.workspaces.push(ws);
                RestoredKind::Workspace {
                    new_ws_index: engine.workspaces.len() - 1,
                }
            }
        };

        engine.mark_layout_dirty();
        CoreEvent::ClosedItemRestored {
            restored: true,
            kind,
        }
    }

    pub(super) fn apply_respawn_terminal(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        cwd: Option<std::path::PathBuf>,
    ) -> CoreEvent {
        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(surface_id);
        let new_terminal = match tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols,
                rows,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
                extra_env: &sh.envs_ref(),
                surface_id,
                working_dir: cwd.as_deref(),
                initial_input: None,
            },
            waker,
        ) {
            Ok(t) => t,
            Err(e) => {
                return CoreEvent::TerminalRespawned {
                    surface_id,
                    error: Some(e.to_string()),
                };
            }
        };
        match engine.replace_terminal_by_id(surface_id, new_terminal) {
            Ok(()) => CoreEvent::TerminalRespawned {
                surface_id,
                error: None,
            },
            Err(e) => CoreEvent::TerminalRespawned {
                surface_id,
                error: Some(e.to_string()),
            },
        }
    }

    /// 벡터의 위치를 바꾸며 App의 활성 workspace 인덱스는 호출자가 보정한다.
    pub(super) fn apply_move_workspace(
        &mut self,
        engine: &mut crate::core::CoreState,
        from_index: usize,
        to_index: usize,
    ) -> CoreEvent {
        let len = engine.workspaces.len();
        if from_index == to_index || from_index >= len || to_index >= len {
            return CoreEvent::WorkspaceMoved {
                from_index,
                to_index,
                moved: false,
            };
        }
        let ws = engine.workspaces.remove(from_index);
        engine.workspaces.insert(to_index, ws);
        engine.mark_layout_dirty();
        CoreEvent::WorkspaceMoved {
            from_index,
            to_index,
            moved: true,
        }
    }

    pub(super) fn apply_update_workspace_meta(
        &mut self,
        engine: &mut crate::core::CoreState,
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let Some(index) = engine
            .workspaces
            .iter()
            .position(|ws| ws.id == workspace_id)
        else {
            anyhow::bail!("Workspace id {} not found", workspace_id);
        };

        let ws = &mut engine.workspaces[index];
        if let Some(ref n) = name {
            ws.name = n.clone();
        }
        if let Some(ref s) = subtitle {
            ws.subtitle = s.clone();
        }
        if let Some(ref d) = description {
            ws.description = d.clone();
        }
        engine.mark_layout_dirty();

        Ok(vec![CoreEvent::WorkspaceMetaUpdated {
            workspace_id,
            index,
            name,
            subtitle,
            description,
        }])
    }

    pub(super) fn apply_create_workspace(
        &mut self,
        engine: &mut crate::core::CoreState,
        params: WorkspaceCreationParams,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        Ok(vec![apply_create_workspace_inner(engine, params)?])
    }

    /// 내부 초기화·빈 창 보충용 terminal workspace를 만들고 인덱스를 반환한다.
    /// 생성 이벤트를 dispatcher에 넘기지 않으므로 여기서는 plugin 생성 알림을 보내지 않는다.
    pub(crate) fn create_default_workspace(
        &mut self,
        engine: &mut crate::core::CoreState,
    ) -> anyhow::Result<usize> {
        let event = apply_create_workspace_inner(engine, WorkspaceCreationParams::terminal())?;
        match event {
            CoreEvent::WorkspaceCreated { index, .. } => Ok(index),
            _ => unreachable!("apply_create_workspace_inner 는 WorkspaceCreated 만 반환"),
        }
    }

    /// 설정·변경 여부·보호된 슬롯을 확인해 저장을 시도한다. debounce 대기는 호출자가 맡는다.
    /// LayoutSaved는 저장 생략 때도 반환되며 실제 쓰기 성공을 뜻하지 않는다.
    #[cfg(any(feature = "gui", test))]
    pub(super) fn apply_save_layout_now(
        engine: &mut crate::core::CoreState,
        active_workspace: usize,
        force: bool,
    ) -> CoreEvent {
        let g = &engine.settings.general;
        let should_save = if force {
            g.restore_layout && (engine.layout_dirty.is_dirty() || g.restore_surface_content)
        } else {
            g.restore_layout && engine.layout_dirty.is_dirty()
        };
        if !should_save {
            return CoreEvent::LayoutSaved;
        }
        // 읽지 못한 기존 사용자 레이아웃을 현재의 빈 상태로 덮지 않도록 저장을 막는다.
        if engine.layout_slot_protected {
            tracing::debug!("layout save skipped: slot is locked because it could not be read");
            return CoreEvent::LayoutSaved;
        }
        // engine이 가진 슬롯에만 쓴다. 슬롯이 없으면 저장하지 않는다.
        let Some(slot) = engine.layout_slot else {
            return CoreEvent::LayoutSaved;
        };
        crate::core::layout_persistence::save_slot(engine, active_workspace, slot);
        engine.layout_dirty.clear();
        CoreEvent::LayoutSaved
    }

    /// 대기 중인 저장 레이아웃을 꺼내 복원하고 활성 workspace 후보를 반환한다.
    #[cfg(feature = "gui")]
    pub(super) fn apply_apply_pending_layout_restore(
        engine: &mut crate::core::CoreState,
    ) -> CoreEvent {
        let Some(saved) = engine.pending_layout_restore.take() else {
            return CoreEvent::LayoutRestored {
                restored: false,
                active_workspace: None,
            };
        };

        // 새 ID가 이전 실행의 surface 메타데이터와 겹치지 않도록 복원 전에 발급 기준을 올린다.
        {
            let mut guard = crate::poison::recover_mutex(
                engine.memory.lock(),
                crate::core::MEMORY_WHAT,
                &crate::core::MEMORY_POISONED,
            );
            seed_surface_id_floor(&mut *guard, &engine.next_ids);
        }

        if !saved.restore(engine) {
            return CoreEvent::LayoutRestored {
                restored: false,
                active_workspace: None,
            };
        }

        // 발급 기준만 올리면 죽은 scope는 남으므로 복원된 live ID 목록으로 따로 정리한다.
        {
            let live: std::collections::HashSet<u32> = engine
                .workspaces
                .iter()
                .flat_map(|ws| ws.all_surface_ids())
                .collect();
            let mut guard = crate::poison::recover_mutex(
                engine.memory.lock(),
                crate::core::MEMORY_WHAT,
                &crate::core::MEMORY_POISONED,
            );
            let removed =
                crate::surface_meta::SurfaceMetaStore::purge_dead_surfaces(&mut *guard, &live);
            if removed > 0 {
                tracing::info!(
                    "surface_meta GC: purged {removed} dead surface scope(s) on restore"
                );
            }
        }

        let active = engine.restored_active_workspace.take();
        CoreEvent::LayoutRestored {
            restored: true,
            active_workspace: active,
        }
    }
}

/// workspace 생성 요청. 사용자 요청과 내부 기본 workspace 생성이 같은 구현을 사용한다.
pub(crate) struct WorkspaceCreationParams {
    pub(crate) cwd: Option<std::path::PathBuf>,
    pub(crate) kind: String,
    pub(crate) surface_params: serde_json::Value,
    pub(crate) name: Option<String>,
    pub(crate) subtitle: Option<String>,
    pub(crate) description: Option<String>,
    pub(crate) category: Option<crate::model::WorkspaceCategoryId>,
}

impl WorkspaceCreationParams {
    pub(crate) fn terminal() -> Self {
        Self {
            cwd: None,
            kind: "terminal".to_string(),
            surface_params: serde_json::Value::Null,
            name: None,
            subtitle: None,
            description: None,
            category: None,
        }
    }
}

pub(crate) fn apply_create_workspace_inner(
    engine: &mut crate::core::CoreState,
    params: WorkspaceCreationParams,
) -> anyhow::Result<CoreEvent> {
    let WorkspaceCreationParams {
        cwd,
        kind,
        surface_params,
        name,
        subtitle,
        description,
        category,
    } = params;
    if kind == "empty" {
        anyhow::bail!("Cannot create workspace with empty surface kind");
    }

    let ws_id = engine.next_ids.next_workspace();
    let pane_id = engine.next_ids.next_pane();
    let tab_id = engine.next_ids.next_tab();
    let surface_id = engine.next_ids.next_surface();
    let auto_name = name
        .clone()
        .unwrap_or_else(|| format!("Workspace {}", engine.workspaces.len() + 1));
    let is_terminal = kind == "terminal";

    let mut ws = if is_terminal {
        let shell = if engine.settings.general.shell.is_empty() {
            None
        } else {
            Some(engine.settings.general.shell.as_str())
        };
        let shell_args_owned = engine.settings.general.effective_shell_args();
        let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
        let shell_envs_owned = engine.settings.general.effective_shell_envs();
        let shell_envs: Vec<(&str, &str)> = shell_envs_owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        let terminal = crate::model::Pane::spawn_terminal(
            surface_id,
            crate::model::ShellSpawnOpts {
                cols: engine.default_cols,
                rows: engine.default_rows,
                shell,
                shell_args: &shell_args,
                extra_env: &shell_envs,
                waker: engine.make_waker(surface_id),
                working_dir: cwd.as_deref(),
            },
        )?;
        engine.terminals.insert(surface_id, terminal);
        crate::model::Workspace::new_with_terminal_marker(
            ws_id, auto_name, pane_id, tab_id, surface_id,
        )
    } else {
        let surface = engine.create_surface_via_registry(
            &kind,
            surface_id,
            cwd.as_deref(),
            &surface_params,
        )?;
        let tab_name = crate::core::surface_registry::default_tab_name_for_kind(
            &kind,
            &surface_params,
            engine.surface_registry.get(&kind).as_deref(),
        );
        let pane = crate::model::Pane::new_with_surface(pane_id, tab_id, tab_name, surface);
        crate::model::Workspace::new_with_pane(ws_id, auto_name, pane)
    };

    if let Some(cat_id) = category
        && engine.category_index(cat_id).is_some()
    {
        ws.set_category(cat_id);
    }

    engine.workspaces.push(ws);
    let idx = engine.workspaces.len() - 1;

    let renamed_name = name;
    let renamed_subtitle = subtitle.inspect(|s| {
        engine.workspaces[idx].subtitle = s.clone();
    });
    let renamed_description = description.inspect(|d| {
        engine.workspaces[idx].description = d.clone();
    });

    if is_terminal {
        engine.send_fast_init(surface_id);
    }
    engine.mark_layout_dirty();

    let final_surface_id = {
        let ws = &engine.workspaces[idx];
        let pane_id = ws.focused_pane;
        ws.pane_layout()
            .find_pane(pane_id)
            .and_then(|pane| pane.tabs.get(pane.active_tab))
            .and_then(|tab| tab.focused_surface_id())
    };

    Ok(CoreEvent::WorkspaceCreated {
        id: ws_id,
        index: idx,
        surface_id: final_surface_id,
        renamed_name,
        renamed_subtitle,
        renamed_description,
    })
}

/// scope에 맞는 최신 복원 항목만 꺼낸다. workspace 전체 항목은 출처가 None이므로 Workspace 필터에서 제외된다.
/// 꺼낸 뒤 거절하면 항목을 잃으므로 후보 선택 때 범위를 적용한다.
fn pop_for_scope(
    engine: &mut crate::core::CoreState,
    scope: crate::core::intent::RestoreScope,
) -> Option<crate::model::ClosedItem> {
    use crate::core::intent::RestoreScope;
    match scope {
        RestoreScope::Workspace(ws_id) => engine.closed_items.pop_matching(|o| o == Some(ws_id)),
        RestoreScope::Local => engine.closed_items.pop_matching(|_| true),
    }
}

fn push_tab_to_pane(
    engine: &mut crate::core::CoreState,
    pane_id: u32,
    tab: crate::model::Tab,
) -> bool {
    for ws in engine.workspaces.iter_mut() {
        if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
            pane.tabs.push(tab);
            pane.active_tab = pane.tabs.len() - 1;
            return true;
        }
    }
    false
}

/// 저장 메타데이터에서 조회한 최대 surface ID를 기준으로 발급 기준을 높인다.
/// PTY 범위의 scope는 삭제를 시도하고 최대값 계산에서 제외해 잘못된 ID가 다음 실행에 이어지지 않게 한다.
/// 이미 높아진 발급 기준을 낮추거나 ID 범위 소진을 막는 함수는 아니다.
#[cfg(any(feature = "gui", test))]
pub(crate) fn seed_surface_id_floor(
    mem: &mut dyn tasty_memory::MemoryStorage,
    ids: &crate::core::state::IdGenerator,
) {
    let purged = crate::surface_meta::SurfaceMetaStore::purge_out_of_range_surfaces(mem);
    if purged > 0 {
        tracing::error!(
            "surface_meta: {purged} scope(s) had a surface id inside the PTY id space \
             (>= {PTY_ID_BASE:#x}); purged and excluded from the id floor"
        );
    }
    let mem_max = crate::surface_meta::SurfaceMetaStore::max_surface_id(mem);
    // 최대 surface ID가 PTY_ID_BASE - 1이면 다음 값은 PTY 경계에 닿는다. 여기서는 clamp하지 않는다.
    ids.bump_surface_floor(mem_max + 1);
}

#[cfg(test)]
mod surface_id_floor_tests {
    use super::seed_surface_id_floor;
    use crate::core::pty_registry::PTY_ID_BASE;
    use crate::core::state::IdGenerator;
    use crate::surface_meta::SurfaceMetaStore;
    use tasty_memory::testing::InMemoryStorage;

    #[test]
    fn floor_rises_above_stale_surface_scopes() {
        let mut mem = InMemoryStorage::new();
        SurfaceMetaStore::set(&mut mem, 17, "restore.command", "claude -r a").unwrap();
        let ids = IdGenerator::new();

        seed_surface_id_floor(&mut mem, &ids);

        assert_eq!(
            ids.next_surface(),
            18,
            "stale 최대 id(17) 위로 floor 가 올라가야 한다"
        );
    }

    #[test]
    fn pty_space_scopes_do_not_ratchet_the_floor() {
        let mut mem = InMemoryStorage::new();
        SurfaceMetaStore::set(&mut mem, PTY_ID_BASE + 499, "restore.command", "polluted").unwrap();
        SurfaceMetaStore::set(&mut mem, 3, "restore.command", "legit").unwrap();
        let ids = IdGenerator::new();

        seed_surface_id_floor(&mut mem, &ids);

        let first = ids.next_surface();
        assert!(
            crate::core::pty_registry::is_surface_id_space(first),
            "오염 scope 가 있어도 surface 카운터는 PTY 공간에 진입하지 않아야 한다 (got {first})"
        );
        assert_eq!(first, 4, "정상 범위 stale 최대 id(3) 기준으로만 floor 상승");
        assert_eq!(
            SurfaceMetaStore::get(&mut mem, PTY_ID_BASE + 499, "restore.command"),
            None,
            "오염 scope 는 그 자리에서 purge 돼야 한다"
        );
        assert_eq!(
            SurfaceMetaStore::get(&mut mem, 3, "restore.command").as_deref(),
            Some("legit"),
            "정상 범위 scope 는 보존"
        );
    }
}
