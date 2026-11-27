//! descriptor의 wire 형식 시험. 기대값의 키는 글자로 적어 struct와 상수의 이름 변경을 함께 잡는다.

use super::AttachedWorkspaceDescriptor;
use crate::{DESCRIPTOR_CAPABILITIES, DESCRIPTOR_PTY_INPUT, PTY_INPUT_VT};
use serde_json::json;

fn sample() -> AttachedWorkspaceDescriptor {
    AttachedWorkspaceDescriptor {
        workspace_id: 7,
        name: "work".to_string(),
        tree: json!({"kind": "leaf", "pane": 1}),
        surfaces: vec![json!({"remote_id": 3, "role": "terminal", "cols": 80, "rows": 24})],
        stream_capabilities: vec!["ipc.stream.resize-ack".to_string()],
        pty_input: "win32-console".to_string(),
    }
}

#[test]
fn a_descriptor_serializes_to_the_wire_keys() {
    let value = serde_json::to_value(sample()).expect("descriptor serializes");
    assert_eq!(
        value,
        json!({
            "event": "attached_workspace",
            "workspace_id": 7,
            "name": "work",
            "tree": {"kind": "leaf", "pane": 1},
            "surfaces": [{"remote_id": 3, "role": "terminal", "cols": 80, "rows": 24}],
            "stream_capabilities": ["ipc.stream.resize-ack"],
            "pty_input": "win32-console",
        })
    );
    assert_eq!(
        value[DESCRIPTOR_CAPABILITIES],
        json!(["ipc.stream.resize-ack"])
    );
    assert_eq!(value[DESCRIPTOR_PTY_INPUT], json!("win32-console"));
    assert!(crate::descriptor_wants_win32_console_input(&value));
}

#[test]
fn a_descriptor_reads_back_what_it_writes() {
    let text = serde_json::to_string(&sample()).expect("descriptor serializes");
    let back: AttachedWorkspaceDescriptor = serde_json::from_str(&text).expect("descriptor parses");
    assert_eq!(back, sample());
}

#[test]
fn an_older_server_descriptor_reads_with_no_capabilities_and_vt_input() {
    let older = json!({
        "event": "attached_workspace",
        "workspace_id": 7,
        "name": "work",
        "tree": null,
        "surfaces": [],
    });
    let read: AttachedWorkspaceDescriptor =
        serde_json::from_value(older.clone()).expect("older descriptor parses");
    assert!(read.stream_capabilities.is_empty());
    assert_eq!(read.pty_input, PTY_INPUT_VT);
    assert!(!crate::descriptor_wants_win32_console_input(&older));
}

#[test]
fn another_event_is_not_a_workspace_descriptor() {
    let mut other = serde_json::to_value(sample()).expect("descriptor serializes");
    other["event"] = json!("attach_error");
    assert!(serde_json::from_value::<AttachedWorkspaceDescriptor>(other.clone()).is_err());
    other
        .as_object_mut()
        .expect("descriptor is an object")
        .remove("event");
    assert!(serde_json::from_value::<AttachedWorkspaceDescriptor>(other).is_err());
}
