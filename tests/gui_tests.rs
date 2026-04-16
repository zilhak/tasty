//! GUI integration tests for tasty.
//!
//! All tests share a single tasty GUI instance. Each test creates its own
//! workspace(s) for isolation — tasty's architecture guarantees that independent
//! workspaces don't interfere with each other.
//!
//! Run with: cargo test --test gui_tests -- --ignored --test-threads=1
//! (single-threaded because only one window can have OS keyboard focus at a time)
//!
//! These tests are ignored by default since they require a display and
//! take focus of the desktop.

mod gui_common;

use std::time::{Duration, Instant};
use enigo::Key;
use gui_common::shared;

/// Maximum acceptable latency for UI operations (milliseconds).
/// This includes ~340ms of intentional sleep in input simulation helpers
/// (focus + key press/release timing), so the effective UI response budget
/// is roughly MAX_UI_RESPONSE_MS - 340ms.
const MAX_UI_RESPONSE_MS: u128 = 1000;

/// Helper: measure how long a UI condition takes to become true after an action.
fn measure_ui_latency<F, C>(
    inst: &mut gui_common::GuiTestInstance,
    action_name: &str,
    action: F,
    condition: C,
) -> Duration
where
    F: FnOnce(&mut gui_common::GuiTestInstance),
    C: Fn(&gui_common::UiState) -> bool,
{
    let start = Instant::now();
    action(inst);
    inst.wait_for_ui(action_name, Duration::from_secs(5), &condition);
    start.elapsed()
}

// ============================================================
// Settings Window Tests
// ============================================================

#[test]
#[ignore]
fn test_settings_open_ctrl_comma() {
    let mut inst = shared();

    // Verify settings is initially closed
    let state = inst.ui_state();
    assert!(!state.settings_open, "settings should be closed initially");

    // Press Ctrl+, to open settings
    inst.press_ctrl(Key::Unicode(','));

    let state = inst.wait_for_ui(
        "settings_open == true",
        Duration::from_secs(3),
        |s| s.settings_open,
    );
    assert!(state.settings_open, "settings should be open after Ctrl+,");

    // Cleanup: close settings
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| !s.settings_open);
}

#[test]
#[ignore]
fn test_settings_close_ctrl_comma() {
    let mut inst = shared();

    // Open settings
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings open", Duration::from_secs(3), |s| s.settings_open);

    // Close with Ctrl+, again (toggle)
    inst.press_ctrl(Key::Unicode(','));

    let state = inst.wait_for_ui(
        "settings_open == false",
        Duration::from_secs(3),
        |s| !s.settings_open,
    );
    assert!(!state.settings_open, "settings should be closed after second Ctrl+,");
}

#[test]
#[ignore]
fn test_settings_close_escape() {
    let mut inst = shared();

    // Open settings
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings open", Duration::from_secs(3), |s| s.settings_open);

    // Close with Escape
    inst.press_key(Key::Escape);

    let state = inst.wait_for_ui(
        "settings closed via escape",
        Duration::from_secs(3),
        |s| !s.settings_open,
    );
    assert!(!state.settings_open, "settings should close with Escape");
}

#[test]
#[ignore]
fn test_settings_open_speed() {
    let mut inst = shared();

    let elapsed = measure_ui_latency(
        &mut inst,
        "settings open speed",
        |i| i.press_ctrl(Key::Unicode(',')),
        |s| s.settings_open,
    );

    println!("Settings open latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Settings open took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| !s.settings_open);
}

// ============================================================
// Notification Panel Tests
// ============================================================

#[test]
#[ignore]
fn test_notification_panel_toggle() {
    let mut inst = shared();

    let state = inst.ui_state();
    assert!(!state.notification_panel_open, "notification panel should be closed initially");

    // Ctrl+Shift+I to open
    inst.press_ctrl_shift(Key::Unicode('i'));

    let state = inst.wait_for_ui(
        "notification panel open",
        Duration::from_secs(3),
        |s| s.notification_panel_open,
    );
    assert!(state.notification_panel_open);

    // Ctrl+Shift+I to close
    inst.press_ctrl_shift(Key::Unicode('i'));

    let state = inst.wait_for_ui(
        "notification panel close",
        Duration::from_secs(3),
        |s| !s.notification_panel_open,
    );
    assert!(!state.notification_panel_open);
}

#[test]
#[ignore]
fn test_notification_panel_close_escape() {
    let mut inst = shared();

    // Open notification panel
    inst.press_ctrl_shift(Key::Unicode('i'));
    inst.wait_for_ui("notification open", Duration::from_secs(3), |s| s.notification_panel_open);

    // Close with Escape
    inst.press_key(Key::Escape);

    let state = inst.wait_for_ui(
        "notification panel close via escape",
        Duration::from_secs(3),
        |s| !s.notification_panel_open,
    );
    assert!(!state.notification_panel_open);
}

#[test]
#[ignore]
fn test_notification_panel_speed() {
    let mut inst = shared();

    let elapsed = measure_ui_latency(
        &mut inst,
        "notification panel open speed",
        |i| i.press_ctrl_shift(Key::Unicode('i')),
        |s| s.notification_panel_open,
    );

    println!("Notification panel open latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Notification panel open took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_ctrl_shift(Key::Unicode('i'));
    inst.wait_for_ui("notification closed", Duration::from_secs(3), |s| !s.notification_panel_open);
}

// ============================================================
// Workspace Tests
// Uses IPC to create workspaces, keyboard to interact.
// ============================================================

#[test]
#[ignore]
fn test_new_workspace_ctrl_shift_n() {
    let mut inst = shared();
    let initial_count = inst.ui_state().workspace_count;

    // Ctrl+Shift+N to create new workspace
    inst.press_ctrl_shift(Key::Unicode('n'));

    let state = inst.wait_for_ui(
        "workspace count increased",
        Duration::from_secs(3),
        |s| s.workspace_count == initial_count + 1,
    );
    assert_eq!(state.workspace_count, initial_count + 1);

    // Cleanup: close the workspace we created (Alt+Shift+W)
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("workspace closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_count);
}

#[test]
#[ignore]
fn test_workspace_switch_alt_number() {
    let mut inst = shared();

    // Create a fresh workspace for this test
    let initial_count = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("new ws", Duration::from_secs(3),
        |s| s.workspace_count == initial_count + 1);

    // Should be on the new workspace (last index)
    let state = inst.ui_state();
    let new_ws_idx = state.active_workspace;

    // Switch to first workspace (Alt+1)
    inst.press_alt(Key::Unicode('1'));
    inst.wait_for_ui("switch to ws 0", Duration::from_secs(3), |s| s.active_workspace == 0);

    // Switch back
    let target = new_ws_idx + 1; // Alt+N is 1-based
    let key = char::from_digit(target as u32, 10).unwrap();
    inst.press_alt(Key::Unicode(key));
    inst.wait_for_ui("switch back", Duration::from_secs(3),
        |s| s.active_workspace == new_ws_idx);

    // Cleanup: close the test workspace
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_count);
}

#[test]
#[ignore]
fn test_workspace_creation_speed() {
    let mut inst = shared();
    let initial_count = inst.ui_state().workspace_count;

    let elapsed = measure_ui_latency(
        &mut inst,
        "workspace creation speed",
        |i| i.press_ctrl_shift(Key::Unicode('n')),
        |s| s.workspace_count == initial_count + 1,
    );

    println!("Workspace creation latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Workspace creation took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_count);
}

// ============================================================
// Tab Tests
// ============================================================

#[test]
#[ignore]
fn test_new_tab_ctrl_shift_t() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    let state = inst.ui_state();
    assert_eq!(state.tab_count, 1, "new workspace should start with 1 tab");

    // Ctrl+Shift+T to create new tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    let state = inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);
    assert_eq!(state.tab_count, 2);

    // Cleanup: close the test workspace
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

#[test]
#[ignore]
fn test_close_tab_ctrl_w() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    // Create a second tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    // Ctrl+W to close the active tab
    inst.press_ctrl(Key::Unicode('w'));
    let state = inst.wait_for_ui("1 tab", Duration::from_secs(3), |s| s.tab_count == 1);
    assert_eq!(state.tab_count, 1);

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

#[test]
#[ignore]
fn test_tab_creation_speed() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    let elapsed = measure_ui_latency(
        &mut inst,
        "tab creation speed",
        |i| i.press_ctrl_shift(Key::Unicode('t')),
        |s| s.tab_count == 2,
    );

    println!("Tab creation latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Tab creation took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

// ============================================================
// Pane Split Tests
// ============================================================

#[test]
#[ignore]
fn test_pane_split_vertical_ctrl_shift_e() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    assert_eq!(inst.ui_state().pane_count, 1);

    // Ctrl+Shift+E for vertical pane split
    inst.press_ctrl_shift(Key::Unicode('e'));
    let state = inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);
    assert_eq!(state.pane_count, 2);

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

#[test]
#[ignore]
fn test_pane_split_horizontal_ctrl_shift_o() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    // Ctrl+Shift+O for horizontal pane split
    inst.press_ctrl_shift(Key::Unicode('o'));
    let state = inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);
    assert_eq!(state.pane_count, 2);

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

#[test]
#[ignore]
fn test_close_pane_ctrl_shift_w() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    // Split first
    inst.press_ctrl_shift(Key::Unicode('e'));
    inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);

    // Close the active pane
    inst.press_ctrl_shift(Key::Unicode('w'));
    let state = inst.wait_for_ui("1 pane", Duration::from_secs(3), |s| s.pane_count == 1);
    assert_eq!(state.pane_count, 1);

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

#[test]
#[ignore]
fn test_pane_split_speed() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    let elapsed = measure_ui_latency(
        &mut inst,
        "pane split speed",
        |i| i.press_ctrl_shift(Key::Unicode('e')),
        |s| s.pane_count == 2,
    );

    println!("Pane split latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Pane split took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

// ============================================================
// Keyboard Routing Tests
// ============================================================

#[test]
#[ignore]
fn test_keyboard_not_sent_to_terminal_when_settings_open() {
    let mut inst = shared();

    // Set a mark so we can check terminal output
    inst.call("surface.set_mark", serde_json::json!({}));

    // Open settings
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings open", Duration::from_secs(3), |s| s.settings_open);

    // Type some text — should NOT reach the terminal
    inst.type_text("hello_should_not_appear");
    std::thread::sleep(Duration::from_millis(500));

    // Check terminal did not receive the text
    let result = inst.call(
        "surface.read_since_mark",
        serde_json::json!({ "strip_ansi": true }),
    );
    let output = result["text"].as_str().unwrap_or("");
    assert!(
        !output.contains("hello_should_not_appear"),
        "Terminal should NOT receive keyboard input when settings is open. Got: {}",
        output,
    );

    // Cleanup: close settings
    inst.press_key(Key::Escape);
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| !s.settings_open);
}

#[test]
#[ignore]
fn test_keyboard_sent_to_terminal_when_no_overlay() {
    let mut inst = shared();

    // Create a test workspace so we have a clean terminal
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    // Wait for shell to be ready
    std::thread::sleep(Duration::from_millis(500));

    // Set mark
    inst.call("surface.set_mark", serde_json::json!({}));

    // Type some text
    inst.type_text("echo gui_test_marker");
    inst.press_key(Key::Return);

    // Wait for the output to appear
    std::thread::sleep(Duration::from_millis(1000));

    let result = inst.call(
        "surface.read_since_mark",
        serde_json::json!({ "strip_ansi": true }),
    );
    let output = result["text"].as_str().unwrap_or("");
    assert!(
        output.contains("gui_test_marker"),
        "Terminal should receive keyboard input when no overlay is open. Got: {}",
        output,
    );

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

// ============================================================
// Settings Window Interaction Tests
// ============================================================

#[test]
#[ignore]
fn test_settings_window_is_interactive() {
    let mut inst = shared();

    // Open settings
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings open", Duration::from_secs(3), |s| s.settings_open);

    // Rapid toggle to verify interactivity
    std::thread::sleep(Duration::from_millis(300));

    // Close
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| !s.settings_open);

    // Open again
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings open again", Duration::from_secs(3), |s| s.settings_open);

    // Verify still responsive
    let state = inst.ui_state();
    assert!(state.settings_open, "settings should still be open after rapid toggle");

    // Cleanup
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| !s.settings_open);
}

// ============================================================
// Combined Workflow Tests
// ============================================================

#[test]
#[ignore]
fn test_full_workflow_workspace_pane_tab() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    // Start: 1 pane, 1 tab in new workspace
    let state = inst.ui_state();
    assert_eq!(state.pane_count, 1);
    assert_eq!(state.tab_count, 1);

    // Create new tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    // Split pane
    inst.press_ctrl_shift(Key::Unicode('e'));
    inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);

    // Create another workspace
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws+1", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 2);

    // Switch back to previous workspace
    let prev_ws_key = char::from_digit((initial_ws + 1) as u32, 10).unwrap();
    inst.press_alt(Key::Unicode(prev_ws_key));
    inst.wait_for_ui("switch back", Duration::from_secs(3),
        |s| s.active_workspace == initial_ws);

    // Verify workspace still has 2 panes
    let state = inst.ui_state();
    assert_eq!(state.pane_count, 2, "workspace should still have 2 panes");

    // Close pane
    inst.press_ctrl_shift(Key::Unicode('w'));
    inst.wait_for_ui("1 pane", Duration::from_secs(3), |s| s.pane_count == 1);

    // Close tab
    inst.press_ctrl(Key::Unicode('w'));
    inst.wait_for_ui("1 tab", Duration::from_secs(3), |s| s.tab_count == 1);

    // Cleanup: close both test workspaces
    // Close current workspace
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws-1", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);
    // Close the other test workspace (now active)
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws restored", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

// ============================================================
// Performance / Speed Tests
// ============================================================

#[test]
#[ignore]
fn test_settings_toggle_speed_repeated() {
    let mut inst = shared();

    let mut latencies = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        inst.press_ctrl(Key::Unicode(','));
        inst.wait_for_ui("settings toggled", Duration::from_secs(3), |s| s.settings_open);
        latencies.push(start.elapsed());

        let start = Instant::now();
        inst.press_ctrl(Key::Unicode(','));
        inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| !s.settings_open);
        latencies.push(start.elapsed());
    }

    let avg_ms = latencies.iter().map(|d| d.as_millis()).sum::<u128>() / latencies.len() as u128;
    let max_ms = latencies.iter().map(|d| d.as_millis()).max().unwrap_or(0);

    println!("Settings toggle: avg={}ms, max={}ms over {} iterations", avg_ms, max_ms, latencies.len());

    assert!(
        max_ms < MAX_UI_RESPONSE_MS,
        "Settings toggle max latency {}ms exceeds {}ms limit",
        max_ms,
        MAX_UI_RESPONSE_MS,
    );
}

#[test]
#[ignore]
fn test_workspace_switch_speed() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    let ws1_key = char::from_digit(initial_ws as u32 + 1, 10).unwrap();
    let ws0_key = char::from_digit(initial_ws as u32, 10).unwrap_or('1');

    let mut latencies = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        inst.press_alt(Key::Unicode(ws0_key));
        inst.wait_for_ui("ws 0", Duration::from_secs(3),
            |s| s.active_workspace == (initial_ws.saturating_sub(1)).max(0));
        latencies.push(start.elapsed());

        let start = Instant::now();
        inst.press_alt(Key::Unicode(ws1_key));
        inst.wait_for_ui("ws 1", Duration::from_secs(3),
            |s| s.active_workspace == initial_ws);
        latencies.push(start.elapsed());
    }

    let avg_ms = latencies.iter().map(|d| d.as_millis()).sum::<u128>() / latencies.len() as u128;
    let max_ms = latencies.iter().map(|d| d.as_millis()).max().unwrap_or(0);

    println!("Workspace switch: avg={}ms, max={}ms over {} iterations", avg_ms, max_ms, latencies.len());

    assert!(
        max_ms < MAX_UI_RESPONSE_MS,
        "Workspace switch max latency {}ms exceeds {}ms limit",
        max_ms,
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}

#[test]
#[ignore]
fn test_tab_switch_speed() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws + 1);

    // Create a second tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    let mut latencies = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        inst.press_ctrl(Key::Tab);
        std::thread::sleep(Duration::from_millis(100));
        latencies.push(start.elapsed());

        let start = Instant::now();
        inst.press_ctrl_shift(Key::Tab);
        std::thread::sleep(Duration::from_millis(100));
        latencies.push(start.elapsed());
    }

    let avg_ms = latencies.iter().map(|d| d.as_millis()).sum::<u128>() / latencies.len() as u128;
    let max_ms = latencies.iter().map(|d| d.as_millis()).max().unwrap_or(0);

    println!("Tab switch: avg={}ms, max={}ms over {} iterations", avg_ms, max_ms, latencies.len());

    assert!(
        max_ms < MAX_UI_RESPONSE_MS,
        "Tab switch max latency {}ms exceeds {}ms limit",
        max_ms,
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3),
        |s| s.workspace_count == initial_ws);
}
