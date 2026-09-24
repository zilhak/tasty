//! mirror workspace의 구조 변경은 로컬 실행을 막고, GUI에서는 원격 실행 큐로 보낸다.

use super::*;

/// mirror의 구조 변경을 로컬에서 실행하지 않았다는 오류.
/// 호출자는 downcast해 forward 여부에 맞게 응답·사용자 안내를 처리한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MirrorStructuralBlocked {
    pub workspace_index: usize,
    /// 원격 실행 큐에 넣었으면 true다. 원격에서 성공했다는 뜻은 아니다.
    /// 헤드리스나 forward할 수 없는 대상이면 false다.
    pub forwarded: bool,
}

impl std::fmt::Display for MirrorStructuralBlocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "structural change rejected: target belongs to a mirror (remote attach) workspace; \
             the operation must be performed on the remote instance"
        )?;
        // 헤드리스는 특정 작업이 아니라 forward 경로 자체가 없어 거절한다.
        #[cfg(not(feature = "gui"))]
        write!(
            f,
            " (this headless build has no attach client to forward it)"
        )?;
        Ok(())
    }
}

impl std::error::Error for MirrorStructuralBlocked {}

/// 원격 실행 요청. Core::apply는 호출 주체를 몰라 user_triggered=false로 넣는다.
/// GUI 호출부가 사용자 요청임을 표시하면 새 surface로 선택을 옮길 수 있다.
/// close_focus_candidates는 닫힘 뒤 선택할 로컬 surface 후보이며 우선순위 순서다.
#[derive(Debug, Clone)]
#[cfg_attr(
    all(not(feature = "gui"), not(test)),
    expect(
        dead_code,
        reason = "the GUI loop reads queued operations; headless only shares the marking helpers, while tests read these fields"
    )
)]
pub(crate) struct PendingStructuralForward {
    pub(crate) op: tasty_ipc::stream::StructuralOp,
    pub(crate) user_triggered: bool,
    pub(crate) close_focus_candidates: Vec<u32>,
    /// 에이전트 요청의 실패는 toast 대신 로그로 남긴다.
    /// 기본값은 false이므로 origin을 아는 호출자가 명시해야 한다.
    pub(crate) silent_failure: bool,
}

impl PendingStructuralForward {
    #[cfg(feature = "gui")]
    fn agent(op: tasty_ipc::stream::StructuralOp) -> Self {
        Self {
            op,
            user_triggered: false,
            close_focus_candidates: Vec::new(),
            silent_failure: false,
        }
    }
}

/// forward된 오류와 Agent origin일 때 마지막 요청의 실패 안내를 숨긴다.
/// 요청 ID를 대조하지 않으므로 해당 apply 직후, 다른 요청을 넣기 전에 호출해야 한다.
pub(crate) fn mark_last_forward_agent_origin(
    engine: &mut CoreState,
    err: &anyhow::Error,
    origin: &crate::core::origin::IntentOrigin,
) {
    let Some(blocked) = err.downcast_ref::<MirrorStructuralBlocked>() else {
        return;
    };
    if !blocked.forwarded || !origin.is_agent() {
        return;
    }
    if let Some(last) = engine.pending_structural_forward.last_mut() {
        last.silent_failure = true;
    }
}

/// forward된 오류와 User origin일 때 마지막 요청을 사용자 조작으로 표시한다.
/// 해당 apply 직후 호출해야 다른 요청을 잘못 표시하지 않는다.
pub(crate) fn mark_last_forward_user_triggered(
    engine: &mut CoreState,
    err: &anyhow::Error,
    origin: &crate::core::origin::IntentOrigin,
) {
    let Some(blocked) = err.downcast_ref::<MirrorStructuralBlocked>() else {
        return;
    };
    if !blocked.forwarded || !origin.is_user() {
        return;
    }
    if let Some(last) = engine.pending_structural_forward.last_mut() {
        last.user_triggered = true;
    }
}

/// anchor는 로컬 surface ID이며 전송할 때 세션 매핑으로 바꾼다.
/// pane·tab 작업은 대표 surface를 찾는다. MoveSurface는 같은 workspace의 두 대상만 허용한다.
/// 다른 workspace의 로컬 ID를 보내면 원격의 무관한 surface ID와 겹칠 수 있다.
#[cfg(feature = "gui")]
fn build_mirror_forward_op(
    engine: &crate::core::CoreState,
    intent: &DomainIntent,
) -> Option<tasty_ipc::stream::StructuralOp> {
    use crate::core::intent::DomainIntent as D;
    use tasty_ipc::stream::{SplitAxis, StructuralOp};

    fn axis(d: &crate::model::SplitDirection) -> SplitAxis {
        match d {
            crate::model::SplitDirection::Horizontal => SplitAxis::Horizontal,
            crate::model::SplitDirection::Vertical => SplitAxis::Vertical,
        }
    }
    let pane_anchor = |pane_id: u32| -> Option<u32> {
        engine
            .find_pane_by_id(pane_id)
            .and_then(|p| p.tabs.get(p.active_tab))
            .and_then(|t| t.focused_surface_id())
    };
    let tab_anchor = |tab_id: u32| -> Option<u32> {
        for ws in &engine.workspaces {
            for pid in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        if tab.id == tab_id {
                            return tab.focused_surface_id();
                        }
                    }
                }
            }
        }
        None
    };

    match intent {
        D::SplitSurface {
            target_surface_id,
            direction,
            kind,
            surface_params,
            ..
        } => Some(StructuralOp::SplitSurface {
            surface_id: *target_surface_id,
            direction: axis(direction),
            surface_kind: kind.clone(),
            params: surface_params.clone(),
        }),
        D::SplitPane {
            target_pane_id,
            direction,
            kind,
            surface_params,
            ..
        } => Some(StructuralOp::SplitPane {
            anchor_surface_id: pane_anchor(*target_pane_id)?,
            direction: axis(direction),
            surface_kind: kind.clone(),
            params: surface_params.clone(),
        }),
        D::CreateTab {
            pane_id,
            kind,
            surface_params,
            ..
        } => Some(StructuralOp::NewTab {
            anchor_surface_id: pane_anchor(*pane_id)?,
            surface_kind: kind.clone(),
            params: surface_params.clone(),
        }),
        D::CloseSurface { surface_id, .. } => Some(StructuralOp::CloseSurface {
            surface_id: *surface_id,
        }),
        D::CloseTab { tab_id } => Some(StructuralOp::CloseTab {
            anchor_surface_id: tab_anchor(*tab_id)?,
        }),
        D::ClosePane { pane_id } => Some(StructuralOp::ClosePane {
            anchor_surface_id: pane_anchor(*pane_id)?,
        }),
        // 복원할 항목은 서버의 복원 목록에서 고르므로 anchor만 보낸다.
        D::RestoreClosedItem { target_pane_id, .. } => Some(StructuralOp::RestoreClosedItem {
            anchor_surface_id: pane_anchor((*target_pane_id)?)?,
        }),
        D::MoveTab {
            pane_id,
            from_index,
            to_index,
        } => Some(StructuralOp::MoveTab {
            anchor_surface_id: pane_anchor(*pane_id)?,
            from_index: *from_index,
            to_index: *to_index,
        }),
        D::ConvertSurface { surface_id, target } => {
            use crate::core::intent::ConvertSurfaceTarget;
            // 명시된 cwd는 그대로 보낸다. 없으면 서버가 자체 설정에 따라 터미널 cwd를 조회한다.
            let (surface_kind, params, cwd) = match target {
                ConvertSurfaceTarget::Terminal { cwd } => {
                    ("terminal".to_string(), serde_json::json!({}), cwd.clone())
                }
                ConvertSurfaceTarget::Kind { kind, params, cwd } => {
                    (kind.clone(), params.clone(), cwd.clone())
                }
            };
            Some(StructuralOp::ConvertSurface {
                surface_id: *surface_id,
                surface_kind,
                params,
                cwd: cwd.map(|p| p.to_string_lossy().into_owned()),
            })
        }
        D::MoveSurface {
            source_surface_id,
            target_surface_id,
        } => {
            let src_ws = engine
                .find_workspace_index_for_surface(*source_surface_id)
                .map(|(i, _)| i);
            let tgt_ws = engine
                .find_workspace_index_for_surface(*target_surface_id)
                .map(|(i, _)| i);
            if src_ws.is_some() && src_ws == tgt_ws {
                Some(StructuralOp::MoveSurface {
                    source_surface_id: *source_surface_id,
                    target_surface_id: *target_surface_id,
                })
            } else {
                None
            }
        }
        _ => None,
    }
}

#[cfg(feature = "gui")]
fn queue_mirror_forward(engine: &mut crate::core::CoreState, intent: &DomainIntent) -> bool {
    match build_mirror_forward_op(engine, intent) {
        Some(op) => {
            engine
                .pending_structural_forward
                .push(PendingStructuralForward::agent(op));
            true
        }
        None => false,
    }
}

/// 헤드리스에는 큐를 전송하는 메인 루프가 없어 성공으로 답하지 않고 거절한다.
#[cfg(not(feature = "gui"))]
fn queue_mirror_forward(_engine: &mut crate::core::CoreState, _intent: &DomainIntent) -> bool {
    false
}

impl Core {
    /// intent를 적용하고 후속 처리용 이벤트를 반환한다. 일부 작업은 여기서 상태를 바꾸고,
    /// 설정·알림 등은 이벤트를 받은 App dispatcher가 적용한다.
    pub(crate) fn apply(
        &mut self,
        engine: &mut crate::core::CoreState,
        intent: DomainIntent,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        // mirror 구조를 로컬에서 바꾸면 원격 트리와 달라지므로 먼저 forward 또는 거절한다.
        if let Some(workspace_index) = engine.mirror_workspace_index_for_structural(&intent) {
            let forwarded = queue_mirror_forward(engine, &intent);
            return Err(anyhow::Error::new(MirrorStructuralBlocked {
                workspace_index,
                forwarded,
            }));
        }
        match intent {
            DomainIntent::UpdateSettings(new_settings) => {
                Ok(vec![CoreEvent::SettingsUpdated(new_settings)])
            }
            DomainIntent::PushNotification {
                ws_id,
                surface_id,
                title,
                body,
                source,
            } => Ok(vec![CoreEvent::NotificationPushRequested {
                ws_id,
                surface_id,
                title,
                body,
                source,
            }]),
            #[cfg(feature = "gui")]
            DomainIntent::MarkNotificationRead { id } => {
                Ok(vec![CoreEvent::NotificationReadRequested { id }])
            }
            #[cfg(feature = "gui")]
            DomainIntent::MarkAllNotificationsRead => {
                Ok(vec![CoreEvent::AllNotificationsReadRequested])
            }
            #[cfg(feature = "gui")]
            DomainIntent::SurfaceCwdChanged { surface_id } => {
                Ok(vec![CoreEvent::SurfaceCwdChanged { surface_id }])
            }
            DomainIntent::SetTerminalMark { surface_id } => {
                Ok(vec![CoreEvent::TerminalMarkSet { surface_id }])
            }
            DomainIntent::SurfaceCompletion { surface_id, kind } => {
                Ok(vec![CoreEvent::SurfaceCompletionRequested {
                    surface_id,
                    kind,
                }])
            }
            DomainIntent::SurfaceAttentionClear { surface_id, kind } => {
                Ok(vec![CoreEvent::SurfaceAttentionClearRequested {
                    surface_id,
                    kind,
                }])
            }
            DomainIntent::CreateWorkspace {
                cwd,
                kind,
                surface_params,
                name,
                subtitle,
                description,
                category,
            } => self.apply_create_workspace(
                engine,
                WorkspaceCreationParams {
                    cwd,
                    kind,
                    surface_params,
                    name,
                    subtitle,
                    description,
                    category,
                },
            ),
            DomainIntent::UpdateWorkspaceMeta {
                workspace_id,
                name,
                subtitle,
                description,
            } => {
                self.apply_update_workspace_meta(engine, workspace_id, name, subtitle, description)
            }
            DomainIntent::MoveWorkspace {
                from_index,
                to_index,
            } => Ok(vec![
                self.apply_move_workspace(engine, from_index, to_index),
            ]),
            DomainIntent::CreateTab {
                pane_id,
                cwd,
                kind,
                name,
                surface_params,
                activate,
            } => Self::apply_create_tab(engine, pane_id, cwd, kind, name, surface_params, activate),
            DomainIntent::CloseTab { tab_id } => Ok(vec![Self::apply_close_tab(engine, tab_id)]),
            DomainIntent::MoveTab {
                pane_id,
                from_index,
                to_index,
            } => Ok(vec![Self::apply_move_tab(
                engine, pane_id, from_index, to_index,
            )]),
            DomainIntent::AdoptTerminal { pane_id, pty_id } => {
                Self::apply_adopt_terminal(engine, pane_id, pty_id)
            }
            DomainIntent::SplitPane {
                target_pane_id,
                direction,
                cwd,
                kind,
                surface_params,
            } => {
                Self::apply_split_pane(engine, target_pane_id, direction, cwd, kind, surface_params)
            }
            DomainIntent::SplitSurface {
                target_surface_id,
                direction,
                cwd,
                kind,
                surface_params,
            } => Self::apply_split_surface(
                engine,
                target_surface_id,
                direction,
                cwd,
                kind,
                surface_params,
            ),
            DomainIntent::ClosePane { pane_id } => {
                Ok(vec![Self::apply_close_pane(engine, pane_id)])
            }
            DomainIntent::CloseSurface {
                surface_id,
                save_snapshot,
            } => Ok(vec![Self::apply_close_surface(
                engine,
                surface_id,
                save_snapshot,
            )]),
            DomainIntent::ConvertSurface { surface_id, target } => {
                Ok(vec![Self::apply_convert_surface(
                    engine, surface_id, target,
                )])
            }
            DomainIntent::MoveSurface {
                source_surface_id,
                target_surface_id,
            } => Ok(vec![Self::apply_move_surface(
                engine,
                source_surface_id,
                target_surface_id,
            )]),
            DomainIntent::SendToSurface {
                surface_id,
                payload,
            } => Ok(vec![Self::apply_send_to_surface(
                engine, surface_id, payload,
            )]),
            DomainIntent::RespawnTerminal { surface_id, cwd } => {
                Ok(vec![Self::apply_respawn_terminal(engine, surface_id, cwd)])
            }
            DomainIntent::RestoreClosedItem {
                target_pane_id,
                scope,
            } => Ok(vec![Self::apply_restore_closed_item(
                engine,
                target_pane_id,
                scope,
            )]),
            #[cfg(feature = "gui")]
            DomainIntent::UpdateTabName { surface_id, name } => {
                Ok(vec![Self::apply_update_tab_name(engine, surface_id, name)])
            }
            #[cfg(feature = "gui")]
            DomainIntent::SaveLayoutNow {
                active_workspace,
                force,
            } => Ok(vec![Self::apply_save_layout_now(
                engine,
                active_workspace,
                force,
            )]),
            #[cfg(feature = "gui")]
            DomainIntent::ApplyPendingLayoutRestore => {
                Ok(vec![Self::apply_apply_pending_layout_restore(engine)])
            }
            // 결과를 이벤트 루프로 돌려주는 identify worker는 GUI에만 있다.
            #[cfg(feature = "gui")]
            DomainIntent::DispatchFile {
                target,
                depth,
                origin_surface_id,
                dispatch_origin,
                ignore_size_limit,
            } => {
                if let Some(sid) = origin_surface_id {
                    crate::core::origin::require_origin_pane(engine, sid)
                        .map_err(anyhow::Error::msg)?;
                }
                match engine.identify_worker.as_ref() {
                    Some(worker) => {
                        worker.spawn_identify(
                            target,
                            depth,
                            origin_surface_id,
                            dispatch_origin,
                            ignore_size_limit,
                        );
                    }
                    None => {
                        tracing::warn!(
                            target = %target.display(),
                            "DispatchFile: identify_worker not injected — drop",
                        );
                    }
                }
                Ok(vec![])
            }
        }
    }
}

#[cfg(test)]
mod mirror_structural_guard_tests {
    use super::*;
    use crate::core::intent::DomainIntent;
    use crate::model::SplitDirection;
    use tasty_terminal::Terminal;

    fn build_test_core() -> (Core, CoreState) {
        use std::sync::{Arc, Mutex};

        use crate::adapters::test::{
            fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
            mock_process::MockProcessSpawner, tmp_home::TmpHome,
        };
        use crate::core::builder::CoreBuilder;
        use crate::ports::notification_sound::NoopPlayer;

        let waker: tasty_terminal::Waker = Arc::new(|| {});
        let engine = CoreState::new(80, 24, waker).expect("engine");

        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn tasty_memory::MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn tasty_themes::ThemeStorage> = Arc::new(tasty_themes::ThemeStore::new());

        let core = CoreBuilder::new()
            .with_fs(Arc::new(MemFileSystem::new()))
            .with_clock(Arc::new(FakeClock::default()))
            .with_clipboard(Arc::new(MockClipboard::default()))
            .with_process(Arc::new(MockProcessSpawner))
            .with_home(Arc::new(TmpHome::new(
                tempfile::tempdir().expect("tmp").keep(),
            )))
            .with_sound_player(Arc::new(NoopPlayer))
            .with_memory(memory)
            .with_themes(themes)
            .with_preset_store(preset_store)
            .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
            .build()
            .expect("test Core");
        (core, engine)
    }

    fn seed(engine: &mut CoreState) -> (u32, u32) {
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine.terminals.insert(a, Terminal::new_detached(80, 24));
        let (_ws, pane) = engine.find_workspace_index_for_surface(a).unwrap();
        (a, pane)
    }

    fn is_blocked(err: &anyhow::Error) -> bool {
        err.downcast_ref::<MirrorStructuralBlocked>().is_some()
    }

    #[test]
    fn mirror_split_and_newtab_are_blocked_without_spawning() {
        let (mut core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let before = engine.terminals.iter().count();

        for intent in [
            DomainIntent::SplitSurface {
                target_surface_id: a,
                direction: SplitDirection::Horizontal,
                cwd: None,
                kind: "terminal".to_string(),
                surface_params: serde_json::json!({}),
            },
            DomainIntent::SplitPane {
                target_pane_id: pane,
                direction: SplitDirection::Horizontal,
                cwd: None,
                kind: "terminal".to_string(),
                surface_params: serde_json::json!({}),
            },
            DomainIntent::CreateTab {
                pane_id: pane,
                cwd: None,
                kind: "terminal".to_string(),
                name: None,
                surface_params: serde_json::json!({}),
                activate: false,
            },
        ] {
            let err = core
                .apply(&mut engine, intent)
                .expect_err("must be blocked");
            assert!(
                is_blocked(&err),
                "expected MirrorStructuralBlocked, got: {err}"
            );
            assert_eq!(
                engine.terminals.iter().count(),
                before,
                "mirror 워크스페이스에서 새 로컬 터미널이 insert 되면 안 된다"
            );
        }
    }

    #[test]
    fn non_mirror_split_passes_and_spawns() {
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        assert!(!engine.workspaces[0].mirror);
        let before = engine.terminals.iter().count();

        core.apply(
            &mut engine,
            DomainIntent::SplitSurface {
                target_surface_id: a,
                direction: SplitDirection::Horizontal,
                cwd: None,
                kind: "terminal".to_string(),
                surface_params: serde_json::json!({}),
            },
        )
        .expect("non-mirror split must succeed");
        assert_eq!(
            engine.terminals.iter().count(),
            before + 1,
            "비-mirror split은 로컬 터미널을 1개 늘려야 한다"
        );
    }

    #[test]
    fn create_tab_in_occupied_workspace_inherits_occupancy() {
        let (mut core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let client_id = 42;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");

        let events = core
            .apply(
                &mut engine,
                DomainIntent::CreateTab {
                    pane_id: pane,
                    cwd: None,
                    kind: "terminal".to_string(),
                    name: None,
                    surface_params: serde_json::json!({}),
                    activate: false,
                },
            )
            .expect("create tab must succeed");
        let Some(CoreEvent::TabCreated { surface_id, .. }) = events.into_iter().next() else {
            panic!("expected TabCreated event");
        };

        assert!(
            engine.attach.is_hard_occupied(surface_id),
            "새 tab 의 터미널은 hard 점유를 상속해야 한다"
        );
        assert_eq!(
            engine.attach.workspace_of_surface(surface_id),
            Some(ws_id),
            "새 surface 는 점유 workspace 멤버로 등록돼야 한다"
        );
        assert_eq!(
            engine.attach.workspace_holder_of(surface_id),
            Some(client_id),
            "새 surface 의 holder 는 workspace holder 와 동일해야 한다"
        );
    }

    #[test]
    fn split_pane_in_occupied_workspace_inherits_occupancy() {
        let (mut core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let client_id = 7;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");

        let events = core
            .apply(
                &mut engine,
                DomainIntent::SplitPane {
                    target_pane_id: pane,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect("split pane must succeed");
        let Some(CoreEvent::PaneSplit { new_surface_id, .. }) = events.into_iter().next() else {
            panic!("expected PaneSplit event");
        };

        assert!(engine.attach.is_hard_occupied(new_surface_id));
        assert_eq!(
            engine.attach.workspace_of_surface(new_surface_id),
            Some(ws_id)
        );
        assert_eq!(
            engine.attach.workspace_holder_of(new_surface_id),
            Some(client_id)
        );
    }

    #[test]
    fn split_surface_in_occupied_workspace_inherits_occupancy() {
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let client_id = 9;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");

        let events = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect("split surface must succeed");
        let Some(CoreEvent::SurfaceSplit { new_surface_id, .. }) = events.into_iter().next() else {
            panic!("expected SurfaceSplit event");
        };

        assert!(engine.attach.is_hard_occupied(new_surface_id));
        assert_eq!(
            engine.attach.workspace_of_surface(new_surface_id),
            Some(ws_id)
        );
        assert_eq!(
            engine.attach.workspace_holder_of(new_surface_id),
            Some(client_id)
        );
    }

    #[test]
    fn adopt_terminal_in_occupied_workspace_inherits_occupancy() {
        use crate::core::pty_registry::PtySpawnSpec;

        let (mut core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let client_id = 13;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");

        let pty_id = engine
            .pty_registry
            .register(
                PtySpawnSpec {
                    owner_agent_id: "agent-x".into(),
                    cwd: None,
                    command: vec![],
                },
                std::time::Instant::now(),
            )
            .expect("register headless pty");
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(pty_id);
        let terminal = tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols: 80,
                rows: 24,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
                extra_env: &sh.envs_ref(),
                surface_id: pty_id,
                working_dir: None,
                initial_input: None,
            },
            waker,
        )
        .expect("spawn headless terminal");
        engine.terminals.insert(pty_id, terminal);

        let events = core
            .apply(
                &mut engine,
                DomainIntent::AdoptTerminal {
                    pane_id: pane,
                    pty_id,
                },
            )
            .expect("adopt must succeed");
        let Some(CoreEvent::TabCreated { surface_id, .. }) = events.into_iter().next() else {
            panic!("expected TabCreated event");
        };

        assert!(engine.attach.is_hard_occupied(surface_id));
        assert_eq!(engine.attach.workspace_of_surface(surface_id), Some(ws_id));
        assert_eq!(
            engine.attach.workspace_holder_of(surface_id),
            Some(client_id)
        );

        engine.terminals.remove(surface_id);
    }

    #[test]
    fn adopt_terminal_promotes_headless_pty_preserving_state() {
        use crate::core::pty_registry::PtySpawnSpec;

        let (mut core, mut engine) = build_test_core();
        let (_a, pane) = seed(&mut engine);

        let pty_id = engine
            .pty_registry
            .register(
                PtySpawnSpec {
                    owner_agent_id: "agent-x".into(),
                    cwd: None,
                    command: vec![],
                },
                std::time::Instant::now(),
            )
            .expect("register headless pty");
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(pty_id);
        let terminal = tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols: 80,
                rows: 24,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
                extra_env: &sh.envs_ref(),
                surface_id: pty_id,
                working_dir: None,
                initial_input: None,
            },
            waker,
        )
        .expect("spawn headless terminal");
        engine.terminals.insert(pty_id, terminal);

        // 화면 내용을 만든 뒤 이동 후에도 남는지 검사한다. 이것만으로 프로세스 동일성을 증명하지는 않는다.
        engine
            .find_terminal_by_id_mut(pty_id)
            .expect("headless terminal")
            .send_bytes(b"echo ADOPT_MARKER_123\n");
        let mut seen = false;
        for _ in 0..500 {
            engine.process_surface(pty_id);
            if engine
                .find_terminal_by_id(pty_id)
                .map(|t| t.screen_text(true))
                .unwrap_or_default()
                .contains("ADOPT_MARKER_123")
            {
                seen = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(seen, "marker should appear before adoption");

        let events = core
            .apply(
                &mut engine,
                DomainIntent::AdoptTerminal {
                    pane_id: pane,
                    pty_id,
                },
            )
            .expect("adopt must succeed");
        let surface_id = match events.into_iter().next() {
            Some(CoreEvent::TabCreated {
                surface_id,
                pane_id: p,
                ..
            }) => {
                assert_eq!(p, pane, "TabCreated pane_id");
                surface_id
            }
            other => panic!("expected TabCreated cascade event, got {other:?}"),
        };

        assert!(
            engine.find_terminal_by_id(pty_id).is_none(),
            "old pty_id key removed from store"
        );
        let screen = engine
            .find_terminal_by_id(surface_id)
            .expect("terminal now at surface_id")
            .screen_text(true);
        assert!(
            screen.contains("ADOPT_MARKER_123"),
            "screen contents preserved across promotion: {screen:?}"
        );

        assert!(
            !engine.pty_registry.contains(pty_id),
            "promoted pty must leave the headless registry"
        );

        let pane_ref = engine.find_pane_by_id(pane).expect("pane");
        assert!(
            pane_ref
                .tabs
                .iter()
                .any(|t| t.all_surface_ids().contains(&surface_id)),
            "promoted surface must appear in the pane's tabs"
        );

        engine.terminals.remove(surface_id);
    }

    /// 새 ID로 옮긴 뒤 옛 PTY ID의 waker 항목은 지우고 새 항목은 유지해야 한다.
    #[test]
    fn adopt_terminal_forgets_old_pty_waker_gate() {
        use crate::adapters::test::mock_waker_factory::RecordingWakerFactory;
        use crate::core::pty_registry::PtySpawnSpec;

        let (mut core, mut engine) = build_test_core();
        let factory = RecordingWakerFactory::new();
        let shared: crate::waker::SharedWakerFactory = factory.clone();
        engine.waker_factory = Some(shared);
        let (_a, pane) = seed(&mut engine);

        let pty_id = engine
            .pty_registry
            .register(
                PtySpawnSpec {
                    owner_agent_id: "agent-x".into(),
                    cwd: None,
                    command: vec![],
                },
                std::time::Instant::now(),
            )
            .expect("register headless pty");
        let sh = crate::core::state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(pty_id);
        let terminal = tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols: 80,
                rows: 24,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
                extra_env: &sh.envs_ref(),
                surface_id: pty_id,
                working_dir: None,
                initial_input: None,
            },
            waker,
        )
        .expect("spawn headless terminal");
        engine.terminals.insert(pty_id, terminal);
        assert!(
            factory.made().contains(&pty_id),
            "spawn 흉내는 pty_id 게이트를 만든다"
        );

        let events = core
            .apply(
                &mut engine,
                DomainIntent::AdoptTerminal {
                    pane_id: pane,
                    pty_id,
                },
            )
            .expect("adopt must succeed");
        let surface_id = match events.into_iter().next() {
            Some(CoreEvent::TabCreated { surface_id, .. }) => surface_id,
            other => panic!("expected TabCreated, got {other:?}"),
        };

        assert!(
            factory.forgotten().contains(&pty_id),
            "adopt 는 옛 pty_id 의 waker 게이트를 정리해야 한다"
        );
        assert!(
            !factory.forgotten().contains(&surface_id),
            "재배선된 새 surface_id 게이트는 정리 대상이 아니다"
        );

        engine.terminals.remove(surface_id);
    }

    #[test]
    fn adopt_unknown_pty_errors() {
        let (mut core, mut engine) = build_test_core();
        let (_a, pane) = seed(&mut engine);
        let bogus = crate::core::pty_registry::PTY_ID_BASE + 4242;
        let before = engine.terminals.iter().count();
        let err = core
            .apply(
                &mut engine,
                DomainIntent::AdoptTerminal {
                    pane_id: pane,
                    pty_id: bogus,
                },
            )
            .expect_err("unknown pty must error");
        assert!(err.to_string().contains("not found"), "err: {err}");
        assert_eq!(
            engine.terminals.iter().count(),
            before,
            "실패한 승격은 store 를 건드리지 않아야 한다"
        );
    }

    #[test]
    fn helper_flags_structural_targets_only_when_mirror() {
        let (_core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        let tab_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        engine
            .terminals
            .insert(sid1, Terminal::new_detached(80, 24));
        engine.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane)
            .unwrap()
            .add_terminal_marker_tab(tab_id, sid1);

        let structural = |a: u32, pane: u32, tab_id: u32| {
            vec![
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
                DomainIntent::SplitPane {
                    target_pane_id: pane,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
                DomainIntent::CreateTab {
                    pane_id: pane,
                    cwd: None,
                    kind: "terminal".to_string(),
                    name: None,
                    surface_params: serde_json::json!({}),
                    activate: false,
                },
                DomainIntent::CloseSurface {
                    surface_id: a,
                    save_snapshot: false,
                },
                DomainIntent::ClosePane { pane_id: pane },
                DomainIntent::CloseTab { tab_id },
                DomainIntent::MoveTab {
                    pane_id: pane,
                    from_index: 0,
                    to_index: 1,
                },
                DomainIntent::RestoreClosedItem {
                    target_pane_id: Some(pane),
                    scope: crate::core::intent::RestoreScope::Local,
                },
            ]
        };

        for intent in structural(a, pane, tab_id) {
            assert_eq!(
                engine.mirror_workspace_index_for_structural(&intent),
                None,
                "비-mirror 는 통과해야 한다: {intent:?}"
            );
        }
        engine.workspaces[0].mirror = true;
        for intent in structural(a, pane, tab_id) {
            assert_eq!(
                engine.mirror_workspace_index_for_structural(&intent),
                Some(0),
                "mirror 는 차단 대상이어야 한다: {intent:?}"
            );
        }
        assert_eq!(
            engine.mirror_workspace_index_for_structural(&DomainIntent::SetTerminalMark {
                surface_id: a
            }),
            None,
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mirror_split_enqueues_forward_with_local_anchor() {
        use tasty_ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        assert!(engine.pending_structural_forward.is_empty());

        let err = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect_err("mirror split must be blocked locally");
        let blocked = err
            .downcast_ref::<MirrorStructuralBlocked>()
            .expect("MirrorStructuralBlocked");
        assert!(blocked.forwarded, "forward 가능 op 는 forwarded=true");
        assert_eq!(engine.pending_structural_forward.len(), 1);
        let queued = &engine.pending_structural_forward[0];
        assert!(
            !queued.user_triggered,
            "Core::apply 는 origin 을 모르므로 기본 user_triggered=false"
        );
        match &queued.op {
            StructuralOp::SplitSurface { surface_id, .. } => {
                assert_eq!(*surface_id, a, "anchor 는 로컬 surface a");
            }
            other => panic!("expected SplitSurface, got {other:?}"),
        }
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mirror_split_pane_anchors_on_pane_surface() {
        use tasty_ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        core.apply(
            &mut engine,
            DomainIntent::SplitPane {
                target_pane_id: pane,
                direction: SplitDirection::Vertical,
                cwd: None,
                kind: "terminal".to_string(),
                surface_params: serde_json::json!({}),
            },
        )
        .expect_err("blocked");
        match &engine.pending_structural_forward[0].op {
            StructuralOp::SplitPane {
                anchor_surface_id, ..
            } => assert_eq!(*anchor_surface_id, a, "pane anchor = 활성 탭 surface a"),
            other => panic!("expected SplitPane, got {other:?}"),
        }
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mirror_restore_enqueues_forward_and_leaves_the_local_stack_alone() {
        use tasty_ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, pane) = seed(&mut engine);
        engine.push_closed_item(crate::model::ClosedItem::Surface {
            surface: crate::model::closed_item::ClosedSurface::from_surface_id(9999, None),
            tab_name: "gone".to_string(),
        });
        let before_len = engine.closed_items.len();
        engine.workspaces[0].mirror = true;

        let err = core
            .apply(
                &mut engine,
                DomainIntent::RestoreClosedItem {
                    target_pane_id: Some(pane),
                    scope: crate::core::intent::RestoreScope::Local,
                },
            )
            .expect_err("mirror restore must be blocked locally");
        let blocked = err
            .downcast_ref::<MirrorStructuralBlocked>()
            .expect("MirrorStructuralBlocked");
        assert!(blocked.forwarded, "복원은 forward 대상이다");
        assert_eq!(engine.pending_structural_forward.len(), 1);
        match &engine.pending_structural_forward[0].op {
            StructuralOp::RestoreClosedItem { anchor_surface_id } => {
                assert_eq!(*anchor_surface_id, a, "anchor 는 그 pane 의 대표 surface");
            }
            other => panic!("expected RestoreClosedItem, got {other:?}"),
        }
        assert_eq!(
            engine.closed_items.len(),
            before_len,
            "로컬 스택은 그대로여야 한다 — 로컬 pop 이 일어나면 안 된다"
        );
    }

    /// 대상 pane이 없으면 mirror 판정을 할 수 없어 일반 복원 경로로 진행한다.
    #[test]
    fn a_restore_without_a_target_pane_is_not_a_mirror_op() {
        let (mut _core, mut engine) = build_test_core();
        let (_a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        assert_eq!(
            engine.mirror_workspace_index_for_structural(&DomainIntent::RestoreClosedItem {
                target_pane_id: None,
                scope: crate::core::intent::RestoreScope::Local,
            }),
            None,
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mirror_convert_enqueues_forward_with_local_anchor() {
        use tasty_ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let err = core
            .apply(
                &mut engine,
                DomainIntent::ConvertSurface {
                    surface_id: a,
                    target: crate::core::intent::ConvertSurfaceTarget::Kind {
                        cwd: None,
                        kind: "markdown".to_string(),
                        params: serde_json::json!({ "file": "/tmp/a.md" }),
                    },
                },
            )
            .expect_err("blocked locally");
        let blocked = err
            .downcast_ref::<MirrorStructuralBlocked>()
            .expect("MirrorStructuralBlocked");
        assert!(blocked.forwarded, "convert는 forward 큐에 들어가야 한다");
        assert_eq!(engine.pending_structural_forward.len(), 1);
        match &engine.pending_structural_forward[0].op {
            StructuralOp::ConvertSurface {
                surface_id,
                surface_kind,
                params,
                cwd,
            } => {
                assert_eq!(*surface_id, a, "anchor 는 로컬 surface a");
                assert_eq!(surface_kind, "markdown");
                assert_eq!(params, &serde_json::json!({ "file": "/tmp/a.md" }));
                assert!(
                    cwd.is_none(),
                    "cwd 를 지정하지 않은 convert 는 None 으로 나간다"
                );
            }
            other => panic!("expected ConvertSurface, got {other:?}"),
        }
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mirror_convert_forwards_cwd() {
        use tasty_ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let err = core
            .apply(
                &mut engine,
                DomainIntent::ConvertSurface {
                    surface_id: a,
                    target: crate::core::intent::ConvertSurfaceTarget::Kind {
                        cwd: Some(std::path::PathBuf::from("/tmp/proj")),
                        kind: "explorer".to_string(),
                        params: serde_json::json!({}),
                    },
                },
            )
            .expect_err("blocked locally");
        assert!(
            err.downcast_ref::<MirrorStructuralBlocked>()
                .expect("MirrorStructuralBlocked")
                .forwarded
        );
        match &engine.pending_structural_forward[0].op {
            StructuralOp::ConvertSurface { cwd, .. } => {
                assert_eq!(cwd.as_deref(), Some("/tmp/proj"));
            }
            other => panic!("expected ConvertSurface, got {other:?}"),
        }
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mirror_convert_to_terminal_forwards_cwd() {
        use tasty_ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        core.apply(
            &mut engine,
            DomainIntent::ConvertSurface {
                surface_id: a,
                target: crate::core::intent::ConvertSurfaceTarget::Terminal {
                    cwd: Some(std::path::PathBuf::from("/tmp/proj")),
                },
            },
        )
        .expect_err("blocked locally");
        match &engine.pending_structural_forward[0].op {
            StructuralOp::ConvertSurface {
                cwd, surface_kind, ..
            } => {
                assert_eq!(surface_kind, "terminal");
                assert_eq!(cwd.as_deref(), Some("/tmp/proj"));
            }
            other => panic!("expected ConvertSurface, got {other:?}"),
        }
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mirror_move_surface_enqueues_forward_when_same_workspace() {
        use tasty_ipc::stream::StructuralOp;
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        let events = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect("split ok");
        let b = match events.into_iter().next() {
            Some(CoreEvent::SurfaceSplit { new_surface_id, .. }) => new_surface_id,
            other => panic!("expected SurfaceSplit, got {other:?}"),
        };
        engine.workspaces[0].mirror = true;

        let err = core
            .apply(
                &mut engine,
                DomainIntent::MoveSurface {
                    source_surface_id: a,
                    target_surface_id: b,
                },
            )
            .expect_err("blocked locally");
        let blocked = err
            .downcast_ref::<MirrorStructuralBlocked>()
            .expect("MirrorStructuralBlocked");
        assert!(
            blocked.forwarded,
            "같은 mirror workspace 안의 move 는 forward 돼야 한다"
        );
        assert_eq!(engine.pending_structural_forward.len(), 1);
        match &engine.pending_structural_forward[0].op {
            StructuralOp::MoveSurface {
                source_surface_id,
                target_surface_id,
            } => {
                assert_eq!(*source_surface_id, a);
                assert_eq!(*target_surface_id, b);
            }
            other => panic!("expected MoveSurface, got {other:?}"),
        }
    }

    #[test]
    fn mirror_move_surface_blocked_when_crossing_workspace_boundary() {
        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;

        let ws1_id = engine.next_ids.next_workspace();
        let pane1_id = engine.next_ids.next_pane();
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        engine
            .terminals
            .insert(sid1, Terminal::new_detached(80, 24));
        let ws1 = crate::model::Workspace::new_with_terminal_marker(
            ws1_id,
            "ws1".to_string(),
            pane1_id,
            tab1_id,
            sid1,
        );
        engine.workspaces.push(ws1);

        let err = core
            .apply(
                &mut engine,
                DomainIntent::MoveSurface {
                    source_surface_id: a,
                    target_surface_id: sid1,
                },
            )
            .expect_err("blocked locally");
        let blocked = err
            .downcast_ref::<MirrorStructuralBlocked>()
            .expect("MirrorStructuralBlocked");
        assert!(
            !blocked.forwarded,
            "workspace 경계를 넘는 move 는 forward 하면 안 된다"
        );
        assert!(engine.pending_structural_forward.is_empty());
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mark_last_forward_user_triggered_flips_on_user_origin() {
        use crate::intent::{IntentOrigin, UserSource};

        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let err = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect_err("blocked");
        assert!(!engine.pending_structural_forward[0].user_triggered);

        mark_last_forward_user_triggered(
            &mut engine,
            &err,
            &IntentOrigin::User {
                source: UserSource::Shortcut("split_surface_horizontal"),
            },
        );
        assert!(
            engine.pending_structural_forward[0].user_triggered,
            "사용자 origin의 forward 요청은 user_triggered로 표시돼야 한다"
        );
    }

    #[cfg(feature = "gui")]
    #[test]
    fn mark_last_forward_user_triggered_stays_false_on_agent_origin() {
        use crate::intent::IntentOrigin;

        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;
        let err = core
            .apply(
                &mut engine,
                DomainIntent::SplitSurface {
                    target_surface_id: a,
                    direction: SplitDirection::Horizontal,
                    cwd: None,
                    kind: "terminal".to_string(),
                    surface_params: serde_json::json!({}),
                },
            )
            .expect_err("blocked");

        mark_last_forward_user_triggered(
            &mut engine,
            &err,
            &IntentOrigin::Agent {
                source: crate::intent::AgentSource::Ipc,
            },
        );
        assert!(
            !engine.pending_structural_forward[0].user_triggered,
            "에이전트 origin을 user_triggered로 표시하면 안 된다"
        );
    }

    #[test]
    fn mark_last_forward_user_triggered_noop_when_not_forwarded() {
        use crate::intent::{IntentOrigin, UserSource};

        let (mut core, mut engine) = build_test_core();
        let (a, _pane) = seed(&mut engine);
        engine.workspaces[0].mirror = true;

        let ws1_id = engine.next_ids.next_workspace();
        let pane1_id = engine.next_ids.next_pane();
        let tab1_id = engine.next_ids.next_tab();
        let sid1 = engine.next_ids.next_surface();
        engine
            .terminals
            .insert(sid1, Terminal::new_detached(80, 24));
        let ws1 = crate::model::Workspace::new_with_terminal_marker(
            ws1_id,
            "ws1".to_string(),
            pane1_id,
            tab1_id,
            sid1,
        );
        engine.workspaces.push(ws1);

        let err = core
            .apply(
                &mut engine,
                DomainIntent::MoveSurface {
                    source_surface_id: a,
                    target_surface_id: sid1,
                },
            )
            .expect_err("blocked");

        mark_last_forward_user_triggered(
            &mut engine,
            &err,
            &IntentOrigin::User {
                source: UserSource::Shortcut("x"),
            },
        );
        assert!(engine.pending_structural_forward.is_empty());
    }
}
