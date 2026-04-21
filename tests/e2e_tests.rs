mod common;

use common::TastyInstance;
use serde_json::json;
use std::time::Duration;

/// All e2e tests run on a single shared tasty instance.
/// This minimizes window spawn/kill (which steals OS focus).
#[test]
fn all_e2e_tests() {
    let tasty = TastyInstance::spawn();
    let sid = tasty.first_surface_id();
    let pid = tasty.first_pane_id();

    // ========== Read-only queries ==========

    // system.info
    let info = tasty.call("system.info", json!({}));
    assert_eq!(
        info.get("version").and_then(|v| v.as_str()),
        Some(env!("CARGO_PKG_VERSION")),
    );
    assert!(info["workspace_count"].as_u64().unwrap() >= 1);

    // tree
    let tree = tasty.call("tree", json!({}));
    let tree_arr = tree.as_array().unwrap();
    assert!(!tree_arr.is_empty());
    assert!(tree_arr[0].get("name").is_some());

    // ui.state
    let ui = tasty.call("ui.state", json!({}));
    assert_eq!(ui["settings_open"], false);
    assert_eq!(ui["notification_panel_open"], false);
    assert!(ui["workspace_count"].as_u64().unwrap() >= 1);
    assert!(ui["pane_count"].as_u64().unwrap() >= 1);
    assert!(ui["tab_count"].as_u64().unwrap() >= 1);

    // surface.list / pane.list
    assert!(
        !tasty
            .call("surface.list", json!({}))
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        !tasty
            .call("pane.list", json!({}))
            .as_array()
            .unwrap()
            .is_empty()
    );

    // screen_text
    let text = tasty.screen_text_of(sid);
    assert!(!text.trim().is_empty());

    // cursor_position
    let cursor = tasty.call("surface.cursor_position", json!({"surface_id": sid}));
    assert!(cursor.get("x").is_some());
    assert!(cursor.get("y").is_some());

    // tab.list
    let tabs = tasty.call("tab.list", json!({"pane_id": pid}));
    assert!(tabs["tabs"].as_array().unwrap().len() >= 1);

    // notification
    tasty.call(
        "notification.create",
        json!({"title": "Test", "body": "Hello"}),
    );
    let notifs = tasty.call("notification.list", json!({}));
    assert!(notifs.as_array().unwrap().len() >= 1);

    // ========== Terminal I/O ==========

    // echo + mark/read
    tasty.set_mark(sid);
    let echo_cmd = if cfg!(windows) {
        "echo hello\r\n"
    } else {
        "echo hello\n"
    };
    tasty.send_text(sid, echo_cmd);
    let output = tasty.wait_for_output(sid, "hello", Duration::from_secs(5));
    assert!(output.contains("hello"));

    // mark_and_read
    tasty.set_mark(sid);
    let echo_cmd = if cfg!(windows) {
        "echo test_marker\r\n"
    } else {
        "echo test_marker\n"
    };
    tasty.send_text(sid, echo_cmd);
    let output = tasty.wait_for_output(sid, "test_marker", Duration::from_secs(5));
    assert!(output.contains("test_marker"));

    // send_key (enter)
    tasty.set_mark(sid);
    tasty.call(
        "surface.send",
        json!({"surface_id": sid, "text": "echo key_test"}),
    );
    tasty.call(
        "surface.send_key",
        json!({"surface_id": sid, "key": "enter"}),
    );
    let output = tasty.wait_for_output(sid, "key_test", Duration::from_secs(5));
    assert!(output.contains("key_test"));

    // send_key: navigation keys
    for key in &["up", "down"] {
        let result = tasty.call("surface.send_key", json!({"surface_id": sid, "key": key}));
        assert_eq!(result["sent"], true, "Failed to send key: {}", key);
    }

    // send_to specific surface
    tasty.set_mark(sid);
    tasty.call(
        "surface.send_to",
        json!({"surface_id": sid, "text": "echo targeted\n"}),
    );
    let output = tasty.wait_for_output(sid, "targeted", Duration::from_secs(5));
    assert!(output.contains("targeted"));

    // send_combo (safe: Alt+X only — no Ctrl+C/Z which disrupts shell)
    let result = tasty.call(
        "surface.send_combo",
        json!({"surface_id": sid, "key": "x", "modifiers": ["alt"]}),
    );
    assert_eq!(result["sent"], true);

    // ========== Hooks ==========

    let hook_result = tasty.call(
        "hook.set",
        json!({"surface_id": sid, "event": "bell", "command": "echo hooked"}),
    );
    let hook_id = hook_result["hook_id"].as_u64().unwrap();
    assert!(hook_id > 0);

    let hooks = tasty.call("hook.list", json!({}));
    let hook_count = hooks.as_array().unwrap().len();
    assert!(hook_count >= 1);

    tasty.call("hook.unset", json!({"hook_id": hook_id}));
    let hooks_after = tasty.call("hook.list", json!({}));
    assert_eq!(hooks_after.as_array().unwrap().len(), hook_count - 1);

    // ========== Structural mutations ==========

    // create workspace
    let ws_before = tasty
        .call("workspace.list", json!({}))
        .as_array()
        .unwrap()
        .len();
    tasty.call("workspace.create", json!({"name": "test"}));
    let ws_after = tasty
        .call("workspace.list", json!({}))
        .as_array()
        .unwrap()
        .len();
    assert_eq!(ws_after, ws_before + 1);

    // split pane
    let panes_before = tasty.call("pane.list", json!({})).as_array().unwrap().len();
    let split_result = tasty.call(
        "split",
        json!({"level": "pane", "direction": "vertical", "target": pid}),
    );
    let new_pane_id = split_result["new_pane_id"].as_u64().unwrap();
    let panes_after = tasty.call("pane.list", json!({})).as_array().unwrap().len();
    assert_eq!(panes_after, panes_before + 1);

    // create tab
    let tabs_before = tasty.call("tab.list", json!({"pane_id": pid}))["tabs"]
        .as_array()
        .unwrap()
        .len();
    tasty.call("tab.create", json!({"pane_id": pid}));
    let tabs_after = tasty.call("tab.list", json!({"pane_id": pid}))["tabs"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(tabs_after, tabs_before + 1);

    // close tab
    let tab_list = tasty.call("tab.list", json!({"pane_id": pid}));
    let last_tab_id = tab_list["tabs"].as_array().unwrap().last().unwrap()["id"]
        .as_u64()
        .unwrap();
    let close_result = tasty.call("tab.close", json!({"tab_id": last_tab_id}));
    assert_eq!(close_result["closed"], true);
    let tabs_final = tasty.call("tab.list", json!({"pane_id": pid}))["tabs"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(tabs_final, tabs_before);

    // close pane (the one we split off)
    let close_pane_result = tasty.call("pane.close", json!({"pane_id": new_pane_id}));
    assert_eq!(close_pane_result["closed"], true);
    let panes_final = tasty.call("pane.list", json!({})).as_array().unwrap().len();
    assert_eq!(panes_final, panes_before);

    // close last pane → should refuse
    let sole_pid = tasty.first_pane_id();
    let result = tasty.call("pane.close", json!({"pane_id": sole_pid}));
    assert_eq!(result["closed"], false);

    // close last tab → should refuse
    let tab_list = tasty.call("tab.list", json!({"pane_id": sole_pid}));
    let sole_tab_id = tab_list["tabs"].as_array().unwrap()[0]["id"]
        .as_u64()
        .unwrap();
    let result = tasty.call("tab.close", json!({"tab_id": sole_tab_id}));
    assert_eq!(result["closed"], false);

    // ========== Error paths ==========

    // method_not_found
    let resp = tasty.call_raw("nonexistent.method", json!({}));
    assert!(resp.get("error").is_some());
    assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32601);

    // send_combo missing key
    let resp = tasty.call_raw(
        "surface.send_combo",
        json!({"surface_id": sid, "modifiers": ["ctrl"]}),
    );
    assert!(resp.get("error").is_some());

    // send_to nonexistent surface
    let resp = tasty.call_raw(
        "surface.send_to",
        json!({"surface_id": 99999, "text": "hello"}),
    );
    assert!(resp.get("error").is_some());

    // missing required params
    assert!(
        tasty
            .call_raw("surface.send_to", json!({"text": "hello"}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("surface.send_to", json!({"surface_id": 1}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("surface.send", json!({"text": "hello"}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("surface.set_mark", json!({}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("surface.screen_text", json!({}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("surface.cursor_position", json!({}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("pane.close", json!({}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("tab.close", json!({}))
            .get("error")
            .is_some()
    );
    assert!(tasty.call_raw("tab.list", json!({})).get("error").is_some());
    assert!(
        tasty
            .call_raw("tab.create", json!({}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("split", json!({"direction": "vertical"}))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("split", json!({"level": "pane", "direction": "vertical"}))
            .get("error")
            .is_some()
    );
}
