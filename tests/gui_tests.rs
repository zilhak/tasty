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

use enigo::Key;
use gui_common::shared;
use serde_json::json;
use std::time::{Duration, Instant};

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

    let state = inst.wait_for_ui("settings_open == true", Duration::from_secs(3), |s| {
        s.settings_open
    });
    assert!(state.settings_open, "settings should be open after Ctrl+,");

    // Cleanup: close settings
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| {
        !s.settings_open
    });
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

    let state = inst.wait_for_ui("settings_open == false", Duration::from_secs(3), |s| {
        !s.settings_open
    });
    assert!(
        !state.settings_open,
        "settings should be closed after second Ctrl+,"
    );
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

    let state = inst.wait_for_ui("settings closed via escape", Duration::from_secs(3), |s| {
        !s.settings_open
    });
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
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| {
        !s.settings_open
    });
}

// ============================================================
// Notification Panel Tests
// ============================================================

#[test]
#[ignore]
fn test_notification_panel_toggle() {
    let mut inst = shared();

    let state = inst.ui_state();
    assert!(
        !state.notification_panel_open,
        "notification panel should be closed initially"
    );

    // Ctrl+Shift+I to open
    inst.press_ctrl_shift(Key::Unicode('i'));

    let state = inst.wait_for_ui("notification panel open", Duration::from_secs(3), |s| {
        s.notification_panel_open
    });
    assert!(state.notification_panel_open);

    // Ctrl+Shift+I to close
    inst.press_ctrl_shift(Key::Unicode('i'));

    let state = inst.wait_for_ui("notification panel close", Duration::from_secs(3), |s| {
        !s.notification_panel_open
    });
    assert!(!state.notification_panel_open);
}

#[test]
#[ignore]
fn test_notification_panel_close_escape() {
    let mut inst = shared();

    // Open notification panel
    inst.press_ctrl_shift(Key::Unicode('i'));
    inst.wait_for_ui("notification open", Duration::from_secs(3), |s| {
        s.notification_panel_open
    });

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
    inst.wait_for_ui("notification closed", Duration::from_secs(3), |s| {
        !s.notification_panel_open
    });
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

    let state = inst.wait_for_ui("workspace count increased", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count + 1
    });
    assert_eq!(state.workspace_count, initial_count + 1);

    // Cleanup: close the workspace we created (Alt+Shift+W)
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("workspace closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count
    });
}

#[test]
#[ignore]
fn test_workspace_switch_alt_number() {
    let mut inst = shared();

    // Create a fresh workspace for this test
    let initial_count = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("new ws", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count + 1
    });

    // Should be on the new workspace (last index)
    let state = inst.ui_state();
    let new_ws_idx = state.active_workspace;

    // Switch to first workspace (Alt+1)
    inst.press_alt(Key::Unicode('1'));
    inst.wait_for_ui("switch to ws 0", Duration::from_secs(3), |s| {
        s.active_workspace == 0
    });

    // Switch back
    let target = new_ws_idx + 1; // Alt+N is 1-based
    let key = char::from_digit(target as u32, 10).unwrap();
    inst.press_alt(Key::Unicode(key));
    inst.wait_for_ui("switch back", Duration::from_secs(3), |s| {
        s.active_workspace == new_ws_idx
    });

    // Cleanup: close the test workspace
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count
    });
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
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count
    });
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
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    let state = inst.ui_state();
    assert_eq!(state.tab_count, 1, "new workspace should start with 1 tab");

    // Ctrl+Shift+T to create new tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    let state = inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);
    assert_eq!(state.tab_count, 2);

    // Cleanup: close the test workspace
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_close_tab_ctrl_w() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    // Create a second tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    // Ctrl+W to close the active tab
    inst.press_ctrl(Key::Unicode('w'));
    let state = inst.wait_for_ui("1 tab", Duration::from_secs(3), |s| s.tab_count == 1);
    assert_eq!(state.tab_count, 1);

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_tab_creation_speed() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

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
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
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
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    assert_eq!(inst.ui_state().pane_count, 1);

    // Ctrl+Shift+E for vertical pane split
    inst.press_ctrl_shift(Key::Unicode('e'));
    let state = inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);
    assert_eq!(state.pane_count, 2);

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_pane_split_horizontal_ctrl_shift_o() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    // Ctrl+Shift+O for horizontal pane split
    inst.press_ctrl_shift(Key::Unicode('o'));
    let state = inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);
    assert_eq!(state.pane_count, 2);

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_close_pane_ctrl_shift_w() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    // Split first
    inst.press_ctrl_shift(Key::Unicode('e'));
    inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);

    // Close the active pane
    inst.press_ctrl_shift(Key::Unicode('w'));
    let state = inst.wait_for_ui("1 pane", Duration::from_secs(3), |s| s.pane_count == 1);
    assert_eq!(state.pane_count, 1);

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_pane_split_speed() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

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
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
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
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| {
        !s.settings_open
    });
}

#[test]
#[ignore]
fn test_keyboard_sent_to_terminal_when_no_overlay() {
    let mut inst = shared();

    // Create a test workspace so we have a clean terminal
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

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
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
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
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| {
        !s.settings_open
    });

    // Open again
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings open again", Duration::from_secs(3), |s| {
        s.settings_open
    });

    // Verify still responsive
    let state = inst.ui_state();
    assert!(
        state.settings_open,
        "settings should still be open after rapid toggle"
    );

    // Cleanup
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| {
        !s.settings_open
    });
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
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

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
    inst.wait_for_ui("ws+1", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 2
    });

    // Switch back to previous workspace
    let prev_ws_key = char::from_digit((initial_ws + 1) as u32, 10).unwrap();
    inst.press_alt(Key::Unicode(prev_ws_key));
    inst.wait_for_ui("switch back", Duration::from_secs(3), |s| {
        s.active_workspace == initial_ws
    });

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
    inst.wait_for_ui("ws-1", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });
    // Close the other test workspace (now active)
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws restored", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
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
        inst.wait_for_ui("settings toggled", Duration::from_secs(3), |s| {
            s.settings_open
        });
        latencies.push(start.elapsed());

        let start = Instant::now();
        inst.press_ctrl(Key::Unicode(','));
        inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| {
            !s.settings_open
        });
        latencies.push(start.elapsed());
    }

    let avg_ms = latencies.iter().map(|d| d.as_millis()).sum::<u128>() / latencies.len() as u128;
    let max_ms = latencies.iter().map(|d| d.as_millis()).max().unwrap_or(0);

    println!(
        "Settings toggle: avg={}ms, max={}ms over {} iterations",
        avg_ms,
        max_ms,
        latencies.len()
    );

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
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    let ws1_key = char::from_digit(initial_ws as u32 + 1, 10).unwrap();
    let ws0_key = char::from_digit(initial_ws as u32, 10).unwrap_or('1');

    let mut latencies = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        inst.press_alt(Key::Unicode(ws0_key));
        inst.wait_for_ui("ws 0", Duration::from_secs(3), |s| {
            s.active_workspace == (initial_ws.saturating_sub(1))
        });
        latencies.push(start.elapsed());

        let start = Instant::now();
        inst.press_alt(Key::Unicode(ws1_key));
        inst.wait_for_ui("ws 1", Duration::from_secs(3), |s| {
            s.active_workspace == initial_ws
        });
        latencies.push(start.elapsed());
    }

    let avg_ms = latencies.iter().map(|d| d.as_millis()).sum::<u128>() / latencies.len() as u128;
    let max_ms = latencies.iter().map(|d| d.as_millis()).max().unwrap_or(0);

    println!(
        "Workspace switch: avg={}ms, max={}ms over {} iterations",
        avg_ms,
        max_ms,
        latencies.len()
    );

    assert!(
        max_ms < MAX_UI_RESPONSE_MS,
        "Workspace switch max latency {}ms exceeds {}ms limit",
        max_ms,
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_tab_switch_speed() {
    let mut inst = shared();

    // Create a test workspace
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

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

    println!(
        "Tab switch: avg={}ms, max={}ms over {} iterations",
        avg_ms,
        max_ms,
        latencies.len()
    );

    assert!(
        max_ms < MAX_UI_RESPONSE_MS,
        "Tab switch max latency {}ms exceeds {}ms limit",
        max_ms,
        MAX_UI_RESPONSE_MS,
    );

    // Cleanup
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

// ============================================================
// IME composition + shortcut flush/clear (hybrid: IPC preedit + enigo shortcut)
// ============================================================
//
// handle_keyboard_input 6단계의 flush/clear 분기를 검출한다. 진짜 OS IME 조합
// 이벤트열은 enigo(SendInput/KEYEVENTF_UNICODE)가 OS IME 를 우회하므로 자동 재현
// 불가하지만, "조합 중(preedit 존재) 상태에서 단축키를 누른" 상황은 하이브리드로
// 재현된다: preedit 는 IPC(surface.ime_preedit)로 윈도우 state 에 직접 세팅하고,
// 단축키만 enigo 실입력으로 handle_keyboard_input 을 태운다.
//
// 분기 조건은 `handle_shortcut` 소비 직후 `popups.has_focused()`.
//   flush = preedit.text 를 PTY 로 확정 전송 (팝업 포커스 없음).
//   clear = preedit 폐기, PTY 미전송 (팝업이 포커스를 가짐).
// dispatch_intent 는 큐잉(지연 적용)이라, intent 로 여는 팝업(command palette/
// notifications)은 이 체크 시점에 아직 focused 가 아니다 → flush 로 떨어진다.
// 동기적으로 has_focused()==true 가 되는 유일한 경로는 `find` 가 "이미 열려 있으나
// 비포커스인" search_bar 를 set_focused 로 재포커스하는 경우다. 아래 clear 테스트가
// 정확히 그 상태(ctrl+f 로 열고 → 터미널 클릭으로 unfocus)를 구성한다.

/// flush 경로: 조합 중 팝업을 열지 않는 단축키(sidebar collapse)를 누르면
/// preedit 이 PTY 로 확정 전송된다.
#[test]
#[ignore]
fn test_ime_preedit_flushed_on_non_popup_shortcut() {
    let mut inst = shared();
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });
    std::thread::sleep(Duration::from_millis(500)); // shell ready

    // 포커스된 터미널에 preedit "한" 을 IPC 로 세팅 (응답에 surface_id 포함).
    let pre = inst.call("surface.ime_preedit", serde_json::json!({ "text": "한" }));
    let sid = pre["surface_id"]
        .as_u64()
        .expect("preedit response surface_id");
    assert_eq!(pre["preedit_active"], serde_json::json!(true));

    // preedit 은 화면에 에코되지 않으므로, mark 이후 read 는 단축키가 보낸 것만 잡는다.
    inst.call("surface.set_mark", serde_json::json!({ "surface_id": sid }));

    // 팝업을 열지 않는 소비형 단축키(ctrl+b = toggle_sidebar_collapse) → flush.
    inst.press_ctrl(Key::Unicode('b'));
    std::thread::sleep(Duration::from_millis(500));

    let out = inst.call(
        "surface.read_since_mark",
        serde_json::json!({ "surface_id": sid, "strip_ansi": true }),
    );
    let text = out["text"].as_str().unwrap_or("");
    assert!(
        text.contains("한"),
        "flush: 조합 중 문자는 단축키 시 PTY 로 확정 전송돼야 한다. Got: {text:?}"
    );
    let status = inst.call("surface.ime_status", serde_json::json!({}));
    assert_eq!(
        status["has_preedit"],
        serde_json::json!(false),
        "flush 후 preedit 은 비워져야 한다"
    );

    // Cleanup: 사이드바 원복 + 워크스페이스 닫기.
    inst.press_ctrl(Key::Unicode('b'));
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

/// clear 경로: 조합 중 "팝업을 포커스시키는" 단축키를 누르면 preedit 이 폐기되고
/// PTY 로 전송되지 않는다. 동기 재포커스가 일어나도록 search_bar 를 열고 → 터미널
/// 클릭으로 unfocus 한 뒤(닫히지 않음) → find 를 다시 눌러 set_focused 를 태운다.
#[test]
#[ignore]
fn test_ime_preedit_cleared_on_popup_focus_shortcut() {
    let mut inst = shared();
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });
    std::thread::sleep(Duration::from_millis(500));

    // search_bar 를 연다(포커스됨). 이어 터미널을 클릭해 unfocus (열린 채 유지 —
    // search_bar 는 close_on_outside_click=false, sticky_focus=false).
    inst.press_ctrl(Key::Unicode('f'));
    std::thread::sleep(Duration::from_millis(300));
    let (w, h) = inst.client_size();
    inst.click_at(w / 2, h * 3 / 4); // 상단 앵커 search_bar 를 피해 터미널 영역 클릭
    std::thread::sleep(Duration::from_millis(200));

    // 이제 터미널 포커스 + search_bar 열림·비포커스. preedit + mark 세팅.
    let pre = inst.call("surface.ime_preedit", serde_json::json!({ "text": "한" }));
    let sid = pre["surface_id"]
        .as_u64()
        .expect("preedit response surface_id");
    inst.call("surface.set_mark", serde_json::json!({ "surface_id": sid }));

    // find 재입력 → 열린 search_bar 를 동기 재포커스 → has_focused()==true → clear.
    inst.press_ctrl(Key::Unicode('f'));
    std::thread::sleep(Duration::from_millis(500));

    let out = inst.call(
        "surface.read_since_mark",
        serde_json::json!({ "surface_id": sid, "strip_ansi": true }),
    );
    let text = out["text"].as_str().unwrap_or("");
    assert!(
        !text.contains("한"),
        "clear: 조합 중 문자는 팝업 포커스 단축키 시 PTY 로 가면 안 된다. Got: {text:?}"
    );
    let status = inst.call("surface.ime_status", serde_json::json!({}));
    assert_eq!(status["has_preedit"], serde_json::json!(false));

    // Cleanup: search_bar 닫기 + 워크스페이스 닫기.
    inst.press_key(Key::Escape);
    inst.press_alt(Key::Unicode('W'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

// ═══════════════════ mouse-routing injection net (구 mouse_routing_tests.rs 병합 — 테스트 다이어트) ═══════════════════
// Mouse-routing injection regression net for `handle_mouse_input`.
//
// `handle_mouse_input` 의 좌표/라우팅 결정 수학은 이미 순수 함수(단위테스트)로
// 격리돼 있다. 이 net 이 잡는 것은 순수테스트가 못 잡는 부분 — "블록→메서드
// 추출이 분기 순서·가드·early-return 을 보존했는가" 하는 stateful 라우팅이다.
//
// 실제 데스크톱 마우스를 뺏지 않고 IPC(`debug.inject_window_mouse`)로 winit
// 레벨 포인터 이벤트를 주입해 실제 `handle_mouse_input` 을 헤드리스 구동하고,
// read-only debug IPC(`debug.selection`/`debug.pending_menu`/`debug.focused_surface`)
// 로 라우팅 결과를 단언한다 (원칙 1·3: 사용자 입력 재현은 debug 격리).
//
// 대상은 focused 테스트 윈도우의 **active workspace** surface 다 — 주입 좌표가
// 보이는 레이아웃에 닿아야 `surface_rect_by_id` 가 해소되기 때문. IPC 로 만든
// workspace 는 active 전환이 없으므로(포커스 독립) 여기서는 쓰지 않는다.
//
// Run with: cargo test --test mouse_routing_tests -- --ignored --test-threads=1
// (display 필요, single-thread — 한 윈도우만 OS 포커스를 가질 수 있으므로.)

/// 주입 후 GUI 상태가 정착할 시간. inject IPC 자체는 동기지만 여유를 둔다.
fn settle() {
    std::thread::sleep(Duration::from_millis(150));
}

/// (a) click-to-activate: 비활성 surface 를 좌클릭(press)하면 포커스가 그 surface
/// 로 전환된다. surface-level split 으로 active workspace 에 2 번째 surface 를 만든
/// 뒤(IPC split 은 focus 미이동), 비활성 surface 중앙을 press+release 한다.
#[test]
#[ignore]
fn click_to_activate_moves_focus() {
    let inst = shared();

    let focused_before = inst
        .debug_focused_surface()
        .expect("an initially focused surface");

    // active workspace 에 2 번째 surface 생성 (같은 pane 내 surface split).
    let res = inst.call(
        "split",
        json!({
            "level": "surface",
            "direction": "vertical",
            "target_surface": focused_before,
        }),
    );
    let new_sid = res["new_surface_id"]
        .as_u64()
        .expect("split should return new_surface_id");
    settle();

    // IPC split 은 focus 를 옮기지 않는다 — 새 surface 는 비활성.
    assert_ne!(
        inst.debug_focused_surface(),
        Some(new_sid),
        "IPC split must not move focus (focus independence)"
    );

    // 비활성 surface 중앙을 좌클릭 → click-to-activate 가 포커스를 전환.
    inst.inject_mouse(new_sid, 0.5, 0.5, "press", 0);
    inst.inject_mouse(new_sid, 0.5, 0.5, "release", 0);
    settle();

    assert_eq!(
        inst.debug_focused_surface(),
        Some(new_sid),
        "click-to-activate should move focus to the clicked surface"
    );

    // cleanup: 생성한 surface 정리.
    inst.call("surface.close", json!({ "surface_id": new_sid }));
    settle();
}

/// (b) 로컬 드래그 선택: 트래킹 OFF 터미널에서 press→move→move→release 하면
/// 로컬 텍스트 선택이 생긴다 (start≠end, 드래그 종료 후 dragging=false).
#[test]
#[ignore]
fn drag_creates_local_selection() {
    let inst = shared();

    let sid = inst
        .debug_focused_surface()
        .expect("a focused terminal surface");

    inst.inject_mouse(sid, 0.3, 0.3, "press", 0);
    inst.inject_mouse(sid, 0.5, 0.4, "move", 0);
    inst.inject_mouse(sid, 0.7, 0.6, "move", 0);
    inst.inject_mouse(sid, 0.7, 0.6, "release", 0);
    settle();

    let sel = inst.debug_selection();
    assert_eq!(sel["present"], json!(true), "selection should be present");
    assert_eq!(
        sel["empty"],
        json!(false),
        "drag selection should not be empty"
    );
    assert_eq!(
        sel["dragging"],
        json!(false),
        "dragging must be false after release"
    );
    assert_eq!(
        sel["surface_id"].as_u64(),
        Some(sid),
        "selection surface_id should match the injected surface"
    );
    // 드래그가 실제로 범위를 만들었는지 — start != end.
    assert_ne!(
        (sel["start"]["col"].clone(), sel["start"]["row"].clone()),
        (sel["end"]["col"].clone(), sel["end"]["row"].clone()),
        "selection start and end should differ"
    );
}

/// (c) 우클릭 컨텍스트 메뉴: 트래킹 OFF 터미널을 우클릭(press)하면 tasty
/// 터미널 컨텍스트 메뉴가 대기 상태로 세워진다 (kind=TerminalSurface).
#[test]
#[ignore]
fn right_click_opens_terminal_menu() {
    let inst = shared();

    let sid = inst
        .debug_focused_surface()
        .expect("a focused terminal surface");

    inst.inject_mouse(sid, 0.5, 0.5, "press", 2);
    settle();

    let menu = inst.debug_pending_menu();
    assert_eq!(
        menu["present"],
        json!(true),
        "a context menu should be pending"
    );
    assert_eq!(
        menu["kind"],
        json!("TerminalSurface"),
        "right-click on a terminal should open the TerminalSurface menu"
    );
    assert_eq!(
        menu["surface_id"].as_u64(),
        Some(sid),
        "menu surface_id should match the injected surface"
    );
}

/// (d) explorer 우클릭은 표면 어디서든 explorer 메뉴가 뜨고, generic surface
/// fallback("터미널 ID 복사")이 새지 않는다. 그리드 콘텐츠뿐 아니라 chrome
/// (툴바/내부 탭바/상태줄/빈 사이드바)까지 `draw_explorer` 의 표면 전체 catch-all 이
/// Empty 메뉴로 흡수하는 회귀를 잡는다(불가침 원칙 §1·§2). 좌표는 surface 상대
/// 정규화라 창 크기와 무관. egui 경로 주입(`inject_egui_mouse`)으로 위젯
/// `secondary_clicked` 라우팅을 그대로 탄다.
#[test]
#[ignore]
fn right_click_explorer_never_falls_back_to_surface_menu() {
    let inst = shared();

    // active workspace 의 pane 에 grid explorer 를 만들어 활성 탭으로 렌더시킨다.
    let pane_id = inst.first_pane_id();
    let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"));
    let mut params = json!({ "pane_id": pane_id, "type": "explorer", "view_mode": "grid" });
    if let Ok(h) = home {
        params["path"] = json!(h);
    }
    let created = inst.call("tab.create", params);
    let sid = created["surface_id"]
        .as_u64()
        .expect("tab.create should return the explorer surface_id");
    settle();

    // surface 상대 좌표(fx,fy ∈ [0,1]): 이전에 surface fallback 이 새던 chrome 영역들 +
    // 콘텐츠 그리드. 전부 explorer 메뉴여야 한다.
    let spots = [
        (0.5_f32, 0.30_f32, "content grid"),
        (0.5, 0.02, "internal tab bar / toolbar (top)"),
        (0.5, 0.99, "status line (bottom)"),
        (0.02, 0.60, "left sidebar empty area"),
    ];
    for (fx, fy, label) in spots {
        inst.inject_egui_mouse(sid, fx, fy, "move", 2);
        inst.inject_egui_mouse(sid, fx, fy, "press", 2);
        inst.inject_egui_mouse(sid, fx, fy, "release", 2);
        settle();
        let menu = inst.debug_pending_menu();
        assert_eq!(
            menu["present"],
            json!(true),
            "explorer right-click at {label} should set a pending menu"
        );
        assert_eq!(
            menu["kind"],
            json!("Explorer"),
            "explorer right-click at {label} must open the Explorer menu, \
             never the generic Surface fallback"
        );
        assert_eq!(
            menu["surface_id"].as_u64(),
            Some(sid),
            "menu surface_id at {label} should match the explorer surface"
        );
    }
}
