//! attach로 점유된 workspace에 비-holder가 로컬 PTY를 입양하려 하면 거절되고, 그 PTY는 headless로 남아
//! 점유되지 않은 workspace에는 그대로 입양되는지 확인한다. 거절 사유 문구와 holder 쪽 무변화는
//! `attach_structure_sync_loopback`이 확인한다. 로컬 생성 멤버의 tap 후처리는 비-holder IPC로 도달할 수 없어
//! `forward_exec_tests::local_split_in_held_workspace_sends_delta_then_taps_once` 단위 시험이 확인한다.

mod attach_common;
mod common;

use attach_common::open_workspace_attach;
use common::TastyInstance;
use serde_json::{Value, json};

fn pty_listed(instance: &TastyInstance, pty_id: u64) -> bool {
    instance.call("pty.list", json!({}))["ptys"]
        .as_array()
        .expect("ptys array")
        .iter()
        .any(|p| p["id"].as_u64() == Some(pty_id))
}

fn surface_entry(instance: &TastyInstance, surface_id: u64) -> Option<Value> {
    instance
        .call("surface.list", json!({}))
        .as_array()
        .expect("surface.list array")
        .iter()
        .find(|s| s["id"].as_u64() == Some(surface_id))
        .cloned()
}

#[test]
fn a_refused_pty_adopt_leaves_the_pty_headless_for_an_unheld_workspace() {
    let server = common::shared();
    let held = server.create_workspace("local-adopt-held");
    let free = server.create_workspace("local-adopt-free");
    let held_pane = server.first_pane_id_in_workspace(held.id);
    let free_pane = server.first_pane_id_in_workspace(free.id);

    let _attach_stream = open_workspace_attach(server.port(), held.id);

    let pty_id = server.call("pty.spawn", json!({}))["pty_id"]
        .as_u64()
        .expect("pty_id");

    let refused = server.call_raw(
        "pty.attach_surface",
        json!({ "id": pty_id, "pane_id": held_pane }),
    );
    assert!(
        refused.get("error").is_some(),
        "점유된 workspace 로의 입양은 거절돼야 한다: {refused:?}"
    );
    assert!(
        pty_listed(server, pty_id),
        "거절된 PTY 는 headless registry 에 남아 있어야 한다"
    );

    let adopted = server.call(
        "pty.attach_surface",
        json!({ "id": pty_id, "pane_id": free_pane }),
    );
    let surface_id = adopted["surface_id"].as_u64().expect("surface_id");
    assert!(
        !pty_listed(server, pty_id),
        "입양된 PTY 는 headless registry 에서 빠져야 한다"
    );
    let entry = surface_entry(server, surface_id).expect("입양한 surface 가 목록에 있어야 한다");
    assert_eq!(
        entry["workspace_id"],
        json!(free.id),
        "점유되지 않은 workspace 에 입양돼야 한다: {entry}"
    );
    assert_eq!(
        entry["attached"], false,
        "점유되지 않은 workspace 의 surface 는 attach 멤버가 아니다: {entry}"
    );
}
