mod common;

use common::TastyInstance;
use std::time::Duration;

#[test]
fn app_starts_and_has_shell() {
    let tasty = TastyInstance::spawn();
    let info = tasty.call("system.info", serde_json::json!({}));
    assert_eq!(
        info.get("version").and_then(|v| v.as_str()),
        Some("0.1.0")
    );
    assert!(info.get("workspace_count").and_then(|v| v.as_u64()).unwrap() >= 1);
}

#[test]
fn echo_hello() {
    let tasty = TastyInstance::spawn();
    tasty.set_mark();
    // Use platform-appropriate echo
    if cfg!(windows) {
        tasty.send_text("echo hello\r\n");
    } else {
        tasty.send_text("echo hello\n");
    }
    let output = tasty.wait_for_output("hello", Duration::from_secs(5));
    assert!(output.contains("hello"));
}

#[test]
fn create_workspace() {
    let tasty = TastyInstance::spawn();
    // workspace.list returns an array directly
    let before = tasty
        .call("workspace.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    tasty.call(
        "workspace.create",
        serde_json::json!({"name": "test"}),
    );
    let after = tasty
        .call("workspace.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after, before + 1);
}

#[test]
fn create_and_close_tab() {
    let tasty = TastyInstance::spawn();
    // tab.list returns an array directly
    let before = tasty
        .call("tab.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    tasty.call("tab.create", serde_json::json!({}));
    let after_create = tasty
        .call("tab.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after_create, before + 1);

    tasty.call("tab.close", serde_json::json!({}));
    let after_close = tasty
        .call("tab.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after_close, before);
}

#[test]
fn split_pane() {
    let tasty = TastyInstance::spawn();
    // pane.list returns an array directly
    let before = tasty
        .call("pane.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    tasty.call(
        "pane.split",
        serde_json::json!({"direction": "vertical"}),
    );
    let after = tasty
        .call("pane.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after, before + 1);
}

#[test]
fn close_pane() {
    let tasty = TastyInstance::spawn();
    tasty.call(
        "pane.split",
        serde_json::json!({"direction": "vertical"}),
    );
    let before = tasty
        .call("pane.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    assert!(before >= 2);
    tasty.call("pane.close", serde_json::json!({}));
    let after = tasty
        .call("pane.list", serde_json::json!({}))
        .as_array()
        .unwrap()
        .len();
    assert_eq!(after, before - 1);
}

#[test]
fn switch_workspace() {
    let tasty = TastyInstance::spawn();
    tasty.call(
        "workspace.create",
        serde_json::json!({"name": "ws2"}),
    );
    let info1 = tasty.call("system.info", serde_json::json!({}));
    let active1 = info1
        .get("active_workspace")
        .and_then(|v| v.as_u64())
        .unwrap();

    tasty.call("workspace.select", serde_json::json!({"index": 0}));
    let info2 = tasty.call("system.info", serde_json::json!({}));
    let active2 = info2
        .get("active_workspace")
        .and_then(|v| v.as_u64())
        .unwrap();
    assert_ne!(active1, active2);
}

#[test]
fn notification_via_ipc() {
    let tasty = TastyInstance::spawn();
    tasty.call(
        "notification.create",
        serde_json::json!({
            "title": "Test",
            "body": "Hello"
        }),
    );
    // notification.list returns an array directly
    let result = tasty.call("notification.list", serde_json::json!({}));
    let notifications = result.as_array().unwrap();
    assert!(notifications.len() >= 1);
}

#[test]
fn hook_set_and_list() {
    let tasty = TastyInstance::spawn();
    let result = tasty.call(
        "hook.set",
        serde_json::json!({
            "surface_id": 1,
            "event": "bell",
            "command": "echo hooked"
        }),
    );
    let hook_id = result.get("hook_id").and_then(|v| v.as_u64()).unwrap();
    assert!(hook_id > 0);

    // hook.list returns an array directly
    let hooks = tasty.call("hook.list", serde_json::json!({}));
    let hook_list = hooks.as_array().unwrap();
    assert!(hook_list.len() >= 1);

    tasty.call("hook.unset", serde_json::json!({"hook_id": hook_id}));
    let hooks_after = tasty.call("hook.list", serde_json::json!({}));
    let hook_list_after = hooks_after.as_array().unwrap();
    assert_eq!(hook_list_after.len(), hook_list.len() - 1);
}

#[test]
fn mark_and_read() {
    let tasty = TastyInstance::spawn();
    tasty.set_mark();
    if cfg!(windows) {
        tasty.send_text("echo test_marker_output\r\n");
    } else {
        tasty.send_text("echo test_marker_output\n");
    }
    let output = tasty.wait_for_output("test_marker_output", Duration::from_secs(5));
    assert!(output.contains("test_marker_output"));
}

#[test]
fn tree_view() {
    let tasty = TastyInstance::spawn();
    // tree returns an array of workspace objects directly
    let result = tasty.call("tree", serde_json::json!({}));
    let tree = result.as_array().unwrap();
    assert!(!tree.is_empty());
    // Each entry should have a "name" field
    assert!(tree[0].get("name").is_some());
}

#[test]
fn screen_text() {
    let tasty = TastyInstance::spawn();
    // Wait for shell prompt
    std::thread::sleep(Duration::from_secs(1));
    let text = tasty.screen_text();
    // Should have some content (shell prompt)
    assert!(!text.trim().is_empty());
}

#[test]
fn cursor_position() {
    let tasty = TastyInstance::spawn();
    std::thread::sleep(Duration::from_millis(500));
    let result = tasty.call("surface.cursor_position", serde_json::json!({}));
    // Should return x and y fields
    assert!(result.get("x").is_some());
    assert!(result.get("y").is_some());
}

#[test]
fn multiple_workspaces_independent() {
    let tasty = TastyInstance::spawn();

    // Set mark on workspace 1
    tasty.set_mark();
    if cfg!(windows) {
        tasty.send_text("echo workspace1\r\n");
    } else {
        tasty.send_text("echo workspace1\n");
    }
    tasty.wait_for_output("workspace1", Duration::from_secs(5));

    // Create workspace 2 (switches to it)
    tasty.call(
        "workspace.create",
        serde_json::json!({"name": "ws2"}),
    );
    // Set mark there
    tasty.set_mark();
    if cfg!(windows) {
        tasty.send_text("echo workspace2\r\n");
    } else {
        tasty.send_text("echo workspace2\n");
    }
    let output2 = tasty.wait_for_output("workspace2", Duration::from_secs(5));

    // workspace2 output should NOT contain workspace1
    assert!(!output2.contains("workspace1"));
}
