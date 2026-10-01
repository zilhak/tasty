//! Outbound socket handshake, reader, writer and heartbeat. No App or View references.
use std::net::TcpStream;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool,Ordering};
use serde_json::Value;
use tasty_ipc::client::StreamConnection;
use tasty_ipc::stream::{self,STREAM_PROTO,StreamControl,StreamTag};
use super::client_session::{MirrorEvent,MirrorOutbox,OutFrame,FrameSender};
pub(crate) fn attach_handshake(
    port: u16,
    workspace: u32,
    log_prefix: &str,
) -> anyhow::Result<(StreamConnection, u32, TcpStream, String, Vec<Value>, Value)> {
    let sock = TcpStream::connect(("127.0.0.1", port))?;
    arm_attach_timeouts(&sock, log_prefix);
    let (mut conn, client_id) =
        StreamConnection::open_attach_workspace(sock, STREAM_PROTO, workspace)?;
    let first = conn.recv()?;
    if first.tag != StreamTag::Control {
        anyhow::bail!("expected attach Control frame, got {:?}", first.tag);
    }
    let ctrl: Value = serde_json::from_slice(&first.payload)?;
    let (name, surfaces, tree) = parse_attach_descriptor(&ctrl)?;
    // 손실 통지는 연결별 opt-in이다. 구 서버는 이 선언을 모를 수 있으므로
    // 지원을 확인해야 하면 system.info의 ipc.stream.loss-notify capability를 조회한다.
    let declare = serde_json::to_vec(&StreamControl::ClientLossNotify {}).unwrap_or_default();
    if let Err(e) = conn.send(StreamTag::Control, &declare) {
        tracing::warn!("{log_prefix}: 손실 통지 요청을 보내지 못했다: {e}");
    }
    let write_half = conn.try_clone_writer()?;
    Ok((conn, client_id, write_half, name, surfaces, tree))
}

pub(crate) fn arm_attach_timeouts(sock: &TcpStream, log_prefix: &str) {
    if let Err(e) = sock.set_read_timeout(Some(stream::HEARTBEAT_TIMEOUT)) {
        tracing::warn!("{log_prefix}: failed to set read timeout: {e}");
    }
    if let Err(e) = sock.set_write_timeout(Some(stream::HEARTBEAT_TIMEOUT)) {
        tracing::warn!("{log_prefix}: failed to set write timeout: {e}");
    }
}

pub(crate) fn parse_attach_descriptor(ctrl: &Value) -> anyhow::Result<(String, Vec<Value>, Value)> {
    match ctrl.get("event").and_then(|v| v.as_str()) {
        Some("attached_workspace") => {}
        Some("attach_error") => {
            let reason = ctrl
                .get("reason")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            anyhow::bail!("workspace attach rejected: {reason}");
        }
        other => anyhow::bail!("unexpected attach control event: {other:?}"),
    }
    let name = ctrl
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("remote")
        .to_string();
    let surfaces = ctrl
        .get("surfaces")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let tree = ctrl.get("tree").cloned().unwrap_or(Value::Null);
    Ok((name, surfaces, tree))
}

pub(crate) fn spawn_attach_write_thread(
    write_half: TcpStream,
    frame_rx: std::sync::mpsc::Receiver<super::connection::QueuedFrame>,
    disconnected: Arc<AtomicBool>,
    wake:Arc<dyn Fn()+Send+Sync>,
    log_suffix: &'static str,
) {
    std::thread::spawn(move || {
        let mut write_half = write_half;
        for queued in frame_rx {
            let item=queued.frame;
            let detach=item.tag==StreamTag::Detach;
            if !detach && (!queued.epoch.is_active() || disconnected.load(Ordering::Acquire)) {continue;}
            if let Err(e) = stream::write_frame(&mut write_half, item.tag, &item.payload) {
                tracing::warn!(
                    "attach write thread{log_suffix}: 프레임을 쓰지 못해 연결 종료로 처리한다: {e}"
                );
                disconnected.store(true, Ordering::SeqCst);
                (wake)();
                break;
            }
            if detach {
                if let Err(error)=write_half.shutdown(std::net::Shutdown::Both) {tracing::debug!("remote socket already closed: {error}");}
                break;
            }
        }
    });
}

pub(crate) fn spawn_attach_reader_thread(
    mut conn: StreamConnection,
    output: MirrorOutbox,
    disconnected: Arc<AtomicBool>,
    wake:Arc<dyn Fn()+Send+Sync>,
    local_workspace: u32,
    log_suffix: &'static str,
    decode_control:fn(&[u8])->Option<MirrorEvent>,
) {
    std::thread::spawn(move || {
        let mut mesh_assembler = tasty_ipc::mesh_stream::MeshFrameAssembler::new();
        loop {
            match conn.recv() {
                Ok(frame) => match frame.tag {
                    StreamTag::Data => {
                        if let Some((sid, payload)) = stream::decode_mux(&frame.payload) {
                            output.push(MirrorEvent::Data(sid, payload.to_vec()));
                        }
                        (wake)();
                    }
                    StreamTag::Detach => {
                        disconnected.store(true, Ordering::SeqCst);
                        (wake)();
                        break;
                    }
                    StreamTag::Control => {
                        if String::from_utf8_lossy(&frame.payload).contains("force_detached") {
                            disconnected.store(true, Ordering::SeqCst);
                            (wake)();
                            break;
                        }
                        let mirror_ev = decode_control(&frame.payload);
                        if let Some(ev) = mirror_ev
                            && output.push(ev)
                        {
                            (wake)();
                        }
                    }
                    StreamTag::Ping => {}
                    // 완성된 mesh frame만 적용한다. 손상 청크는 폐기하며 이후 full frame으로 복구해야 한다.
                    StreamTag::MeshData => {
                        if let Ok(Some((meta, bytes))) = mesh_assembler.push_chunk(&frame.payload)
                            && output.push(MirrorEvent::Mesh(
                                meta.surface_id,
                                meta.generation,
                                meta.frame_seq,
                                meta.full_textures,
                                bytes,
                            ))
                        {
                            (wake)();
                        }
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "attach reader thread{log_suffix}: mirror workspace {local_workspace} 원격 수신 실패로 연결 종료를 알린다: {e}"
                    );
                    disconnected.store(true, Ordering::SeqCst);
                    (wake)();
                    break;
                }
            }
        }
    });
}

pub(crate) fn spawn_attach_heartbeat_thread(raw_frame_tx: FrameSender, disconnected: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(stream::HEARTBEAT_INTERVAL);
            if disconnected.load(Ordering::SeqCst) {
                break;
            }
            if raw_frame_tx
                .send(OutFrame {
                    tag: StreamTag::Ping,
                    payload: Vec::new(),
                })
                .is_err()
            {
                break;
            }
        }
    });
}
