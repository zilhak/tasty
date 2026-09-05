//! `Core` — workspace 생성/이동/메타 + layout 저장/복원 + tab/pane 복구. `src/core/mod.rs` 의 `impl Core` 분할.

use super::*;
use crate::core::pty_registry::PTY_ID_BASE;

impl Core {
    /// `DomainIntent::RestoreClosedItem` 본문. closed_items stack pop → kind 별
    /// rebuild + engine attach. AppState 의존 부분 (active_workspace 변경) 은
    /// cascade 가 처리하므로 본 함수는 *engine mutate* 만.
    pub(super) fn apply_restore_closed_item(
        engine: &mut crate::core::CoreState,
        target_pane_id: Option<u32>,
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

        let Some(item) = engine.closed_items.pop() else {
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
                // Surface/Tab 복원과 동일 관례 — 닫힐 당시 워크스페이스가 아니라
                // 호출 시점 focused pane(=사용자가 지금 보는 워크스페이스)을
                // "현재 컨텍스트"로 삼는다.
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
                    // sibling 이 그 사이 사라졌다(추가로 더 닫혔거나, 원래
                    // 다른 워크스페이스에 있었다) — 호출 시점 focused pane
                    // 기준 fallback split. `pane_id` 는 방금 이 workspace 안에서
                    // 찾았으므로 여기선 항상 성공한다.
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

    /// `DomainIntent::RespawnTerminal` 본문. 새 Terminal 생성 → engine.replace_terminal_by_id.
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

    /// `DomainIntent::MoveWorkspace` 본문. workspaces 벡터의 from→to 이동.
    /// active_workspace 보정은 cascade 에서 처리 (Core 는 state 모름).
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

    /// `DomainIntent::UpdateWorkspaceMeta` 본문. `workspace_id` 로 찾고 None
    /// 아닌 필드만 갱신. cascade (`cascade_workspace_meta_updated`) 가 host
    /// event 발화.
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

    /// `DomainIntent::CreateWorkspace` 본문. engine 에 새 workspace + pane +
    /// tab + surface 를 생성하고 `WorkspaceCreated` event 를 반환한다.
    /// host event 발화 (WorkspaceRenamed) + (User origin 이면) active 전환은
    /// cascade (`cascade_workspace_created`) 에서 처리한다.
    pub(super) fn apply_create_workspace(
        &mut self,
        engine: &mut crate::core::CoreState,
        params: WorkspaceCreationParams,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        Ok(vec![apply_create_workspace_inner(engine, params)?])
    }

    /// 시스템 내부 invariant restorer — bootstrap / close 후 자동 재생성 /
    /// closed_item precondition 용. `kind="terminal"` + auto name + cwd 미지정.
    /// Intent 큐를 우회하므로 cascade 도중 호출해도 재진입 위험 없음.
    ///
    /// 옛 `AppState::add_workspace` 의 의미를 그대로 유지 — *동작 보존* 위해
    /// host event (WorkspaceCreated/Renamed) 발화하지 않는다. plugin 알림은
    /// 사용자/에이전트 의도 경로 (`DomainIntent::CreateWorkspace`) 만.
    ///
    /// 반환: 새 workspace 의 index (`engine.workspaces.len() - 1`).
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

    /// `DomainIntent::SaveLayoutNow` 본문. settings + force gate 를 통과하면
    /// 디스크에 저장 + `layout_dirty.clear()`. 옛 `App::flush_layout_persistence`
    /// 의 조건 분기 + 옛 `Core::save_layout` wrapper 본문을 흡수.
    ///
    /// **debounce 는 여기서 재지 않는다** — `force=false` 호출은 호스트의
    /// `Tick::LayoutFlush` 타이머가 데드라인에 도달했을 때만 오므로, 이 시점엔
    /// 이미 debounce 를 통과한 것이다(`docs/dev-guide/timer-hub.md`).
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
        // 부팅 때 이 슬롯을 읽지 못했으면 디스크에 사용자 레이아웃이 그대로 남아 있다.
        // 지금 상태를 쓰면 그것을 대체하므로 저장을 건너뛴다 — 로드 실패는 이미
        // `layout_persistence` 가 error 로 남겼고, 여기서는 매 flush 마다 반복되므로
        // debug 로만 흔적을 둔다.
        if engine.layout_slot_protected {
            tracing::debug!("layout save skipped: slot is locked because it could not be read");
            return CoreEvent::LayoutSaved;
        }
        // engine 이 점유한 슬롯에만 쓴다 — 창(engine)마다 자기 파일이라
        // `App::flush_layout_persistence` 가 전 engine 을 돌아도 서로 덮어쓰지 않는다.
        // `None` 은 headless engine — 복원 자체를 적용하지 않으므로 저장도 하지
        // 않는다. 저장을 건너뛰어도 `LayoutSaved` 는 그대로 반환한다(위쪽
        // `should_save` 스킵과 같은 의미론).
        let Some(slot) = engine.layout_slot else {
            return CoreEvent::LayoutSaved;
        };
        crate::core::layout_persistence::save_slot(engine, active_workspace, slot);
        engine.layout_dirty.clear();
        CoreEvent::LayoutSaved
    }

    /// `DomainIntent::ApplyPendingLayoutRestore` 본문. engine 의
    /// `pending_layout_restore` 를 take 해 `SavedLayout::restore` 호출. 성공 시
    /// `restored_active_workspace` 도 take 해 CoreEvent payload 로 caller 에게 넘김.
    /// caller (window_lifecycle.rs::create_app_state) 가 결과 받아
    /// `state.switch_workspace` 수행.
    pub(super) fn apply_apply_pending_layout_restore(
        engine: &mut crate::core::CoreState,
    ) -> CoreEvent {
        let Some(saved) = engine.pending_layout_restore.take() else {
            return CoreEvent::LayoutRestored {
                restored: false,
                active_workspace: None,
            };
        };

        // 복원이 surface_id 를 발급하기 *전에* 카운터 floor 를 memory.db 의 최대 stale
        // Scope::Surface id 위로 올린다. surface_meta 는 영속되지만 surface_id 는 매 실행
        // 재발급되므로, 이래야 복원 surface 자체가 재사용 id(=stale 메타 보유)와 겹치지
        // 않아 capture 가 남의 restore.command 를 읽지 않는다.
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

        // 복원으로 확정된 live id 외 모든 Surface scope 를 정리한다. 위 floor 시딩
        // (`seed_surface_id_floor`)은 새 surface 가 stale 메타와 겹치는 것만 막고 죽은
        // scope 자체를 지우지는 않으므로, 강제 종료 등으로 graceful close 가 호출되지
        // 못해 남은 stale 메타가 무한 누적되는 것을 여기서 끊는다.
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

/// `DomainIntent::CreateWorkspace` 가 운반하는 생성 파라미터 — `apply_create_workspace`
/// / `apply_create_workspace_inner` 양쪽이 개념적으로 하나의 "생성 요청" 을 낱개
/// 인자로 재나열하지 않도록 묶는다. 필드 구성은 `DomainIntent::CreateWorkspace` 와 1:1.
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
    /// 시스템 invariant restorer (`create_default_workspace` 등) 가 쓰는 기본값 —
    /// cwd 미지정 terminal, 이름/카테고리 자동.
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

/// `DomainIntent::CreateWorkspace` 의 *순수 engine* 구현. `Core::apply_create_workspace`
/// 와 `Core::create_default_workspace` 양쪽이 공유.
///
/// 반환: `CoreEvent::WorkspaceCreated`. host event (WorkspaceRenamed) +
/// (User origin 이면) active 전환은 호출 측 cascade 책임.
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
        let tab_name = crate::state::pane::default_tab_name_for_kind(
            &kind,
            &surface_params,
            engine.surface_registry.get(&kind).as_deref(),
        );
        let pane = crate::model::Pane::new_with_surface(pane_id, tab_id, tab_name, surface);
        crate::model::Workspace::new_with_pane(ws_id, auto_name, pane)
    };

    // 카테고리 소속 지정(존재하는 카테고리만). 없거나 dangling 이면 normal(기본) 유지.
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

/// `RestoreClosedItem` 의 helper. pane_id 에 tab attach + active_tab 갱신.
/// *모든* workspace 순회 (포커스 독립).
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

/// 복원 직전 surface 카운터 floor 시딩 — memory.db 의 stale `Scope::Surface` 최대 id 위로
/// 올려 재사용을 원천 차단한다.
///
/// **PTY id 공간(`>= PTY_ID_BASE`)을 침범한 scope 는 floor 산정에서 제외하고 그 자리에서
/// purge 한다.** 포함하면 오염된 scope 하나가 카운터를 PTY 공간으로 밀어 올리고, 그 실행이
/// 발급한 surface 들이 다시 memory.db 에 기록되어 다음 부팅의 floor 를 유지하는 **비가역
/// 래칫**이 된다(`docs/adr/0094-surface-id-space-bounded-below-pty-base.md`). 제외 + purge
/// 이므로 이미 래칫이 걸린 인스턴스도 부팅 한 번으로 정상 범위로 복귀한다.
///
/// purge 되는 scope 는 정의상 이전 실행의 잔재다(부팅 시점에 live surface 는 아직 없다) —
/// 곧이어 도는 `purge_dead_surfaces` 가 어차피 지울 대상이므로 추가 손실이 없다.
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
    // `max_surface_id` 의 상한은 `PTY_ID_BASE - 1` 이므로 `mem_max + 1` 은 overflow 하지
    // 않는다. floor 는 정상 경로에서 PTY 공간 아래에 머문다 — 단 `mem_max` 가 상한
    // (`0x7FFF_FFFF`)일 때만 `mem_max + 1 == PTY_ID_BASE` 가 되어 경계에 닿는다. 한 실행이
    // 20 억 개 가까운 surface 를 발급해야 도달하므로 여기서 clamp 하지 않는다.
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
        // memory.db 가 PTY id 공간을 침범한 scope 로 오염된 상태(실사용 관측값 재현).
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
