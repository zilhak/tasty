//! attach된 workspace에 로컬 IPC로 추가한 터미널이 점유 목록과 출력 스트림에 포함되는지 확인한다.
//! 점유 중 tab.create는 거절되므로 실제 에이전트 경로인 pty.spawn → pty.attach_surface를 사용한다.
//! 새 surface의 attached 값과 해당 ID의 Data 프레임을 확인하며 GUI 렌더링 자체는 검사하지 않는다.

mod attach_common;
mod common;

use std::net::TcpStream;

use attach_common::{TAG_DATA, open_workspace_attach, read_frame};
use common::TastyInstance;
use serde_json::json;
use tasty_ipc::stream::decode_mux;

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
    panic!("attach 소켓에서 새 surface {surface_id}의 Data 프레임을 받지 못했다");
}

#[test]
fn local_pty_adopt_in_occupied_workspace_is_tapped() {
    let server = common::shared();
    let ws = server.create_workspace("local-creation-tap");
    let pane_id = server.first_pane_id_in_workspace(ws.id);

    let mut attach_stream = open_workspace_attach(server.port(), ws.id);

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
        surface_attached(server, new_surface_id as u64),
        "attach 점유 중인 workspace 에 새로 승격된 surface 는 attached:true 여야 한다"
    );

    expect_data_frame_for_surface(&mut attach_stream, new_surface_id);
}
