mod common;

use common::TastyInstance;
use serde_json::json;
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
    // spawn() already guarantees shell is ready
    let text = tasty.screen_text();
    assert!(!text.trim().is_empty());
}

#[test]
fn cursor_position() {
    let tasty = TastyInstance::spawn();
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

// ============================================================
// New IPC API tests: full remote control
// ============================================================

#[test]
fn send_combo_ctrl_c() {
    let tasty = TastyInstance::spawn();
    // Verify send_combo works by sending Ctrl+C (0x03) to the terminal
    let result = tasty.call("surface.send_combo", json!({"key": "c", "modifiers": ["ctrl"]}));
    assert_eq!(result["sent"], true);

    // Also verify other combos don't error
    let result = tasty.call("surface.send_combo", json!({"key": "z", "modifiers": ["ctrl"]}));
    assert_eq!(result["sent"], true);

    let result = tasty.call("surface.send_combo", json!({"key": "d", "modifiers": ["ctrl"]}));
    assert_eq!(result["sent"], true);

    // After Ctrl+D the shell may exit, give it a moment and verify shell prompt returns
    std::thread::sleep(Duration::from_millis(500));

    // Verify Alt combo (sends ESC prefix)
    let result = tasty.call("surface.send_combo", json!({"key": "x", "modifiers": ["alt"]}));
    assert_eq!(result["sent"], true);
}

#[test]
fn pane_focus_by_id() {
    let tasty = TastyInstance::spawn();

    // Get initial focused pane
    let panes_before = tasty.call("pane.list", json!({}));
    let first_pane_id = panes_before.as_array().unwrap()[0]["id"].as_u64().unwrap();

    // Split pane
    tasty.call("pane.split", json!({"direction": "vertical"}));

    // Get new pane list
    let panes_after = tasty.call("pane.list", json!({}));
    let panes = panes_after.as_array().unwrap();
    assert_eq!(panes.len(), 2);

    // Focus the first (original) pane by ID
    let result = tasty.call("pane.focus", json!({"pane_id": first_pane_id}));
    assert_eq!(result["focused"], true);

    // Verify it's actually focused
    let panes_check = tasty.call("pane.list", json!({}));
    let focused = panes_check.as_array().unwrap().iter()
        .find(|p| p["focused"].as_bool() == Some(true))
        .unwrap();
    assert_eq!(focused["id"].as_u64().unwrap(), first_pane_id);
}

#[test]
fn surface_focus_by_id() {
    let tasty = TastyInstance::spawn();

    // Get surface list
    let surfaces = tasty.call("surface.list", json!({}));
    let first_surface_id = surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap();

    // Split pane to create another surface
    tasty.call("pane.split", json!({"direction": "vertical"}));

    // Focus the first surface by ID
    let result = tasty.call("surface.focus", json!({"surface_id": first_surface_id}));
    assert_eq!(result["focused"], true);
}

#[test]
fn send_to_specific_surface() {
    let tasty = TastyInstance::spawn();

    // Get the surface ID
    let surfaces = tasty.call("surface.list", json!({}));
    let sid = surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap();

    // Send text to specific surface
    tasty.call("surface.set_mark", json!({"surface_id": sid}));
    tasty.call("surface.send_to", json!({"surface_id": sid, "text": "echo targeted_send\r\n"}));

    let output = tasty.wait_for_output("targeted_send", Duration::from_secs(5));
    assert!(output.contains("targeted_send"));
}

#[test]
fn screen_text_by_surface_id() {
    let tasty = TastyInstance::spawn();
    // spawn() already guarantees shell is ready

    let surfaces = tasty.call("surface.list", json!({}));
    let sid = surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap();

    let result = tasty.call("surface.screen_text", json!({"surface_id": sid}));
    let text = result["text"].as_str().unwrap_or("");
    assert!(!text.trim().is_empty(), "screen_text by surface_id should return content");
}

#[test]
fn cursor_position_by_surface_id() {
    let tasty = TastyInstance::spawn();

    let surfaces = tasty.call("surface.list", json!({}));
    let sid = surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap();

    let result = tasty.call("surface.cursor_position", json!({"surface_id": sid}));
    assert!(result.get("x").is_some());
    assert!(result.get("y").is_some());
}

#[test]
fn send_key_with_surface_id() {
    let tasty = TastyInstance::spawn();

    let surfaces = tasty.call("surface.list", json!({}));
    let sid = surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap();

    // Send text and then a key to specific surface
    tasty.call("surface.set_mark", json!({"surface_id": sid}));
    tasty.call("surface.send", json!({"surface_id": sid, "text": "echo key_test"}));
    tasty.call("surface.send_key", json!({"surface_id": sid, "key": "enter"}));

    let output = tasty.wait_for_output("key_test", Duration::from_secs(5));
    assert!(output.contains("key_test"));
}

#[test]
fn send_key_function_keys() {
    let tasty = TastyInstance::spawn();
    // Just verify these don't error out
    for key in &["f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8", "f9", "f10", "f11", "f12"] {
        let result = tasty.call("surface.send_key", json!({"key": key}));
        assert_eq!(result["sent"], true, "Failed to send key: {}", key);
    }
}

#[test]
fn ui_state_query() {
    let tasty = TastyInstance::spawn();
    let result = tasty.call("ui.state", json!({}));
    assert_eq!(result["settings_open"], false);
    assert_eq!(result["notification_panel_open"], false);
    assert!(result["workspace_count"].as_u64().unwrap() >= 1);
    assert!(result["pane_count"].as_u64().unwrap() >= 1);
    assert!(result["tab_count"].as_u64().unwrap() >= 1);
}

// Error path tests

#[test]
fn pane_focus_nonexistent_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("pane.focus", json!({"pane_id": 99999}));
    assert!(resp.get("error").is_some(), "Should return error for nonexistent pane");
}

#[test]
fn surface_focus_nonexistent_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("surface.focus", json!({"surface_id": 99999}));
    assert!(resp.get("error").is_some(), "Should return error for nonexistent surface");
}

#[test]
fn send_to_nonexistent_surface_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("surface.send_to", json!({"surface_id": 99999, "text": "hello"}));
    assert!(resp.get("error").is_some(), "Should return error for nonexistent surface");
}

#[test]
fn send_combo_missing_key_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("surface.send_combo", json!({"modifiers": ["ctrl"]}));
    assert!(resp.get("error").is_some(), "Should return error when key is missing");
}

#[test]
fn pane_focus_missing_id_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("pane.focus", json!({}));
    assert!(resp.get("error").is_some(), "Should return error when pane_id is missing");
}

#[test]
fn surface_focus_missing_id_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("surface.focus", json!({}));
    assert!(resp.get("error").is_some(), "Should return error when surface_id is missing");
}

#[test]
fn send_to_missing_surface_id_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("surface.send_to", json!({"text": "hello"}));
    assert!(resp.get("error").is_some(), "Should return error when surface_id is missing");
}

#[test]
fn send_to_missing_text_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("surface.send_to", json!({"surface_id": 1}));
    assert!(resp.get("error").is_some(), "Should return error when text is missing");
}

#[test]
fn method_not_found_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("nonexistent.method", json!({}));
    assert!(resp.get("error").is_some(), "Should return error for unknown method");
    let code = resp["error"]["code"].as_i64().unwrap();
    assert_eq!(code, -32601, "Should be method_not_found error code");
}

#[test]
fn workspace_select_out_of_range_returns_error() {
    let tasty = TastyInstance::spawn();
    let resp = tasty.call_raw("workspace.select", json!({"index": 9999}));
    assert!(resp.get("error").is_some(), "Should return error for out-of-range index");
}

#[test]
fn close_last_pane_returns_not_closed() {
    let tasty = TastyInstance::spawn();
    let result = tasty.call("pane.close", json!({}));
    assert_eq!(result["closed"], false, "Should not close the last pane");
}

#[test]
fn close_last_tab_returns_not_closed() {
    let tasty = TastyInstance::spawn();
    let result = tasty.call("tab.close", json!({}));
    assert_eq!(result["closed"], false, "Should not close the last tab");
}
