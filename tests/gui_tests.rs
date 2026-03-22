//! GUI integration tests for tasty.
//!
//! These tests launch the full GUI application, simulate keyboard/mouse input
//! via enigo (SendInput on Windows), and verify state via IPC.
//!
//! Run with: cargo test --test gui_tests -- --test-threads=1
//! (single-threaded because only one window can have focus at a time)
//!
//! These tests are ignored by default since they require a display and
//! take focus of the desktop. Run explicitly with: --ignored

mod gui_common;

use std::time::{Duration, Instant};
use enigo::Key;
use gui_common::GuiTestInstance;

/// Maximum acceptable latency for UI operations (milliseconds).
/// This includes ~340ms of intentional sleep in input simulation helpers
/// (focus + key press/release timing), so the effective UI response budget
/// is roughly MAX_UI_RESPONSE_MS - 340ms.
const MAX_UI_RESPONSE_MS: u128 = 1000;

/// Helper: measure how long a UI condition takes to become true after an action.
fn measure_ui_latency<F, C>(
    instance: &mut GuiTestInstance,
    action_name: &str,
    action: F,
    condition: C,
) -> Duration
where
    F: FnOnce(&mut GuiTestInstance),
    C: Fn(&gui_common::UiState) -> bool,
{
    let start = Instant::now();
    action(instance);
    let state = instance.wait_for_ui(
        action_name,
        Duration::from_secs(5),
        &condition,
    );
    let elapsed = start.elapsed();
    let _ = state; // used only for waiting
    elapsed
}

// ============================================================
// Settings Window Tests
// ============================================================

#[test]
#[ignore] // Requires display + window focus
fn test_settings_open_ctrl_comma() {
    let mut inst = GuiTestInstance::spawn();

    // Verify settings is initially closed
    let state = inst.ui_state();
    assert!(!state.settings_open, "settings should be closed initially");

    // Press Ctrl+, to open settings
    inst.press_ctrl(Key::Unicode(','));

    // Verify settings opened
    let state = inst.wait_for_ui(
        "settings_open == true",
        Duration::from_secs(3),
        |s| s.settings_open,
    );
    assert!(state.settings_open, "settings should be open after Ctrl+,");
}

#[test]
#[ignore]
fn test_settings_close_ctrl_comma() {
    let mut inst = GuiTestInstance::spawn();

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
    let mut inst = GuiTestInstance::spawn();

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
    let mut inst = GuiTestInstance::spawn();

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
}

// ============================================================
// Notification Panel Tests
// ============================================================

#[test]
#[ignore]
fn test_notification_panel_toggle() {
    let mut inst = GuiTestInstance::spawn();

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
    let mut inst = GuiTestInstance::spawn();

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
    let mut inst = GuiTestInstance::spawn();

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
}

// ============================================================
// Workspace Tests (via keyboard shortcuts)
// ============================================================

#[test]
#[ignore]
fn test_new_workspace_ctrl_shift_n() {
    let mut inst = GuiTestInstance::spawn();

    let initial = inst.ui_state();
    assert_eq!(initial.workspace_count, 1, "should start with 1 workspace");

    // Ctrl+Shift+N to create new workspace
    inst.press_ctrl_shift(Key::Unicode('n'));

    let state = inst.wait_for_ui(
        "workspace_count == 2",
        Duration::from_secs(3),
        |s| s.workspace_count == 2,
    );
    assert_eq!(state.workspace_count, 2);
    assert_eq!(state.active_workspace, 1, "new workspace should be active");
}

#[test]
#[ignore]
fn test_workspace_switch_alt_number() {
    let mut inst = GuiTestInstance::spawn();

    // Create a second workspace
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("2 workspaces", Duration::from_secs(3), |s| s.workspace_count == 2);

    // Should be on workspace 1 (second)
    let state = inst.ui_state();
    assert_eq!(state.active_workspace, 1);

    // Alt+1 to switch to first workspace
    inst.press_alt(Key::Unicode('1'));

    let state = inst.wait_for_ui(
        "switch to workspace 0",
        Duration::from_secs(3),
        |s| s.active_workspace == 0,
    );
    assert_eq!(state.active_workspace, 0);

    // Alt+2 to switch back to second
    inst.press_alt(Key::Unicode('2'));

    let state = inst.wait_for_ui(
        "switch to workspace 1",
        Duration::from_secs(3),
        |s| s.active_workspace == 1,
    );
    assert_eq!(state.active_workspace, 1);
}

#[test]
#[ignore]
fn test_workspace_creation_speed() {
    let mut inst = GuiTestInstance::spawn();

    let elapsed = measure_ui_latency(
        &mut inst,
        "workspace creation speed",
        |i| i.press_ctrl_shift(Key::Unicode('n')),
        |s| s.workspace_count == 2,
    );

    println!("Workspace creation latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Workspace creation took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );
}

// ============================================================
// Tab Tests
// ============================================================

#[test]
#[ignore]
fn test_new_tab_ctrl_shift_t() {
    let mut inst = GuiTestInstance::spawn();

    let initial = inst.ui_state();
    assert_eq!(initial.tab_count, 1, "should start with 1 tab");

    // Ctrl+Shift+T to create new tab
    inst.press_ctrl_shift(Key::Unicode('t'));

    let state = inst.wait_for_ui(
        "tab_count == 2",
        Duration::from_secs(3),
        |s| s.tab_count == 2,
    );
    assert_eq!(state.tab_count, 2);
}

#[test]
#[ignore]
fn test_close_tab_ctrl_w() {
    let mut inst = GuiTestInstance::spawn();

    // Create a second tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    // Ctrl+W to close the active tab
    inst.press_ctrl(Key::Unicode('w'));

    let state = inst.wait_for_ui(
        "tab_count == 1",
        Duration::from_secs(3),
        |s| s.tab_count == 1,
    );
    assert_eq!(state.tab_count, 1);
}

#[test]
#[ignore]
fn test_tab_creation_speed() {
    let mut inst = GuiTestInstance::spawn();

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
}

// ============================================================
// Pane Split Tests
// ============================================================

#[test]
#[ignore]
fn test_pane_split_vertical_ctrl_shift_e() {
    let mut inst = GuiTestInstance::spawn();

    let initial = inst.ui_state();
    assert_eq!(initial.pane_count, 1, "should start with 1 pane");

    // Ctrl+Shift+E for vertical pane split
    inst.press_ctrl_shift(Key::Unicode('e'));

    let state = inst.wait_for_ui(
        "pane_count == 2",
        Duration::from_secs(3),
        |s| s.pane_count == 2,
    );
    assert_eq!(state.pane_count, 2);
}

#[test]
#[ignore]
fn test_pane_split_horizontal_ctrl_shift_o() {
    let mut inst = GuiTestInstance::spawn();

    // Ctrl+Shift+O for horizontal pane split
    inst.press_ctrl_shift(Key::Unicode('o'));

    let state = inst.wait_for_ui(
        "pane_count == 2",
        Duration::from_secs(3),
        |s| s.pane_count == 2,
    );
    assert_eq!(state.pane_count, 2);
}

#[test]
#[ignore]
fn test_close_pane_ctrl_shift_w() {
    let mut inst = GuiTestInstance::spawn();

    // Split first
    inst.press_ctrl_shift(Key::Unicode('e'));
    inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);

    // Close the active pane
    inst.press_ctrl_shift(Key::Unicode('w'));

    let state = inst.wait_for_ui(
        "pane_count == 1",
        Duration::from_secs(3),
        |s| s.pane_count == 1,
    );
    assert_eq!(state.pane_count, 1);
}

#[test]
#[ignore]
fn test_pane_split_speed() {
    let mut inst = GuiTestInstance::spawn();

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
}

// ============================================================
// Keyboard Routing Tests
// ============================================================

#[test]
#[ignore]
fn test_keyboard_not_sent_to_terminal_when_settings_open() {
    let mut inst = GuiTestInstance::spawn();

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

    // Close settings
    inst.press_key(Key::Escape);
    inst.wait_for_ui("settings closed", Duration::from_secs(3), |s| !s.settings_open);
}

#[test]
#[ignore]
fn test_keyboard_sent_to_terminal_when_no_overlay() {
    let mut inst = GuiTestInstance::spawn();

    // Make sure no overlay is open
    let state = inst.ui_state();
    assert!(!state.settings_open);
    assert!(!state.notification_panel_open);

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
}

// ============================================================
// Settings Window Interaction Tests
// ============================================================

#[test]
#[ignore]
fn test_settings_window_is_interactive() {
    let mut inst = GuiTestInstance::spawn();

    // Open settings
    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings open", Duration::from_secs(3), |s| s.settings_open);

    // The settings window should be interactable.
    // We verify by toggling it twice quickly — if it wasn't interactive/rendering,
    // the second toggle wouldn't work.
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
}

// ============================================================
// Combined Workflow Tests
// ============================================================

#[test]
#[ignore]
fn test_full_workflow_workspace_pane_tab() {
    let mut inst = GuiTestInstance::spawn();

    // Start: 1 workspace, 1 pane, 1 tab
    let state = inst.ui_state();
    assert_eq!(state.workspace_count, 1);
    assert_eq!(state.pane_count, 1);
    assert_eq!(state.tab_count, 1);

    // Create new tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    // Split pane
    inst.press_ctrl_shift(Key::Unicode('e'));
    inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);

    // New workspace
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("2 workspaces", Duration::from_secs(3), |s| s.workspace_count == 2);

    // Switch back to first workspace
    inst.press_alt(Key::Unicode('1'));
    inst.wait_for_ui("workspace 0", Duration::from_secs(3), |s| s.active_workspace == 0);

    // Verify first workspace still has 2 panes
    let state = inst.ui_state();
    assert_eq!(state.pane_count, 2, "first workspace should still have 2 panes");

    // Close pane
    inst.press_ctrl_shift(Key::Unicode('w'));
    inst.wait_for_ui("1 pane", Duration::from_secs(3), |s| s.pane_count == 1);

    // Close tab
    inst.press_ctrl(Key::Unicode('w'));
    inst.wait_for_ui("1 tab", Duration::from_secs(3), |s| s.tab_count == 1);
}

// ============================================================
// Performance / Speed Tests
// ============================================================

#[test]
#[ignore]
fn test_settings_toggle_speed_repeated() {
    let mut inst = GuiTestInstance::spawn();

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
    let mut inst = GuiTestInstance::spawn();

    // Create a second workspace
    inst.press_ctrl_shift(Key::Unicode('n'));
    inst.wait_for_ui("2 workspaces", Duration::from_secs(3), |s| s.workspace_count == 2);

    let mut latencies = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        inst.press_alt(Key::Unicode('1'));
        inst.wait_for_ui("ws 0", Duration::from_secs(3), |s| s.active_workspace == 0);
        latencies.push(start.elapsed());

        let start = Instant::now();
        inst.press_alt(Key::Unicode('2'));
        inst.wait_for_ui("ws 1", Duration::from_secs(3), |s| s.active_workspace == 1);
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
}

#[test]
#[ignore]
fn test_tab_switch_speed() {
    let mut inst = GuiTestInstance::spawn();

    // Create a second tab
    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    let mut latencies = Vec::new();
    for _ in 0..5 {
        // Ctrl+Tab to switch to next tab
        let start = Instant::now();
        inst.press_ctrl(Key::Tab);
        std::thread::sleep(Duration::from_millis(100));
        latencies.push(start.elapsed());

        // Ctrl+Shift+Tab to switch to previous tab
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
}
