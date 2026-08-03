//! (docs/todo-conductor 04) attach 중인 workspace 에서 **로컬** IPC 로 새 tab 을
//! 만들 때 그 새 터미널 surface 가 스트림 tap 대상에서 누락되는 회귀를 막는다.
//!
//! `tests/attach_list_dir_loopback.rs` 와 동일한 접근: attach client 는 실제 GUI 앱이
//! 아니라 raw `TcpStream` 으로 `stream.open{target_workspace}` 핸드셰이크만 흉내낸다.
//! 서버는 이 연결이 실제 mirror client 인지 신경 쓰지 않으므로, `tasty claude spawn`
//! 등이 타는 것과 동일한 IPC 채널로 구조 변경을 일으켜도 재현에 충분하다.
//!
//! **`pty.spawn` + `pty.attach_surface` 를 쓴다 — `tab.create` 가 아니다.** IPC 라우터의
//! `hard_occupied_structural_guard`(`src/adapters/ipc/handler.rs`)는 `tab.create`/
//! `split`/`tab.close`/`tab.move`/`pane.close`/`surface.close` 를 hard-occupied
//! workspace 에서 거부하지만, `pty.attach_surface`(headless PTY → Surface 승격,
//! `AdoptTerminal` intent — `tasty claude spawn` 이 실제로 타는 경로)는 그 가드
//! 목록에 없어 통과한다. 즉 이 테스트가 재현하는 것이 실제 프로덕션 repro 경로다.
//!
//! 검증 대상(완료 확인 방법의 "실측 재현" 절차를 자동화):
//! 1. `surface.list` 의 새 surface `attached` 필드가 `true` 여야 한다(occupancy 편입).
//! 2. 그 새 surface 의 초기 스냅샷(mux `Data` 프레임)이 attach 소켓으로 실제 push
//!    돼야 한다(스트림 tap 이 실제로 시작됐다는 증거 — 검정 화면 회귀의 핵심 판정).

mod common;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use common::TastyInstance;
use serde_json::{Value, json};
use tasty_ipc::stream::decode_mux;

const TAG_DATA: u8 = 0;
const TAG_CONTROL: u8 = 1;

fn read_frame(stream: &mut TcpStream) -> (u8, Vec<u8>) {
    let mut hdr = [0u8; 5];
    stream.read_exact(&mut hdr).expect("read frame header");
    let tag = hdr[0];
    let len = u32::from_be_bytes([hdr[1], hdr[2], hdr[3], hdr[4]]) as usize;
    let mut payload = vec![0u8; len];
    if len > 0 {
        stream.read_exact(&mut payload).expect("read frame payload");
    }
    (tag, payload)
}

/// `stream.open{target_workspace}` 핸드셰이크 — `attach_list_dir_loopback.rs` 의
/// `open_workspace_attach` 와 동형(중복 허용 — test 유틸 공유 모듈은 이 변경 범위 밖).
fn open_workspace_attach(port: u16, workspace_id: u64) -> TcpStream {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect to server");
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .expect("set read timeout");

    let req = json!({
        "jsonrpc": "2.0",
        "method": "stream.open",
        "params": {"proto": 1, "target_workspace": workspace_id},
        "id": 1,
    });
    let mut msg = serde_json::to_string(&req).unwrap();
    msg.push('\n');
    stream.write_all(msg.as_bytes()).expect("send handshake");

    loop {
        let (tag, payload) = read_frame(&mut stream);
        if tag != TAG_CONTROL {
            continue;
        }
        let v: Value = serde_json::from_slice(&payload).unwrap();
        if let Some(ok) = v.get("ok") {
            assert_eq!(ok, true, "handshake rejected: {v:?}");
            continue;
        }
        match v.get("event").and_then(|e| e.as_str()) {
            Some("attached_workspace") => return stream,
            Some("attach_error") => panic!("workspace attach rejected: {v:?}"),
            _ => continue, // 기존 터미널의 초기 스냅샷 등 무관한 프레임 — 계속 대기.
        }
    }
}

fn first_workspace_id(instance: &TastyInstance) -> u64 {
    let workspaces = instance.call("workspace.list", json!({}));
    workspaces.as_array().unwrap()[0]["id"].as_u64().unwrap()
}

fn first_pane_id_for_workspace(instance: &TastyInstance, workspace_id: u64) -> u64 {
    let panes = instance.call("pane.list", json!({}));
    panes
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["workspace_id"].as_u64() == Some(workspace_id))
        .expect("pane for workspace")["id"]
        .as_u64()
        .unwrap()
}

fn surface_attached(instance: &TastyInstance, surface_id: u64) -> bool {
    let surfaces = instance.call("surface.list", json!({}));
    surfaces
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"].as_u64() == Some(surface_id))
        .expect("created surface must appear in surface.list")["attached"]
        .as_bool()
        .unwrap()
}

/// mirror client 가 attach 소켓에서 특정 surface_id 의 mux `Data` 프레임(초기 스냅샷
/// 또는 이후 출력)을 수신하는지 확인 — 못 받으면 "검정 화면" 회귀 그 자체다.
fn expect_data_frame_for_surface(stream: &mut TcpStream, surface_id: u32) {
    for _ in 0..64 {
        let (tag, payload) = read_frame(stream);
        if tag != TAG_DATA {
            continue;
        }
        if let Some((sid, _bytes)) = decode_mux(&payload)
            && sid == surface_id
        {
            return;
        }
    }
    panic!("attach 소켓에서 새 surface {surface_id}의 Data 프레임을 받지 못함 — 검정 화면 회귀");
}

#[test]
fn local_pty_adopt_in_occupied_workspace_is_tapped() {
    let server = TastyInstance::spawn();
    let ws_id = first_workspace_id(&server);
    let pane_id = first_pane_id_for_workspace(&server, ws_id);

    // attach client 가 이 workspace 를 점유 — `tasty claude spawn` 이후에도 attach
    // client 화면에 새 tab 이 검정으로만 보이던 실측 재현 조건과 동일.
    let mut attach_stream = open_workspace_attach(server.port(), ws_id);

    // `tasty claude spawn` 과 동일한 실제 경로: headless PTY spawn → 그 pane 에 승격.
    let spawned = server.call(
        "pty.spawn",
        json!({
            "command": ["echo", "hello-attach-tap-test"],
        }),
    );
    let pty_id = spawned["pty_id"].as_u64().expect("pty_id");

    let adopted = server.call(
        "pty.attach_surface",
        json!({
            "id": pty_id,
            "pane_id": pane_id,
        }),
    );
    let new_surface_id = adopted["surface_id"].as_u64().expect("surface_id") as u32;

    assert!(
        surface_attached(&server, new_surface_id as u64),
        "attach 점유 중인 workspace 에 새로 승격된 surface 는 attached:true 여야 한다"
    );

    expect_data_frame_for_surface(&mut attach_stream, new_surface_id);
}
