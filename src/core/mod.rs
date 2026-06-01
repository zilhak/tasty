//! `Core` — 도메인 본체 + 단일 mutate 진입점 + 외부 자원 (port) 주입.
//!
//! ```text
//! handler   ──read──>   &CoreState        (외부 read-only 노출)
//! handler   ──enqueue─> Intent            (HandlerCtx.intents)
//! dispatch  ──drain──>  Core::apply(...)  (Core 만이 mutate)
//! Core::apply ──mutate self.state          (도메인 일관성 보장)
//! ```
//!
//! Phase D 진행 중. 본 Core 는 11 outbound port + preset_store 직속 보유만.
//! 도메인 데이터 (`CoreState`) 는 `crate::engine_state` 에 — App.engine_state
//! 가 main owner. D.3.C 의 도메인 마이그레이션으로 점진 흡수 예정.

pub(crate) mod builder;
pub(crate) mod intent;
pub(crate) mod restore_rebuild;

use std::sync::{Arc, Mutex};

use intent::{CoreEvent, DomainIntent};
use tasty_memory::MemoryStorage;
use tasty_presets::{PresetStorage, PresetStore};
use tasty_settings::SettingsStorage;
use tasty_themes::ThemeStorage;

use crate::ports::clipboard::ClipboardSystem;
use crate::ports::clock::Clock;
use crate::ports::fs::FileSystem;
use crate::ports::home::HomeDirectory;
use crate::ports::process::ProcessSpawner;
use crate::ports::pty::{PtyService, TerminalWaker};

/// 도메인 본체. 11 outbound port (7 external + 4 internal) + preset_store 직속.
///
/// 도메인 데이터 (`crate::engine_state::CoreState`) 는 본 struct 가 아닌
/// `App.engine_state` 가 main owner — Phase D 진행 중의 *공존 layer*. D.3.C
/// 에서 점진 흡수.
#[allow(dead_code)]
pub(crate) struct Core {
    // ─── External ports (bin 안 정의, src/ports/) ───
    pty: Arc<dyn PtyService>,
    waker: Arc<dyn TerminalWaker>,
    fs: Arc<dyn FileSystem>,
    clock: Arc<dyn Clock>,
    clipboard: Arc<dyn ClipboardSystem>,
    process: Arc<dyn ProcessSpawner>,
    home: Arc<dyn HomeDirectory>,

    // ─── Internal crate trait ports ───
    /// `Sync` 아님 (SQLite Connection 의 `RefCell` 캐시) — `Mutex` 보호.
    memory: Arc<Mutex<dyn MemoryStorage>>,
    themes: Arc<dyn ThemeStorage>,
    /// `&mut self` 메서드 (save/delete/rename) 가 있어 `Mutex` 보호.
    /// `preset_store` 와 *같은 allocation* (coerce 된 trait Arc).
    presets: Arc<Mutex<dyn PresetStorage>>,
    settings_storage: Arc<dyn SettingsStorage>,

    /// Layout preset 디스크 캐시. 구체 Arc — MainWindow / PresetWindow 에 clone
    /// 으로 전달해 *공유 owner* 가 된다. `presets` (trait Arc) 와 같은 allocation.
    pub(crate) preset_store: Arc<Mutex<PresetStore>>,
}

impl Core {
    // ─── Domain wrapper methods ───
    //
    // sync 결과 반환이 필요한 mutate 는 본 wrapper 들로 노출 (handler 가 직접
    // 호출). Phase D 진행 중에는 wrapper 가 `engine` 도 함께 받아 그쪽을 mutate
    // 한다 — 도메인 데이터의 Core 흡수가 완료되면 `engine` 인자 제거 예정.

    /// Surface message 전송. 옛 `engine.send_message` 의 Core 진입점.
    pub(crate) fn send_surface_message(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        from: u32,
        to: u32,
        content: String,
    ) -> u32 {
        engine.send_message(from, to, content)
    }

    /// Surface message 큐 read (peek/consume). 옛 `engine.read_messages` 의 Core 진입점.
    pub(crate) fn read_surface_messages(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        sid: u32,
        from: Option<u32>,
        peek: bool,
    ) -> Vec<crate::state::SurfaceMessage> {
        engine.read_messages(sid, from, peek)
    }

    /// Surface message 큐 clear. 옛 `engine.clear_messages` 의 Core 진입점.
    pub(crate) fn clear_surface_messages(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        sid: u32,
    ) {
        engine.clear_messages(sid);
    }

    // ─── Output observers (D.3.C.E.5) ───

    /// Observer 등록. 반환: 새 observer id.
    pub(crate) fn observer_register(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        spec: crate::output_observer::ObserverSpec,
    ) -> Result<u64, crate::output_observer::ObserverError> {
        let memory = engine.memory.clone();
        engine.observer_router.register(spec, memory)
    }

    /// Observer 해제.
    pub(crate) fn observer_unregister(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        observer_id: u64,
    ) -> Result<(), crate::output_observer::ObserverError> {
        engine.observer_router.unregister(observer_id)
    }

    /// Observer 목록 — read 인터페이스.
    pub(crate) fn observer_list(
        &self,
        engine: &crate::engine_state::CoreState,
    ) -> Vec<crate::output_observer::ObserverInfo> {
        engine.observer_router.list()
    }

    /// 특정 observer 의 info — read 인터페이스.
    pub(crate) fn observer_info(
        &self,
        engine: &crate::engine_state::CoreState,
        observer_id: u64,
    ) -> Option<crate::output_observer::ObserverInfo> {
        engine.observer_router.info(observer_id)
    }

    // ─── Hooks ───

    /// surface hook 등록. 반환: 새 hook id.
    pub(crate) fn register_surface_hook(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        surface_id: u32,
        event: tasty_hooks::HookEvent,
        command: String,
        once: bool,
    ) -> u64 {
        engine
            .hook_manager
            .add_hook(surface_id, event, command, once)
    }

    /// surface hook 해제. 반환: 실제 제거 여부.
    pub(crate) fn unregister_surface_hook(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        hook_id: u64,
    ) -> bool {
        engine.hook_manager.remove_hook(hook_id)
    }

    /// global hook 등록. 반환: 새 hook id.
    pub(crate) fn register_global_hook(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        condition: crate::global_hooks::HookCondition,
        command: String,
        label: Option<String>,
    ) -> u32 {
        engine.global_hook_manager.add(condition, command, label)
    }

    /// global hook 해제. 반환: 실제 제거 여부.
    pub(crate) fn unregister_global_hook(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        hook_id: u32,
    ) -> bool {
        engine.global_hook_manager.remove(hook_id)
    }

    /// surface 의 hook 들 중 event 매칭 시 fire — 발사된 hook id 들 반환.
    /// AppState 의 enqueue_host_event 는 호출처 (handler) 에서 처리.
    pub(crate) fn fire_surface_hooks(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        surface_id: u32,
        events: &[tasty_hooks::HookEvent],
    ) -> Vec<u64> {
        engine.hook_manager.check_and_fire(surface_id, events)
    }

    // ─── Approval (휴먼 핸드오프) ───

    /// approval 요청 생성. 옛 `engine.approval_store.request` 의 Core 진입점.
    pub(crate) fn request_approval(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        req: tasty_approval::ApprovalRequest,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.request(req)
    }

    /// approval 응답 적용. 옛 `engine.approval_store.respond` 의 Core 진입점.
    pub(crate) fn respond_approval(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        req_id: &tasty_approval::ApprovalId,
        choice: String,
        by: tasty_approval::Responder,
        comment: Option<String>,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.respond(req_id, choice, by, comment)
    }

    /// approval 취소. 옛 `engine.approval_store.cancel` 의 Core 진입점.
    pub(crate) fn cancel_approval(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        req_id: &tasty_approval::ApprovalId,
    ) -> Result<tasty_approval::StateChange, tasty_approval::ApprovalError> {
        engine.approval_store.cancel(req_id)
    }

    // ─── Memory (영속 store) ───

    /// Memory port 의 Arc clone. AppState 등 *Core 인자가 cascade 로 도달하지
    /// 못하는 표면* (UI popup draw_fn, state cleanup 등) 에 inject 해 동일
    /// allocation 의 port 를 공유시킨다.
    pub(crate) fn memory_arc(
        &self,
    ) -> std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>> {
        self.memory.clone()
    }

    /// Memory store 의 lock 안에서 함수를 실행한다. Mutex poisoning 시
    /// poison 해제 후 inner 사용 (host 부팅이 store 의 Arc 를 항상 inject
    /// 하므로 None 반환 분기는 없다 — 호출처는 `Result<R, _>` 만 처리하면 된다).
    pub(crate) fn with_memory<R>(
        &self,
        f: impl FnOnce(&mut dyn tasty_memory::MemoryStorage) -> R,
    ) -> R {
        let mut guard = match self.memory.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        f(&mut *guard)
    }

    // ─── Layout persistence (~/.tasty/layout.json) ───

    /// 현재 layout 을 디스크에 저장한다. debounce / shutdown flush 의 진입점.
    /// 호출자는 (engine, active_workspace_index) 를 넘긴다.
    pub(crate) fn save_layout(
        &self,
        engine: &mut crate::engine_state::CoreState,
        active_workspace: usize,
    ) {
        crate::engine::layout_persistence::save_to_disk(engine, active_workspace);
    }

    /// 저장된 layout 을 live engine 으로 복원한다. plugin 이 register 한 surface
    /// kind 가 준비된 후에만 호출해야 한다 — engine boot 시점이 아닌, 첫 plugin
    /// pump 이후 호출.
    pub(crate) fn restore_layout(
        &self,
        engine: &mut crate::engine_state::CoreState,
        saved: crate::engine::layout_persistence::SavedLayout,
    ) -> bool {
        saved.restore(engine)
    }

    // ─── Clipboard (외부 시스템 clipboard) ───

    /// 시스템 clipboard 에 text 쓰기. 옛 `arboard::Clipboard::new().set_text` 의 Core 진입점.
    pub(crate) fn clipboard_write_text(&self, text: &str) -> anyhow::Result<()> {
        self.clipboard.write_text(text)
    }

    /// 시스템 clipboard 에 image 쓰기. 옛 `arboard::Clipboard::new().set_image` 의 Core 진입점.
    pub(crate) fn clipboard_write_image(
        &self,
        image: &crate::ports::clipboard::ClipboardImage,
    ) -> anyhow::Result<()> {
        self.clipboard.write_image(image)
    }

    /// 도메인 변경의 단일 진입점. handler 가 발행한 `DomainIntent` 를 받아
    /// 결과 이벤트 목록을 반환. Phase D 진행 중 — variant 추가 시 본 match 도 채움.
    ///
    /// `engine` 인자: 발화 대상 engine. 현재 *이벤트만 발행* 패턴인 variant
    /// 들은 인자를 사용하지 않으나 (점진적 흡수 진행 중), workspace.create
    /// 처럼 *결과 정보가 필요한* variant 는 본 메서드 안에서 직접 mutate 후
    /// event 에 결과를 담아 반환한다. CreateWorkspace 분기만 engine 을
    /// 사용하므로 rustc 는 unused 경고를 내지 않는다.
    #[allow(dead_code)]
    pub(crate) fn apply(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        intent: DomainIntent,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        match intent {
            // Phase D 진행 중 — 본 stub 들은 *이벤트만 발행*. cascade
            // (Theme apply / Scrollback limit / clipboard max / notification
            // coalesce 등) 는 후속 sub-step (호출처 전환과 함께) 에서 통합.
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
            DomainIntent::MarkNotificationRead { id } => {
                Ok(vec![CoreEvent::NotificationReadRequested { id }])
            }
            DomainIntent::MarkAllNotificationsRead => {
                Ok(vec![CoreEvent::AllNotificationsReadRequested])
            }
            DomainIntent::SurfaceCwdChanged { surface_id } => {
                Ok(vec![CoreEvent::SurfaceCwdChanged { surface_id }])
            }
            DomainIntent::SetTerminalMark { surface_id } => {
                Ok(vec![CoreEvent::TerminalMarkSet { surface_id }])
            }
            DomainIntent::RecordInternalClipboardCopy { text } => {
                Ok(vec![CoreEvent::InternalClipboardCopyRecorded { text }])
            }
            DomainIntent::CreateWorkspace {
                cwd,
                kind,
                surface_params,
                name,
                subtitle,
                description,
            } => self.apply_create_workspace(
                engine,
                cwd,
                kind,
                surface_params,
                name,
                subtitle,
                description,
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
                surface_params,
            } => Self::apply_create_tab(engine, pane_id, cwd, kind, surface_params),
            DomainIntent::CloseTab { tab_id } => Ok(vec![Self::apply_close_tab(engine, tab_id)]),
            DomainIntent::MoveTab {
                pane_id,
                from_index,
                to_index,
            } => Ok(vec![Self::apply_move_tab(
                engine, pane_id, from_index, to_index,
            )]),
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
            DomainIntent::SendToSurface {
                surface_id,
                payload,
            } => Ok(vec![Self::apply_send_to_surface(
                engine, surface_id, payload,
            )]),
            DomainIntent::RespawnTerminal { surface_id, cwd } => {
                Ok(vec![Self::apply_respawn_terminal(engine, surface_id, cwd)])
            }
            DomainIntent::RestoreClosedItem { target_pane_id } => {
                Ok(vec![Self::apply_restore_closed_item(
                    engine,
                    target_pane_id,
                )])
            }
        }
    }

    /// `DomainIntent::RestoreClosedItem` 본문. closed_items stack pop → kind 별
    /// rebuild + engine attach. AppState 의존 부분 (active_workspace 변경) 은
    /// cascade 가 처리하므로 본 함수는 *engine mutate* 만.
    fn apply_restore_closed_item(
        engine: &mut crate::engine_state::CoreState,
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
                RestoredKind::TabIntoPane { pane_id }
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
                RestoredKind::TabIntoPane { pane_id }
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
    fn apply_respawn_terminal(
        engine: &mut crate::engine_state::CoreState,
        surface_id: u32,
        cwd: Option<std::path::PathBuf>,
    ) -> CoreEvent {
        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(surface_id);
        let new_terminal = match tasty_terminal::Terminal::new(
            tasty_terminal::TerminalConfig {
                cols,
                rows,
                shell: sh.shell_ref(),
                args: &sh.args_ref(),
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

    /// `DomainIntent::SendToSurface` 본문. ensure_surface_initialized → terminal
    /// lookup → send_bytes / send_key 분기.
    fn apply_send_to_surface(
        engine: &mut crate::engine_state::CoreState,
        surface_id: u32,
        payload: crate::core::intent::SendPayload,
    ) -> CoreEvent {
        engine.ensure_surface_initialized(surface_id);
        let sent = if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
            match payload {
                crate::core::intent::SendPayload::Bytes(bytes) => {
                    terminal.send_bytes(&bytes);
                }
                crate::core::intent::SendPayload::Text(text) => {
                    terminal.send_key(&text);
                }
            }
            true
        } else {
            false
        };
        CoreEvent::SurfaceSent { surface_id, sent }
    }

    /// `DomainIntent::SplitPane` 본문. 4-phase borrow 분리.
    fn apply_split_pane(
        engine: &mut crate::engine_state::CoreState,
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
        let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(new_surface_id);

        // Phase 2: 새 pane 구성
        let new_pane = if is_terminal {
            crate::model::Pane::new_with_shell(
                new_pane_id,
                new_tab_id,
                new_surface_id,
                crate::model::ShellSpawnOpts {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    waker,
                    working_dir: cwd.as_deref(),
                },
            )?
        } else {
            let surface =
                engine.create_surface_via_registry(&kind, new_surface_id, &surface_params)?;
            let name = crate::state::pane::default_tab_name_for_kind(&kind, &surface_params);
            crate::model::Pane::new_with_surface(new_pane_id, new_tab_id, name, surface)
        };

        // Phase 3: workspace pane tree mutate
        engine.workspaces[ws_idx]
            .pane_layout_mut()
            .split_pane_in_place(target_pane_id, direction, new_pane);

        // Phase 4: engine mutate (pane borrow 끝)
        engine.send_fast_init(new_surface_id);
        engine.mark_layout_dirty();

        Ok(vec![CoreEvent::PaneSplit {
            workspace_index: ws_idx,
            original_pane_id: target_pane_id,
            new_pane_id,
            new_surface_id,
            direction,
        }])
    }

    /// `DomainIntent::SplitSurface` 본문. tab 안에서 surface 추가 (pane tree 변경 X).
    fn apply_split_surface(
        engine: &mut crate::engine_state::CoreState,
        target_surface_id: u32,
        direction: crate::model::SplitDirection,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        surface_params: serde_json::Value,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let new_surface_id = engine.next_ids.next_surface();
        let is_terminal = kind == "terminal";

        // Phase 1: 새 surface 생성. terminal 은 직접 Box::new, 그 외는 registry.
        let new_surface: Box<dyn crate::model::Surface> = if is_terminal {
            let cols = engine.default_cols;
            let rows = engine.default_rows;
            let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
            let waker = engine.make_waker(new_surface_id);
            let terminal = tasty_terminal::Terminal::new(
                tasty_terminal::TerminalConfig {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    args: &sh.args_ref(),
                    surface_id: new_surface_id,
                    working_dir: cwd.as_deref(),
                    initial_input: None,
                },
                waker,
            )?;
            Box::new(crate::model::TerminalSurface {
                id: new_surface_id,
                terminal,
                deferred_spawn: None,
                scrollback_persist_id: None,
            })
        } else {
            engine.create_surface_via_registry(&kind, new_surface_id, &surface_params)?
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

        Ok(vec![CoreEvent::SurfaceSplit {
            workspace_index: ws_idx,
            pane_id,
            target_surface_id,
            new_surface_id,
        }])
    }

    /// `DomainIntent::ClosePane` 본문. pane_id 로 모든 workspace 순회.
    /// cleanup_targets 수집 → pane tree close → workspace 안 focused_pane 보정
    /// (닫힌 곳의 자연 이동, 원칙 위반 아님). cleanup_surface 는 cascade.
    fn apply_close_pane(engine: &mut crate::engine_state::CoreState, pane_id: u32) -> CoreEvent {
        let ws_idx = match engine.find_workspace_index_for_pane(pane_id) {
            Some(idx) => idx,
            None => {
                return CoreEvent::PaneClosed {
                    pane_id,
                    closed: false,
                    cleanup_targets: vec![],
                };
            }
        };

        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        if let Some(pane) = engine.workspaces[ws_idx].pane_layout().find_pane(pane_id) {
            for tab in &pane.tabs {
                crate::state::AppState::collect_close_targets(tab, &mut targets);
            }
        }

        let ws = &mut engine.workspaces[ws_idx];
        let was_focused = ws.focused_pane == pane_id;
        let removed = ws.pane_layout_mut().close_pane(pane_id);
        if removed {
            if was_focused {
                if let Some(first) = ws.pane_layout().first_pane() {
                    ws.focused_pane = first.id;
                }
            }
            engine.mark_layout_dirty();
        }
        CoreEvent::PaneClosed {
            pane_id,
            closed: removed,
            cleanup_targets: if removed { targets } else { vec![] },
        }
    }

    /// `DomainIntent::ConvertSurface` 본문. tab 안 split leaf 만 교체 / sole
    /// surface tab 전체 교체. 옛 `replace_surface_for_id` + 4 variant 의
    /// surface 생성 로직 흡수.
    fn apply_convert_surface(
        engine: &mut crate::engine_state::CoreState,
        surface_id: u32,
        target: crate::core::intent::ConvertSurfaceTarget,
    ) -> CoreEvent {
        use crate::core::intent::ConvertSurfaceTarget;

        let is_terminal = matches!(target, ConvertSurfaceTarget::Terminal { .. });

        // Phase 1: 새 surface 생성 (실패 가능)
        let (new_surface, new_name): (Box<dyn crate::model::Surface>, Option<Option<String>>) =
            match target {
                ConvertSurfaceTarget::Terminal { cwd } => {
                    let cols = engine.default_cols;
                    let rows = engine.default_rows;
                    let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
                    let waker = engine.make_waker(surface_id);
                    let terminal = match tasty_terminal::Terminal::new(
                        tasty_terminal::TerminalConfig {
                            cols,
                            rows,
                            shell: sh.shell_ref(),
                            args: &sh.args_ref(),
                            surface_id,
                            working_dir: cwd.as_deref(),
                            initial_input: None,
                        },
                        waker,
                    ) {
                        Ok(t) => t,
                        Err(_) => {
                            return CoreEvent::SurfaceConverted {
                                surface_id,
                                replaced: false,
                                is_terminal,
                            };
                        }
                    };
                    let node = crate::model::TerminalSurface {
                        id: surface_id,
                        terminal,
                        deferred_spawn: None,
                        scrollback_persist_id: None,
                    };
                    // Terminal 변환은 explicit_name 클리어 (auto-derived from CWD).
                    (Box::new(node), Some(None))
                }
                ConvertSurfaceTarget::Markdown { file_path } => {
                    let surface = Box::new(crate::model::MarkdownPanel::new(
                        surface_id,
                        file_path.clone(),
                    ));
                    let name = std::path::Path::new(&file_path)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string());
                    (surface, Some(name))
                }
                ConvertSurfaceTarget::Image => {
                    let surface = Box::new(crate::model::ImagePanel::new_blank(surface_id));
                    (surface, Some(Some("Image".to_string())))
                }
                ConvertSurfaceTarget::Kind { kind, params } => {
                    let new_surface =
                        match engine.create_surface_via_registry(&kind, surface_id, &params) {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::warn!("ConvertSurface kind='{}' failed: {}", kind, e);
                                return CoreEvent::SurfaceConverted {
                                    surface_id,
                                    replaced: false,
                                    is_terminal,
                                };
                            }
                        };
                    // 변환 시 explicit_name 은 클리어 (surface 자체의 display_name).
                    (new_surface, Some(None))
                }
            };

        // Phase 2: location 찾기 (workspace index, pane id, tab index)
        let mut location: Option<(usize, u32, usize)> = None;
        'outer: for (ws_idx, workspace) in engine.workspaces.iter().enumerate() {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    for (tab_idx, tab) in pane.tabs.iter().enumerate() {
                        if tab.contains_surface(surface_id) {
                            location = Some((ws_idx, pid, tab_idx));
                            break 'outer;
                        }
                    }
                }
            }
        }
        let (ws_idx, pane_id, tab_idx) = match location {
            Some(loc) => loc,
            None => {
                return CoreEvent::SurfaceConverted {
                    surface_id,
                    replaced: false,
                    is_terminal,
                };
            }
        };

        // Phase 3: replace
        let replaced = {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = match ws.pane_layout_mut().find_pane_mut(pane_id) {
                Some(p) => p,
                None => {
                    return CoreEvent::SurfaceConverted {
                        surface_id,
                        replaced: false,
                        is_terminal,
                    };
                }
            };
            let tab = &mut pane.tabs[tab_idx];
            if tab.is_split() {
                // Tab has split layout — replace just the leaf. Tab name 은 변경 X.
                tab.layout_mut().replace_surface(surface_id, new_surface)
            } else {
                // Tab's sole surface — replace whole tab surface.
                tab.put_surface(new_surface);
                if let Some(name_opt) = new_name {
                    tab.explicit_name = name_opt;
                }
                true
            }
        };

        // Phase 4: engine mutate (pane borrow 끝)
        if replaced {
            engine.mark_layout_dirty();
            if is_terminal {
                engine.send_fast_init(surface_id);
            }
        }

        CoreEvent::SurfaceConverted {
            surface_id,
            replaced,
            is_terminal,
        }
    }

    /// `DomainIntent::CloseSurface` 본문. cascading close — surface→tab→pane→
    /// workspace 단계까지 자동 cascade. 옛 `close_surface_by_id_inner` 의 4-case
    /// 코드 이동. cleanup_surface / memory purge / active_workspace 보정 /
    /// auto-recreate 는 cascade + caller 책임.
    fn apply_close_surface(
        engine: &mut crate::engine_state::CoreState,
        surface_id: u32,
        save_snapshot: bool,
    ) -> CoreEvent {
        use crate::core::intent::CascadeLevel;

        let not_found = || CoreEvent::SurfaceClosed {
            surface_id,
            closed: false,
            cascade_level: CascadeLevel::Surface,
            cleanup_targets: vec![],
            workspace_id_purged: None,
            workspaces_now_empty: false,
        };

        let (ws_idx, pane_id) = match engine.find_workspace_index_for_surface(surface_id) {
            Some(v) => v,
            None => return not_found(),
        };

        // Step 1: tab_idx + sole/split 판정
        let tab_idx;
        let surface_is_sole_in_tab;
        let can_close_surface_in_group;
        {
            let ws = &mut engine.workspaces[ws_idx];
            let pane = match ws.pane_layout_mut().find_pane_mut(pane_id) {
                Some(p) => p,
                None => return not_found(),
            };
            let mut found_tab = None;
            for (i, tab) in pane.tabs.iter().enumerate() {
                if tab.contains_surface(surface_id) {
                    found_tab = Some(i);
                    break;
                }
            }
            tab_idx = match found_tab {
                Some(i) => i,
                None => return not_found(),
            };
            let tab = &pane.tabs[tab_idx];
            if tab.is_split() {
                surface_is_sole_in_tab = false;
                can_close_surface_in_group =
                    !matches!(tab.layout(), crate::model::SurfaceLayout::Leaf(_));
            } else if tab.contains_surface(surface_id) {
                surface_is_sole_in_tab = true;
                can_close_surface_in_group = false;
            } else {
                return not_found();
            }
        }

        // Case 1: split tab 안 surface 다중 close
        if !surface_is_sole_in_tab && can_close_surface_in_group {
            if save_snapshot {
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                let tab = &pane.tabs[tab_idx];
                if let Some(node) = tab.find_terminal_surface(surface_id) {
                    let snapshot =
                        crate::model::closed_item::ClosedSurface::from_surface_node(node);
                    engine.push_closed_item(crate::model::ClosedItem::Surface {
                        surface: snapshot,
                        tab_name: tab.display_name().to_string(),
                    });
                }
            }
            let persist_id = {
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                let tab = &pane.tabs[tab_idx];
                tab.find_terminal_surface(surface_id)
                    .and_then(|ts| ts.scrollback_persist_id.clone())
            };
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            let tab = &mut pane.tabs[tab_idx];
            if tab.close_surface(surface_id) {
                engine.mark_layout_dirty();
                return CoreEvent::SurfaceClosed {
                    surface_id,
                    closed: true,
                    cascade_level: CascadeLevel::Surface,
                    cleanup_targets: vec![(surface_id, persist_id)],
                    workspace_id_purged: None,
                    workspaces_now_empty: false,
                };
            }
            return not_found();
        }

        // Case 2: sole surface tab, pane.tabs.len() > 1 — tab close
        {
            if save_snapshot {
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                if pane.tabs.len() > 1 {
                    let snapshot_opt = {
                        let mut snap_fn = crate::engine::surface_registry::snapshot_fn_for(
                            &engine.surface_registry,
                        );
                        crate::model::closed_item::ClosedTab::from_tab(
                            &pane.tabs[tab_idx],
                            &mut snap_fn,
                        )
                    };
                    if let Some(snapshot) = snapshot_opt {
                        engine.push_closed_item(crate::model::ClosedItem::Tab(snapshot));
                    }
                }
            }
            let mut targets: Vec<(u32, Option<String>)> = Vec::new();
            {
                let ws = &engine.workspaces[ws_idx];
                let pane = ws.pane_layout().find_pane(pane_id).unwrap();
                if pane.tabs.len() > 1 {
                    crate::state::AppState::collect_close_targets(
                        &pane.tabs[tab_idx],
                        &mut targets,
                    );
                }
            }
            let ws = &mut engine.workspaces[ws_idx];
            let pane = ws.pane_layout_mut().find_pane_mut(pane_id).unwrap();
            if pane.tabs.len() > 1 {
                pane.tabs.remove(tab_idx);
                if pane.active_tab >= pane.tabs.len() {
                    pane.active_tab = pane.tabs.len() - 1;
                }
                engine.mark_layout_dirty();
                return CoreEvent::SurfaceClosed {
                    surface_id,
                    closed: true,
                    cascade_level: CascadeLevel::Tab,
                    cleanup_targets: targets,
                    workspace_id_purged: None,
                    workspaces_now_empty: false,
                };
            }
        }

        // Case 3: last tab in pane, ws 안 pane >1 — pane close
        {
            let mut targets: Vec<(u32, Option<String>)> = Vec::new();
            {
                let ws = &engine.workspaces[ws_idx];
                if ws.pane_layout().all_pane_ids().len() > 1 {
                    if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                        for tab in &pane.tabs {
                            crate::state::AppState::collect_close_targets(tab, &mut targets);
                        }
                    }
                }
            }
            let ws = &mut engine.workspaces[ws_idx];
            if ws.pane_layout().all_pane_ids().len() > 1 {
                ws.pane_layout_mut().close_pane(pane_id);
                if let Some(first) = ws.pane_layout().first_pane() {
                    ws.focused_pane = first.id;
                }
                engine.mark_layout_dirty();
                return CoreEvent::SurfaceClosed {
                    surface_id,
                    closed: true,
                    cascade_level: CascadeLevel::Pane,
                    cleanup_targets: targets,
                    workspace_id_purged: None,
                    workspaces_now_empty: false,
                };
            }
        }

        // Case 4: last pane in workspace — workspace close
        if save_snapshot {
            let item = {
                let mut snap_fn =
                    crate::engine::surface_registry::snapshot_fn_for(&engine.surface_registry);
                let ws = &engine.workspaces[ws_idx];
                crate::model::ClosedItem::from_workspace(ws, &mut snap_fn)
            };
            engine.push_closed_item(item);
        }
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        {
            let ws = &engine.workspaces[ws_idx];
            for pid in ws.pane_layout().all_pane_ids() {
                if let Some(pane) = ws.pane_layout().find_pane(pid) {
                    for tab in &pane.tabs {
                        crate::state::AppState::collect_close_targets(tab, &mut targets);
                    }
                }
            }
        }
        let workspace_id = engine.workspaces[ws_idx].id;
        engine.workspaces.remove(ws_idx);
        let workspaces_now_empty = engine.workspaces.is_empty();
        engine.mark_layout_dirty();

        CoreEvent::SurfaceClosed {
            surface_id,
            closed: true,
            cascade_level: CascadeLevel::Workspace,
            cleanup_targets: targets,
            workspace_id_purged: Some(workspace_id),
            workspaces_now_empty,
        }
    }

    /// `DomainIntent::MoveTab` 본문. pane_id 로 모든 workspace 순회
    /// (focused 의존 없음 — 포커스 독립 원칙).
    fn apply_move_tab(
        engine: &mut crate::engine_state::CoreState,
        pane_id: u32,
        from_index: usize,
        to_index: usize,
    ) -> CoreEvent {
        let moved = engine
            .find_pane_by_id_mut(pane_id)
            .map(|p| p.move_tab(from_index, to_index))
            .unwrap_or(false);
        if moved {
            engine.mark_layout_dirty();
        }
        CoreEvent::TabMoved { pane_id, moved }
    }

    /// `DomainIntent::CloseTab` 본문. tab 위치 + cleanup_targets 수집 →
    /// pane.close_tab_by_id → mark_layout_dirty. cleanup_surface (AppState
    /// 데이터) 는 cascade 가 처리한다.
    fn apply_close_tab(engine: &mut crate::engine_state::CoreState, tab_id: u32) -> CoreEvent {
        let mut targets: Vec<(u32, Option<String>)> = Vec::new();
        let mut found_pane_id = None;
        for workspace in &engine.workspaces {
            for &pid in &workspace.pane_layout().all_pane_ids() {
                if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                    if let Some(tab) = pane.tabs.iter().find(|t| t.id == tab_id) {
                        crate::state::AppState::collect_close_targets(tab, &mut targets);
                        found_pane_id = Some(pid);
                        break;
                    }
                }
            }
            if found_pane_id.is_some() {
                break;
            }
        }

        let pane_id = match found_pane_id {
            Some(pid) => pid,
            None => {
                return CoreEvent::TabClosed {
                    tab_id,
                    closed: false,
                    cleanup_targets: vec![],
                };
            }
        };

        let closed = engine
            .find_pane_by_id_mut(pane_id)
            .map(|p| p.close_tab_by_id(tab_id))
            .unwrap_or(false);
        if closed {
            engine.mark_layout_dirty();
        }
        CoreEvent::TabClosed {
            tab_id,
            closed,
            cleanup_targets: if closed { targets } else { vec![] },
        }
    }

    /// `DomainIntent::CreateTab` 본문. borrow 분리:
    /// 1) settings / waker / surface 미리 추출 (engine 의 *불변* 의존)
    /// 2) scope block 으로 pane mutate (engine 의 가변 borrow 좁힘)
    /// 3) send_fast_init / mark_layout_dirty (pane borrow 끝난 후)
    fn apply_create_tab(
        engine: &mut crate::engine_state::CoreState,
        pane_id: u32,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        surface_params: serde_json::Value,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();
        let is_terminal = kind == "terminal";

        let cols = engine.default_cols;
        let rows = engine.default_rows;
        let lazy_init = engine.settings.performance.lazy_pty_init;
        let sh = crate::engine_state::ShellConfig::from_settings(&engine.settings);
        let waker = engine.make_waker(surface_id);

        let prepared_non_terminal = if !is_terminal {
            let surface = engine.create_surface_via_registry(&kind, surface_id, &surface_params)?;
            let name = crate::state::pane::default_tab_name_for_kind(&kind, &surface_params);
            Some((surface, name))
        } else {
            None
        };

        {
            let pane = engine
                .find_pane_by_id_mut(pane_id)
                .ok_or_else(|| anyhow::anyhow!("pane {pane_id} not found"))?;
            if is_terminal {
                let spawn = crate::model::ShellSpawnOpts {
                    cols,
                    rows,
                    shell: sh.shell_ref(),
                    shell_args: &sh.args_ref(),
                    waker,
                    working_dir: cwd.as_deref(),
                };
                if lazy_init {
                    pane.add_tab_deferred(tab_id, surface_id, spawn);
                } else {
                    pane.add_tab_background_with_shell(tab_id, surface_id, spawn)?;
                }
            } else {
                let (surface, name) = prepared_non_terminal.unwrap();
                pane.add_surface_tab(tab_id, name, surface);
            }
        }

        if is_terminal && !lazy_init {
            engine.send_fast_init(surface_id);
        }
        engine.mark_layout_dirty();

        let (tab_count, active_tab) = engine
            .find_pane_by_id(pane_id)
            .map(|p| (p.tabs.len(), p.active_tab))
            .unwrap_or((0, 0));

        Ok(vec![CoreEvent::TabCreated {
            pane_id,
            tab_id,
            surface_id,
            tab_count,
            active_tab,
        }])
    }

    /// `DomainIntent::MoveWorkspace` 본문. workspaces 벡터의 from→to 이동.
    /// active_workspace 보정은 cascade 에서 처리 (Core 는 state 모름).
    fn apply_move_workspace(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
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
    fn apply_update_workspace_meta(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
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
    fn apply_create_workspace(
        &mut self,
        engine: &mut crate::engine_state::CoreState,
        cwd: Option<std::path::PathBuf>,
        kind: String,
        surface_params: serde_json::Value,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    ) -> anyhow::Result<Vec<CoreEvent>> {
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

        let ws = if is_terminal {
            let shell = if engine.settings.general.shell.is_empty() {
                None
            } else {
                Some(engine.settings.general.shell.as_str())
            };
            let shell_args_owned = engine.settings.general.effective_shell_args();
            let shell_args: Vec<&str> = shell_args_owned.iter().map(|s| s.as_str()).collect();
            crate::model::Workspace::new_with_shell(
                ws_id,
                auto_name,
                pane_id,
                tab_id,
                surface_id,
                crate::model::ShellSpawnOpts {
                    cols: engine.default_cols,
                    rows: engine.default_rows,
                    shell,
                    shell_args: &shell_args,
                    waker: engine.make_waker(surface_id),
                    working_dir: cwd.as_deref(),
                },
            )?
        } else {
            let surface = engine.create_surface_via_registry(&kind, surface_id, &surface_params)?;
            let tab_name = crate::state::pane::default_tab_name_for_kind(&kind, &surface_params);
            let pane = crate::model::Pane::new_with_surface(pane_id, tab_id, tab_name, surface);
            crate::model::Workspace::new_with_pane(ws_id, auto_name, pane)
        };

        engine.workspaces.push(ws);
        let idx = engine.workspaces.len() - 1;

        let renamed_name = name;
        let renamed_subtitle = subtitle.map(|s| {
            engine.workspaces[idx].subtitle = s.clone();
            s
        });
        let renamed_description = description.map(|d| {
            engine.workspaces[idx].description = d.clone();
            d
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

        Ok(vec![CoreEvent::WorkspaceCreated {
            id: ws_id,
            index: idx,
            surface_id: final_surface_id,
            renamed_name,
            renamed_subtitle,
            renamed_description,
        }])
    }
}

/// `RestoreClosedItem` 의 helper. pane_id 에 tab attach + active_tab 갱신.
/// *모든* workspace 순회 (포커스 독립).
fn push_tab_to_pane(
    engine: &mut crate::engine_state::CoreState,
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
