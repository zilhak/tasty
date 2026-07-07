//! attach 런타임 결선 (attach/detach 단계 4).
//!
//! 단계 3 의 [`OccupancyRegistry`](crate::core::attach) 는 순수 lock 테이블이고,
//! 단계 2 의 mirror(`new_detached`/`feed_bytes`/output tap/input sink)는 터미널
//! 메커니즘이다. 이 모듈은 둘을 **실제 바이트 파이프로 결선**한다:
//!
//! - [`CoreState::attach_surface_for_stream`]: stream client 의 attach 요청을
//!   처리 — lock 획득 → 초기 화면 bulk 스냅샷 push → 출력 tap 을 forwarder 스레드로
//!   client 에 흘림(서버 PTY 출력 → tap → StreamHub → client mirror).
//! - [`CoreState::feed_attached_input`]: client 입력 Data 프레임을 점유 surface 의
//!   PTY 로 전달(holder 검증 후, 서버 로컬 입력 차단 우회).
//!
//! 메인루프(gui `event_handler` / headless `boot`)의 `StreamReady` arm 이
//! `pump_inbound` 가 분류한 [`PumpOutcome`](crate::adapters::production::stream_hub::PumpOutcome)
//! 를 받아 이 메서드들을 호출한다.
//!
//! 범위: surface 단위(터미널 1개). workspace 단위는 단계 6.

use std::collections::HashMap;
use std::thread;

use crate::adapters::production::stream_hub::{PushResult, StreamHub};
use crate::core::CoreState;
use crate::core::attach::{AttachClientId, AttachError};
use crate::ipc::stream::{StreamControl, StreamFrame, StreamTag, StructuralOp, encode_mux};
use crate::model::{AttachSurfaceClass, SurfaceId};

impl CoreState {
    /// stream client 의 attach 요청 처리(`stream.open` 의 `target`). 성공 시 그 client
    /// 가 surface 를 배타 점유하고, 서버는 현재 화면을 1 회 bulk 스냅샷으로 push 한 뒤
    /// 이후 PTY 출력을 forwarder 스레드로 계속 흘린다. 거부/실패 시 `attach_error`
    /// Control + Detach 로 연결을 닫는다.
    ///
    /// `hub` 는 메인루프가 보유한 StreamHub(= client sink 등록처). forwarder 스레드는
    /// 그 clone 을 들고 client 끊김(push Unknown/Disconnected) 시 자동 종료한다.
    pub fn attach_surface_for_stream(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        // 대상 검증: 실재(또는 deferred) 터미널 surface 만 점유 대상.
        if !self.terminals.contains(surface_id) && !self.is_surface_deferred(surface_id) {
            reject_attach(hub, client_id, "not_found", None);
            return;
        }

        // 배타 lock 획득(동시 attach 거부).
        match self.attach.acquire(surface_id, client_id) {
            Ok(_) => {}
            Err(AttachError::AlreadyAttached { holder }) => {
                reject_attach(hub, client_id, "already_attached", Some(holder));
                return;
            }
            Err(_) => {
                reject_attach(hub, client_id, "lock_error", None);
                return;
            }
        }

        // deferred 면 여기서 PTY spawn.
        self.ensure_surface_initialized(surface_id);

        let Some(terminal) = self.terminals.get_mut(surface_id) else {
            // 점유는 됐으나 터미널이 없다(spawn 실패) → lock 환원 + 에러.
            let _ = self.attach.release(surface_id, client_id); // best-effort release — 실패 무시
            reject_attach(hub, client_id, "spawn_failed", None);
            return;
        };

        let cols = terminal.cols();
        let rows = terminal.rows();
        // 현재 화면 bulk 스냅샷 → tap 등록(메인루프 단일소유라 그 사이 ingest 없음 →
        // 누락/중복 없음). 이후 tap 바이트가 delta.
        let snapshot = terminal.snapshot_as_vt();
        let tap_rx = terminal.add_output_tap();
        let resize_rx = terminal.add_resize_tap();

        // attach 성공 통지(client 가 cols/rows 로 mirror 생성) + 초기 스냅샷.
        let attached = serde_json::json!({
            "event": "attached",
            "surface_id": surface_id,
            "cols": cols,
            "rows": rows,
        });
        let attached_frame = StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&attached).unwrap_or_default(),
        );
        let _ = hub.push(client_id, attached_frame); // best-effort 통지 — PushResult(Result 아님) 무시: client 끊김 시 forwarder 가 정리.
        let _ = hub.push(client_id, StreamFrame::new(StreamTag::Data, snapshot)); // best-effort 스냅샷 push — client 끊김 시 무시.

        // forwarder: 서버 PTY 출력 tap → client. client 끊김 시 자동 종료(다음 출력
        // 때 terminal 의 tap 도 prune 됨, design 단계 4 §8-R4).
        let hub2 = hub.clone();
        thread::spawn(move || {
            for chunk in tap_rx {
                match hub2.push(client_id, StreamFrame::new(StreamTag::Data, chunk)) {
                    PushResult::Unknown | PushResult::Disconnected => break,
                    _ => {}
                }
            }
        });

        // resize forwarder: 원격 grid 변경 → client mirror 크기 갱신(Control frame).
        // client 끊김 시 자동 종료(bare surface_id — 이 연결은 단일 surface).
        let hub3 = hub.clone();
        thread::spawn(move || {
            for (cols, rows) in resize_rx {
                let msg = StreamControl::Resize {
                    surface_id,
                    cols,
                    rows,
                };
                let frame = StreamFrame::new(
                    StreamTag::Control,
                    serde_json::to_vec(&msg).unwrap_or_default(),
                );
                match hub3.push(client_id, frame) {
                    PushResult::Unknown | PushResult::Disconnected => break,
                    _ => {}
                }
            }
        });

        tracing::debug!("attach: surface {surface_id} -> client {client_id}");
    }

    /// client 입력 Data 프레임을 그 client 가 점유한 surface 의 PTY 로 전달한다.
    /// 서버 로컬 입력 차단(`apply_send_to_surface` 의 is_hard_occupied 거부)을 우회하는
    /// 유일한 정규 경로 — holder 검증을 거치므로 점유자만 입력할 수 있다.
    /// 반환: 라우팅 성공 여부(false = 점유 surface 없음 = 비-attach client).
    pub fn feed_attached_input(&mut self, client_id: AttachClientId, bytes: &[u8]) -> bool {
        let Some(surface_id) = self.attach.surface_held_by(client_id) else {
            return false;
        };
        if let Some(terminal) = self.terminals.get_mut(surface_id) {
            terminal.send_bytes(bytes);
            true
        } else {
            false
        }
    }

    /// workspace 단위 attach 요청 처리(단계 6, D1/D3/D4). 그 workspace 의 모든 터미널
    /// surface 를 배타 점유하고 각각 단계 4 와 동일하게 초기 스냅샷 + 출력 forwarder 를
    /// 건다. 단, 한 연결에 N 개 터미널이 실리므로 모든 Data 는 surface-prefixed
    /// (`encode_mux`)다. 비-터미널은 mirror 없이 placeholder 로만 디스크립터에 실린다.
    pub fn attach_workspace_for_stream(
        &mut self,
        workspace_id: u32,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        let Some(idx) = self.find_workspace_index_for_id(workspace_id) else {
            reject_attach(hub, client_id, "workspace_not_found", None);
            return;
        };
        let class = self.workspaces[idx].classify_attach_surfaces();
        let members: Vec<SurfaceId> = class
            .terminals
            .iter()
            .chain(class.non_terminals.iter())
            .copied()
            .collect();

        match self
            .attach
            .acquire_workspace(workspace_id, &class.terminals, &members, client_id)
        {
            Ok(_) => {}
            Err(AttachError::AlreadyAttached { holder }) => {
                reject_attach(hub, client_id, "already_attached", Some(holder));
                return;
            }
            Err(_) => {
                reject_attach(hub, client_id, "lock_error", None);
                return;
            }
        }

        // deferred 터미널은 여기서 PTY spawn(크기·스냅샷 확정).
        for &sid in &class.terminals {
            self.ensure_surface_initialized(sid);
        }

        // 트리 디스크립터(client mirror 트리 재구성 + per-surface role/cols/rows).
        let descriptor = self.build_workspace_descriptor(idx, workspace_id, &class);
        let descriptor_frame = StreamFrame::new(
            StreamTag::Control,
            serde_json::to_vec(&descriptor).unwrap_or_default(),
        );
        let _ = hub.push(client_id, descriptor_frame); // best-effort 디스크립터 push — PushResult(Result 아님) 무시: client 끊김 시 forwarder 가 정리.

        // 각 터미널: 초기 스냅샷(mux) + 출력 forwarder(mux). client 끊김 시 자동 종료.
        for &sid in &class.terminals {
            let Some(terminal) = self.terminals.get_mut(sid) else {
                continue;
            };
            let snapshot = terminal.snapshot_as_vt();
            let tap_rx = terminal.add_output_tap();
            let resize_rx = terminal.add_resize_tap();
            let snapshot_frame = StreamFrame::new(StreamTag::Data, encode_mux(sid, &snapshot));
            let _ = hub.push(client_id, snapshot_frame); // best-effort 초기 mux 스냅샷 push — PushResult 무시: client 끊김 시 forwarder 가 정리.
            let hub2 = hub.clone();
            thread::spawn(move || {
                for chunk in tap_rx {
                    match hub2.push(
                        client_id,
                        StreamFrame::new(StreamTag::Data, encode_mux(sid, &chunk)),
                    ) {
                        PushResult::Unknown | PushResult::Disconnected => break,
                        _ => {}
                    }
                }
            });

            // resize forwarder: 원격 grid 변경 → client mirror 크기 갱신. workspace
            // 모드라 Control payload 에 remote surface_id(sid)를 실어 client 가
            // remote→local 매핑으로 해당 mirror 만 갱신하게 한다.
            let hub3 = hub.clone();
            thread::spawn(move || {
                for (cols, rows) in resize_rx {
                    let msg = StreamControl::Resize {
                        surface_id: sid,
                        cols,
                        rows,
                    };
                    let frame = StreamFrame::new(
                        StreamTag::Control,
                        serde_json::to_vec(&msg).unwrap_or_default(),
                    );
                    match hub3.push(client_id, frame) {
                        PushResult::Unknown | PushResult::Disconnected => break,
                        _ => {}
                    }
                }
            });
        }

        tracing::debug!(
            "attach: workspace {workspace_id} -> client {client_id} ({} terminals, {} placeholders)",
            class.terminals.len(),
            class.non_terminals.len(),
        );
    }

    /// workspace mode client 의 입력(surface-prefixed)을 지정 remote surface 의 PTY 로.
    /// holder 가 그 workspace 를 점유 중일 때만 통과(타 workspace surface 주입 차단).
    pub fn feed_attached_workspace_input(
        &mut self,
        client_id: AttachClientId,
        remote_surface_id: u32,
        bytes: &[u8],
    ) -> bool {
        let Some(ws) = self.attach.workspace_of_surface(remote_surface_id) else {
            return false;
        };
        if self.attach.workspace_holder(ws) != Some(client_id) {
            return false;
        }
        if let Some(terminal) = self.terminals.get_mut(remote_surface_id) {
            terminal.send_bytes(bytes);
            true
        } else {
            false
        }
    }

    /// workspace attach 디스크립터: 트리(분할 비율 포함) + per-surface role/cols/rows/kind.
    fn build_workspace_descriptor(
        &self,
        idx: usize,
        workspace_id: u32,
        class: &AttachSurfaceClass,
    ) -> serde_json::Value {
        let ws = &self.workspaces[idx];
        // sid → kind (비-터미널 placeholder 라벨용).
        let mut kinds: HashMap<u32, &'static str> = HashMap::new();
        for pane_id in ws.pane_layout().all_pane_ids() {
            if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                for tab in &pane.tabs {
                    tab.for_each_surface(&mut |s| {
                        if let Some(id) = s.surface_id() {
                            kinds.insert(id, s.kind());
                        }
                    });
                }
            }
        }
        let mut surfaces = Vec::new();
        for &sid in &class.terminals {
            let (cols, rows) = self
                .terminals
                .get(sid)
                .map(|t| (t.cols(), t.rows()))
                .unwrap_or((80, 24));
            surfaces.push(serde_json::json!({
                "remote_id": sid,
                "role": "terminal",
                "cols": cols,
                "rows": rows,
            }));
        }
        for &sid in &class.non_terminals {
            surfaces.push(serde_json::json!({
                "remote_id": sid,
                "role": "placeholder",
                "kind": kinds.get(&sid).copied().unwrap_or("unknown"),
            }));
        }
        serde_json::json!({
            "event": "attached_workspace",
            "workspace_id": workspace_id,
            "name": ws.name,
            "tree": ws.to_attach_tree_json(),
            "surfaces": surfaces,
        })
    }
}

/// mirror client 가 forward 한 구조 op 를 이 인스턴스(원격 = authoritative)에서
/// 실행한다. anchor **원격 surface id** 로 pane/tab/workspace 를 resolve 한 뒤 기존
/// IPC 핸들러(split / tab.create / tab.close / tab.move / pane.close / surface.close)를
/// 그대로 재사용해 full cascade(PTY spawn·host event·cleanup)로 처리한다 — 서버측
/// 워크스페이스는 mirror 가 아니므로 `Core::apply` 의 mirror 가드에 걸리지 않고 실제로
/// 실행된다.
///
/// 반환: 성공이면 `Ok(())`, JSON-RPC 에러(예: 원격에 등록되지 않은 plugin kind →
/// `create_surface_via_registry` 가 "unknown surface kind" Err)면 `Err(reason)`. 호출자
/// (메인루프)가 이 결과를 [`StreamControl::StructuralResult`] 로 client 에 회신한다.
///
/// `ConvertSurface`/`MoveSurface` 는 재사용할 IPC 핸들러가 없어 아직 forward 대상이
/// 아니다(client 도 이 둘은 forward 하지 않고 기존 차단 유지) — 방어적으로 거부 사유를
/// 반환한다.
pub(crate) fn execute_forwarded_structural_op(
    core: &mut crate::core::Core,
    state: &mut crate::state::AppState,
    engine: &mut CoreState,
    op: &StructuralOp,
) -> Result<(), String> {
    use crate::adapters::ipc::handler::{pane, surface, tab};
    use serde_json::json;

    // 재사용 핸들러는 응답 id 를 페이로드로만 쓴다(내부 호출 → Null).
    let rid = serde_json::Value::Null;

    let resp = match op {
        StructuralOp::SplitSurface {
            surface_id,
            direction,
            surface_kind,
            params,
        } => {
            let p = structural_params(
                params,
                json!({
                    "level": "surface",
                    "direction": direction.as_ipc_str(),
                    "target_surface": surface_id,
                    "type": surface_kind,
                }),
            );
            pane::handle_split(core, state, engine, rid, &p)
        }
        StructuralOp::SplitPane {
            anchor_surface_id,
            direction,
            surface_kind,
            params,
        } => {
            // pane-level split 은 target_surface 로도 pane 을 resolve 한다(handle_split).
            let p = structural_params(
                params,
                json!({
                    "level": "pane",
                    "direction": direction.as_ipc_str(),
                    "target_surface": anchor_surface_id,
                    "type": surface_kind,
                }),
            );
            pane::handle_split(core, state, engine, rid, &p)
        }
        StructuralOp::NewTab {
            anchor_surface_id,
            surface_kind,
            params,
        } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} not found"))?;
            let p = structural_params(params, json!({ "pane_id": pane_id, "type": surface_kind }));
            tab::handle_tab_create(core, state, engine, rid, &p)
        }
        StructuralOp::CloseSurface { surface_id } => {
            let p = json!({ "surface_id": surface_id });
            surface::handle_surface_close(core, state, engine, rid, &p)
        }
        StructuralOp::CloseTab { anchor_surface_id } => {
            let tab_id = engine
                .find_tab_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} tab not found"))?;
            let p = json!({ "tab_id": tab_id });
            tab::handle_tab_close(core, state, engine, rid, &p)
        }
        StructuralOp::ClosePane { anchor_surface_id } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} pane not found"))?;
            let p = json!({ "pane_id": pane_id });
            pane::handle_pane_close(core, state, engine, rid, &p)
        }
        StructuralOp::MoveTab {
            anchor_surface_id,
            from_index,
            to_index,
        } => {
            let pane_id = engine
                .find_pane_for_surface(*anchor_surface_id)
                .ok_or_else(|| format!("anchor surface {anchor_surface_id} pane not found"))?;
            let p = json!({ "pane_id": pane_id, "from_index": from_index, "to_index": to_index });
            tab::handle_tab_move(core, state, engine, rid, &p)
        }
        StructuralOp::ConvertSurface { .. } | StructuralOp::MoveSurface { .. } => {
            return Err("op not forwardable yet".to_string());
        }
    };

    match resp.error {
        Some(err) => Err(err.message),
        None => Ok(()),
    }
}

/// forward 된 op 의 kind params(있으면)에 재사용 핸들러가 기대하는 제어 키
/// (level/direction/target_surface/type/pane_id 등)를 덮어 얹는다. `base` 가 객체가
/// 아니면 빈 객체에서 시작한다.
fn structural_params(base: &serde_json::Value, control: serde_json::Value) -> serde_json::Value {
    let mut obj = base.as_object().cloned().unwrap_or_default();
    if let Some(ctrl) = control.as_object() {
        for (k, v) in ctrl {
            obj.insert(k.clone(), v.clone());
        }
    }
    serde_json::Value::Object(obj)
}

/// attach 거부/실패 통지: `attach_error` Control + Detach 로 연결 종료 유도.
/// 어떤 engine 도 대상 surface 를 소유하지 않을 때 메인루프(gui)도 호출한다.
pub(crate) fn reject_attach(
    hub: &StreamHub,
    client_id: AttachClientId,
    reason: &str,
    holder: Option<AttachClientId>,
) {
    let msg = serde_json::json!({
        "event": "attach_error",
        "reason": reason,
        "holder": holder,
    });
    let error_frame = StreamFrame::new(
        StreamTag::Control,
        serde_json::to_vec(&msg).unwrap_or_default(),
    );
    let _ = hub.push(client_id, error_frame); // best-effort attach_error 통지 — PushResult(Result 아님) 무시: client 끊겼으면 무해.
    let _ = hub.push(client_id, StreamFrame::new(StreamTag::Detach, Vec::new())); // best-effort detach 신호 — 무시.
}

#[cfg(test)]
mod forward_exec_tests {
    //! forward 된 구조 op 실행(2단계). 서버(원격 authoritative)측 워크스페이스는
    //! mirror 가 아니므로 `execute_forwarded_structural_op` 이 기존 IPC 핸들러를 재사용해
    //! **실제로** split/new-tab 을 수행한다(로컬 PTY = 원격의 정당한 PTY). 원격에 없는
    //! kind 는 `Err(reason)` 으로 실패 회신된다.
    use super::execute_forwarded_structural_op;
    use crate::ipc::stream::{SplitAxis, StructuralOp};
    use crate::state::AppState;
    use tasty_terminal::Terminal;

    fn make_core_state() -> (
        crate::core::Core,
        AppState,
        crate::core::CoreState,
        tempfile::TempDir,
    ) {
        use std::sync::{Arc, Mutex};
        use tasty_memory::MemoryStorage;
        use tasty_themes::{ThemeStorage, ThemeStore};

        use crate::adapters::test::{
            fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
            mock_process::MockProcessSpawner, tmp_home::TmpHome,
        };
        use crate::core::builder::CoreBuilder;
        use crate::ports::notification_sound::NoopPlayer;

        let term_waker: crate::terminal::Waker = Arc::new(|| {});
        let mut engine = crate::core::CoreState::new(80, 24, term_waker).unwrap();
        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn ThemeStorage> = Arc::new(ThemeStore::new());
        let state = AppState::new(&mut engine, preset_store.clone(), memory.clone());
        let home_tmp = tempfile::tempdir().expect("test tempdir");
        let core = CoreBuilder::new()
            .with_fs(Arc::new(MemFileSystem::new()))
            .with_clock(Arc::new(FakeClock::default()))
            .with_clipboard(Arc::new(MockClipboard::default()))
            .with_process(Arc::new(MockProcessSpawner::default()))
            .with_home(Arc::new(TmpHome::new(home_tmp.path().to_path_buf())))
            .with_sound_player(Arc::new(NoopPlayer))
            .with_memory(memory)
            .with_themes(themes)
            .with_preset_store(preset_store)
            .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
            .build()
            .expect("test Core build");
        (core, state, engine, home_tmp)
    }

    /// 기본 워크스페이스 0 의 단일 surface 에 detached 터미널을 붙이고 그 surface_id 반환.
    fn seed(engine: &mut crate::core::CoreState) -> u32 {
        let a = engine.workspaces[0].all_surface_ids()[0];
        engine.terminals.insert(a, Terminal::new_detached(80, 24));
        a
    }

    /// forward 된 SplitSurface 는 (비-mirror) 서버 워크스페이스에서 실제로 실행되어
    /// 새 터미널이 insert 된다(=원격이 새 PTY spawn). Ok 회신.
    #[test]
    fn forward_split_surface_executes_and_spawns() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let before = engine.terminals.iter().count();
        let op = StructuralOp::SplitSurface {
            surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let r = execute_forwarded_structural_op(&mut core, &mut state, &mut engine, &op);
        assert!(r.is_ok(), "expected Ok, got {r:?}");
        assert_eq!(
            engine.terminals.iter().count(),
            before + 1,
            "forward split 은 서버에서 새 터미널을 spawn 해야 한다"
        );
    }

    /// forward 된 NewTab 도 실제 실행(pane 은 anchor surface 로 resolve). Ok + 터미널 +1.
    #[test]
    fn forward_new_tab_executes() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let before = engine.terminals.iter().count();
        let op = StructuralOp::NewTab {
            anchor_surface_id: a,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let r = execute_forwarded_structural_op(&mut core, &mut state, &mut engine, &op);
        assert!(r.is_ok(), "expected Ok, got {r:?}");
        assert_eq!(engine.terminals.iter().count(), before + 1);
    }

    /// 원격에 등록되지 않은 kind(예: plugin markdown 부재)는 Err(reason) — client 가
    /// 실패 toast 를 띄우고 어느 쪽도 구조를 바꾸지 않는다.
    #[test]
    fn forward_unknown_kind_fails() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let before = engine.terminals.iter().count();
        let op = StructuralOp::NewTab {
            anchor_surface_id: a,
            surface_kind: "definitely-not-registered".to_string(),
            params: serde_json::json!({}),
        };
        let r = execute_forwarded_structural_op(&mut core, &mut state, &mut engine, &op);
        assert!(r.is_err(), "unknown kind must fail");
        assert!(
            r.unwrap_err().contains("unknown surface kind"),
            "reason 이 미등록 kind 를 가리켜야 한다"
        );
        assert_eq!(
            engine.terminals.iter().count(),
            before,
            "실패한 forward 는 새 터미널을 만들지 않는다"
        );
    }

    /// anchor surface 가 서버 트리에 없으면 Err(회신) — client 매핑이 stale 한 경우.
    #[test]
    fn forward_missing_anchor_fails() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        seed(&mut engine);
        let op = StructuralOp::ClosePane {
            anchor_surface_id: 999_999,
        };
        let r = execute_forwarded_structural_op(&mut core, &mut state, &mut engine, &op);
        assert!(r.is_err(), "missing anchor must fail");
    }
}
