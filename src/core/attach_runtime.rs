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

        // 각 터미널: 초기 스냅샷(mux) + 출력/resize forwarder(mux). client 끊김 시 자동 종료.
        for &sid in &class.terminals {
            self.tap_surface_for_stream(sid, client_id, hub);
        }

        tracing::debug!(
            "attach: workspace {workspace_id} -> client {client_id} ({} terminals, {} placeholders)",
            class.terminals.len(),
            class.non_terminals.len(),
        );
    }

    /// 한 라이브 터미널 surface 를 workspace-mode stream client 로 tap 한다: 초기 화면
    /// bulk 스냅샷(mux) 을 1회 push 한 뒤 출력·resize forwarder 스레드를 건다. 핸드셰이크
    /// (`attach_workspace_for_stream`)와 3단계 역반영(원격에 새로 생긴 surface 를
    /// on-the-fly 로 tap)이 공유한다.
    ///
    /// snapshot → tap 은 인접 동기 호출이라 그 사이 ingest 가 없어 누락/중복이 없다
    /// (메인루프 단일소유). client 끊김(push Unknown/Disconnected) 또는 원격 터미널
    /// drop(close 로 `terminals.remove` → tap sender drop → 채널 EOF) 시 forwarder 는
    /// 자연 종료한다.
    pub(crate) fn tap_surface_for_stream(
        &mut self,
        sid: SurfaceId,
        client_id: AttachClientId,
        hub: &StreamHub,
    ) {
        let Some(terminal) = self.terminals.get_mut(sid) else {
            return;
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

        // resize forwarder: 원격 grid 변경 → client mirror 크기 갱신. workspace 모드라
        // Control payload 에 remote surface_id(sid)를 실어 client 가 remote→local
        // 매핑으로 해당 mirror 만 갱신하게 한다.
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

    /// client-driven mirror geometry(ADR-0045): mirror client 가 보낸
    /// [`StreamControl::ClientResize`](crate::ipc::stream::StreamControl) 를 지정
    /// remote surface 의 **실제 PTY** 에 적용한다. `feed_attached_workspace_input`
    /// 과 동형으로 holder 를 검증해(그 workspace 를 점유한 client 만) 타 workspace 의
    /// grid 를 구동하지 못하게 막는다.
    ///
    /// 반환: 적용 시도 여부(`false` = anchor workspace 미발견/holder 아님/surface
    /// 없음). 실제 grid 변화 판정은 `Terminal::resize` 내부(동일값이면 no-op)이며,
    /// 변화가 있으면 기존 resize tap 이 server→client `Resize` echo 를 자동
    /// fan-out 한다 — 여기서 추가로 push 하지 않는다(echo 경로 재사용).
    pub fn apply_attached_workspace_resize(
        &mut self,
        client_id: AttachClientId,
        remote_surface_id: u32,
        cols: usize,
        rows: usize,
    ) -> bool {
        let Some(ws) = self.attach.workspace_of_surface(remote_surface_id) else {
            return false;
        };
        if self.attach.workspace_holder(ws) != Some(client_id) {
            return false;
        }
        if let Some(terminal) = self.terminals.get_mut(remote_surface_id) {
            terminal.resize(cols, rows);
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
        let (tree, surfaces) = self.build_workspace_tree_surfaces(idx, class);
        serde_json::json!({
            "event": "attached_workspace",
            "workspace_id": workspace_id,
            "name": self.workspaces[idx].name,
            "tree": tree,
            "surfaces": surfaces,
        })
    }

    /// `(tree, surfaces)` 페이로드 — 핸드셰이크 디스크립터(`build_workspace_descriptor`)와
    /// 3단계 역반영 delta([`StreamControl::StructuralDelta`])가 공유한다. tree 는
    /// `to_attach_tree_json`(분할 방향/비율 포함), surfaces 는 per-surface
    /// role/cols/rows/kind.
    pub(crate) fn build_workspace_tree_surfaces(
        &self,
        idx: usize,
        class: &AttachSurfaceClass,
    ) -> (serde_json::Value, Vec<serde_json::Value>) {
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
        (ws.to_attach_tree_json(), surfaces)
    }
}

/// mirror client 가 forward 한 구조 op 를 이 인스턴스(원격 = authoritative)에서 실행한
/// 결과로, 성공 시 client 에 역반영할 delta 를 담는다(3단계).
#[derive(Debug)]
pub(crate) struct ForwardedDelta {
    /// client 에 push 할 `StreamControl::StructuralDelta`(원격 ws 전체 트리+surfaces).
    pub delta: crate::ipc::stream::StreamControl,
    /// 이 op 로 **새로 생긴** 터미널 surface 들. 호출자가 delta push **직후**
    /// [`CoreState::tap_surface_for_stream`] 로 tap 을 건다(스냅샷이 client 매핑 생성
    /// 뒤에 도착하도록 delta 다음 순서를 보장).
    pub added_terminals: Vec<SurfaceId>,
}

/// mirror client 가 forward 한 구조 op 를 이 인스턴스(원격 = authoritative)에서
/// 실행한다. anchor **원격 surface id** 로 pane/tab/workspace 를 resolve 한 뒤 기존
/// IPC 핸들러(split / tab.create / tab.close / tab.move / pane.close / surface.close)를
/// 그대로 재사용해 full cascade(PTY spawn·host event·cleanup)로 처리한다 — 서버측
/// 워크스페이스는 mirror 가 아니므로 `Core::apply` 의 mirror 가드에 걸리지 않고 실제로
/// 실행된다.
///
/// 반환:
/// - 성공: `Ok(Some(delta))` — anchor 워크스페이스의 실행 **전/후** `all_surface_ids`
///   diff 로 added(신규 터미널)를 계산하고, 실행 후 트리+surfaces 를 담은
///   [`StreamControl::StructuralDelta`] 를 만들어 반환한다(핸들러 응답 파싱이 아니라
///   트리 diff 라 close cascade·move 도 균일 커버). 워크스페이스가 통째로 사라진 극단
///   케이스는 `Ok(None)`.
/// - 실패: JSON-RPC 에러(예: 원격 미등록 plugin kind → "unknown surface kind")면
///   `Err(reason)`.
///
/// 호출자(메인루프)가 [`StreamControl::StructuralResult`] 로 회신한 **뒤** delta 를 push
/// 하고, 그 다음 added_terminals 를 tap 한다(순서: result → delta → snapshot).
///
/// `ConvertSurface`/`MoveSurface` 는 재사용할 IPC 핸들러가 없어 아직 forward 대상이
/// 아니다(client 도 이 둘은 forward 하지 않고 기존 차단 유지) — 방어적으로 거부 사유를
/// 반환한다.
pub(crate) fn execute_forwarded_structural_op(
    core: &mut crate::core::Core,
    state: &mut crate::state::AppState,
    engine: &mut CoreState,
    op: &StructuralOp,
) -> Result<Option<ForwardedDelta>, String> {
    use crate::adapters::ipc::handler::{pane, surface, tab};
    use serde_json::json;
    use std::collections::HashSet;

    // anchor 워크스페이스를 실행 **전** 확보(close 로 anchor surface 가 사라져도 ws id
    // 로 재조회 가능하게). before-set 은 delta 의 added 계산 기준.
    let ws_id = engine
        .find_workspace_index_for_surface(op.anchor_surface_id())
        .map(|(idx, _)| engine.workspaces[idx].id);
    let before: HashSet<SurfaceId> = ws_id
        .and_then(|id| engine.find_workspace_index_for_id(id))
        .map(|idx| {
            engine.workspaces[idx]
                .all_surface_ids()
                .into_iter()
                .collect()
        })
        .unwrap_or_default();

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

    if let Some(err) = resp.error {
        return Err(err.message);
    }

    // 성공 — 실행 후 트리 스냅샷으로 delta 구성. anchor 를 못 찾았거나(방어) ws 가 통째로
    // 사라졌으면(극단) delta 없음.
    let Some(ws_id) = ws_id else {
        return Ok(None);
    };
    let Some(idx_after) = engine.find_workspace_index_for_id(ws_id) else {
        return Ok(None);
    };
    let class = engine.workspaces[idx_after].classify_attach_surfaces();
    // added = after − before 중 터미널만(비-터미널 placeholder 는 tap 불필요).
    let added_terminals: Vec<SurfaceId> = class
        .terminals
        .iter()
        .copied()
        .filter(|sid| !before.contains(sid))
        .collect();
    // 점유 상속(ADR-0040 "workspace 전체가 remote" 불변식): forward 로 원격에 새로 생긴
    // 터미널을 이 workspace 의 hard 점유에 편입한다. 등록하지 않으면 새 surface 가
    // 비점유로 남아 (1) host 창 sweep 이 자기 grid 로 되돌리고(레터박스) (2) is_hard_occupied
    // 미표시 (3) `feed_attached_workspace_input`/`apply_attached_workspace_resize` 의
    // holder 검증(surface_to_workspace 기반)이 실패해 client 입력·resize 가 거부된다.
    for sid in &added_terminals {
        engine.attach.add_workspace_member(ws_id, *sid, true);
    }
    let (tree, surfaces) = engine.build_workspace_tree_surfaces(idx_after, &class);
    let delta = crate::ipc::stream::StreamControl::StructuralDelta {
        workspace_id: ws_id,
        tree,
        surfaces,
    };
    Ok(Some(ForwardedDelta {
        delta,
        added_terminals,
    }))
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

    /// 성공한 op 의 delta 에서 surfaces 배열의 remote_id 집합을 뽑는다(테스트 헬퍼).
    fn delta_surface_ids(fd: &super::ForwardedDelta) -> std::collections::HashSet<u32> {
        let crate::ipc::stream::StreamControl::StructuralDelta { surfaces, .. } = &fd.delta else {
            panic!("expected StructuralDelta");
        };
        surfaces
            .iter()
            .filter_map(|s| {
                s.get("remote_id")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
            })
            .collect()
    }

    /// forward 된 SplitSurface 는 (비-mirror) 서버 워크스페이스에서 실제로 실행되어
    /// 새 터미널이 insert 된다(=원격이 새 PTY spawn). Ok + delta(신규 surface 포함).
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
        let fd = r
            .expect("expected Ok")
            .expect("split 성공은 delta 를 동반해야 한다");
        assert_eq!(
            engine.terminals.iter().count(),
            before + 1,
            "forward split 은 서버에서 새 터미널을 spawn 해야 한다"
        );
        // added_terminals 는 신규 surface 하나, delta.surfaces 에는 anchor+신규 모두 포함.
        assert_eq!(fd.added_terminals.len(), 1, "added 는 신규 터미널 1개");
        let ids = delta_surface_ids(&fd);
        assert!(ids.contains(&a), "delta 에 anchor surface 가 있어야 한다");
        assert!(
            ids.contains(&fd.added_terminals[0]),
            "delta.surfaces 에 신규 surface 가 있어야 한다"
        );
    }

    /// forward 된 split 으로 원격에 새로 생긴 surface 는 점유된 workspace 의 hard 점유를
    /// **상속**한다(ADR-0040 "workspace 전체가 remote"). 등록이 빠지면 새 surface 가
    /// 비점유로 남아 (host 창 sweep 이 grid 를 되돌리는) 레터박스·(holder 검증 실패로)
    /// client 입력 거부를 유발한다. surface_locks(is_hard_occupied) + surface_to_workspace
    /// (입력 라우팅) 양쪽에 등록돼야 한다.
    #[test]
    fn forward_split_inherits_workspace_occupancy() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        let ws_id = engine.workspaces[0].id;
        let client_id = 42;
        engine
            .attach
            .acquire_workspace(ws_id, &[a], &[a], client_id)
            .expect("workspace 점유 획득");
        let op = StructuralOp::SplitSurface {
            surface_id: a,
            direction: SplitAxis::Horizontal,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let fd = execute_forwarded_structural_op(&mut core, &mut state, &mut engine, &op)
            .expect("expected Ok")
            .expect("split delta");
        let new_sid = fd.added_terminals[0];
        assert!(
            engine.attach.is_hard_occupied(new_sid),
            "새 surface 는 hard 점유(surface_locks)를 상속해야 한다 — resize skip/readonly"
        );
        assert_eq!(
            engine.attach.workspace_of_surface(new_sid),
            Some(ws_id),
            "새 surface 는 점유 workspace 멤버로 등록돼야 한다 — 입력 라우팅 holder 검증"
        );
        assert_eq!(
            engine.attach.workspace_holder_of(new_sid),
            Some(client_id),
            "새 surface 의 holder 는 workspace holder 와 동일해야 한다"
        );
    }

    /// forward 된 NewTab 도 실제 실행(pane 은 anchor surface 로 resolve). Ok + delta + 터미널 +1.
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
        let fd = r.expect("expected Ok").expect("new-tab 성공은 delta 동반");
        assert_eq!(engine.terminals.iter().count(), before + 1);
        assert_eq!(fd.added_terminals.len(), 1);
    }

    /// forward 된 CloseTab 은 cascade 로 surface 를 제거한다 — delta 에 그 surface 가
    /// 빠지고(=client 가 removed 도출), added 는 비어 있다.
    #[test]
    fn forward_close_tab_removes_from_delta() {
        let (mut core, mut state, mut engine, _home) = make_core_state();
        let a = seed(&mut engine);
        // 먼저 new-tab 으로 두 번째 탭(surface) 을 만든다.
        let mk = StructuralOp::NewTab {
            anchor_surface_id: a,
            surface_kind: "terminal".to_string(),
            params: serde_json::json!({}),
        };
        let added = execute_forwarded_structural_op(&mut core, &mut state, &mut engine, &mk)
            .expect("new-tab Ok")
            .expect("new-tab delta");
        let new_sid = added.added_terminals[0];
        // 새 surface 가 속한 탭을 닫는다.
        let close = StructuralOp::CloseTab {
            anchor_surface_id: new_sid,
        };
        let fd = execute_forwarded_structural_op(&mut core, &mut state, &mut engine, &close)
            .expect("close Ok")
            .expect("close delta");
        assert!(fd.added_terminals.is_empty(), "close 는 added 없음");
        let ids = delta_surface_ids(&fd);
        assert!(
            !ids.contains(&new_sid),
            "닫힌 surface 는 delta 에서 빠져야 한다"
        );
        assert!(ids.contains(&a), "남은 surface 는 delta 에 유지");
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
