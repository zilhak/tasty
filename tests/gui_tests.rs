//! GUI 인스턴스를 공유하는 수동 통합 시험이다. 화면과 키보드 포커스를 사용하므로 기본 실행에서는 제외한다.
//! 격리된 디스플레이에서 cargo test --test gui_tests -- --ignored --test-threads=1로 실행한다.
//! 변화가 없음을 검사할 때는 같은 입력 경로가 실제로 동작하는 대조도 확인한다.
//! 설정 창은 열기 요청 래치 대신 실제 모달 상태와 종류로 기다린다.

mod gui_common;

use enigo::Key;
use gui_common::shared;
use serde_json::json;
use std::time::{Duration, Instant};

/// 입력 헬퍼의 포커스·키 간격 sleep도 포함한 전체 응답 시간 상한이다.
const MAX_UI_RESPONSE_MS: u128 = 1000;

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

#[test]
#[ignore]
fn test_settings_open_ctrl_comma() {
    let mut inst = shared();

    let state = inst.ui_state();
    assert!(
        !state.settings_modal_is_up(),
        "settings should be closed initially"
    );

    inst.press_ctrl(Key::Unicode(','));

    let state = inst.wait_for_ui("settings modal is up", Duration::from_secs(3), |s| {
        s.settings_modal_is_up()
    });
    assert!(
        state.settings_modal_is_up(),
        "settings should be open after Ctrl+,"
    );

    inst.close_active_modal();
    inst.wait_for_ui("settings modal is gone", Duration::from_secs(3), |s| {
        !s.settings_modal_is_up()
    });
}

#[test]
#[ignore]
fn test_settings_closes_on_close_request() {
    let mut inst = shared();

    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings modal is up", Duration::from_secs(3), |s| {
        s.settings_modal_is_up()
    });

    inst.close_active_modal();

    let state = inst.wait_for_ui("settings modal is gone", Duration::from_secs(3), |s| {
        !s.settings_modal_is_up()
    });
    assert!(
        !state.settings_modal_is_up(),
        "close request 를 받은 모달은 사라져야 한다"
    );
    assert!(
        state.active_modal_kind.is_none(),
        "종류도 함께 지워져야 한다 — 남으면 다음 시험이 자기가 안 연 창을 본다. 지금: {:?}",
        state.active_modal_kind
    );
}

/// 설정 토글 바인딩으로 닫히고 Escape로는 닫히지 않는 현재 동작을 확인한다.
#[test]
#[ignore]
fn test_the_toggle_binding_closes_the_settings_modal_and_escape_does_not() {
    let mut inst = shared();

    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings modal is up", Duration::from_secs(3), |s| {
        s.settings_modal_is_up()
    });

    inst.press_key(Key::Escape);
    std::thread::sleep(Duration::from_millis(600));
    assert!(
        inst.ui_state().settings_modal_is_up(),
        "Escape 로 설정 모달이 닫혔다 — 동작이 바뀐 것이다. 이 시험과 그 근거 주석을 \
         함께 갱신해라(닫혔다는 것 자체는 결함이 아닐 수 있다)"
    );

    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings modal is gone", Duration::from_secs(3), |s| {
        !s.settings_modal_is_up()
    });
}

#[test]
#[ignore]
fn test_settings_open_speed() {
    let mut inst = shared();

    let elapsed = measure_ui_latency(
        &mut inst,
        "settings open speed",
        |i| i.press_ctrl(Key::Unicode(',')),
        |s| s.settings_modal_is_up(),
    );

    println!("Settings open latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Settings open took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );

    // 열기 지연만 측정하므로 정리는 별도 닫기 요청으로 한다.
    inst.close_active_modal();
    inst.wait_for_ui("settings modal is gone", Duration::from_secs(3), |s| {
        !s.settings_modal_is_up()
    });
}

#[test]
#[ignore]
fn test_notification_panel_toggle() {
    let mut inst = shared();

    let state = inst.ui_state();
    assert!(
        !state.notification_panel_open,
        "notification panel should be closed initially"
    );

    inst.press_ctrl_shift(Key::Unicode('i'));

    let state = inst.wait_for_ui("notification panel open", Duration::from_secs(3), |s| {
        s.notification_panel_open
    });
    assert!(state.notification_panel_open);

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

    inst.press_ctrl_shift(Key::Unicode('i'));
    inst.wait_for_ui("notification open", Duration::from_secs(3), |s| {
        s.notification_panel_open
    });

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

    inst.press_ctrl_shift(Key::Unicode('i'));
    inst.wait_for_ui("notification closed", Duration::from_secs(3), |s| {
        !s.notification_panel_open
    });
}

#[test]
#[ignore]
fn test_new_workspace_ctrl_shift_n() {
    let mut inst = shared();
    let initial_count = inst.ui_state().workspace_count;

    inst.press_alt(Key::Unicode('n'));

    let state = inst.wait_for_ui("workspace count increased", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count + 1
    });
    assert_eq!(state.workspace_count, initial_count + 1);

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("workspace closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count
    });
}

#[test]
#[ignore]
fn test_workspace_switch_alt_number() {
    let mut inst = shared();

    let initial_count = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("new ws", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count + 1
    });

    let state = inst.ui_state();
    let new_ws_idx = state.active_workspace;

    inst.press_alt(Key::Unicode('1'));
    inst.wait_for_ui("switch to ws 0", Duration::from_secs(3), |s| {
        s.active_workspace == 0
    });

    let target = new_ws_idx + 1; // Alt+N is 1-based
    let key = char::from_digit(target as u32, 10).unwrap();
    inst.press_alt(Key::Unicode(key));
    inst.wait_for_ui("switch back", Duration::from_secs(3), |s| {
        s.active_workspace == new_ws_idx
    });

    inst.press_alt_shift(Key::Unicode('w'));
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
        |i| i.press_alt(Key::Unicode('n')),
        |s| s.workspace_count == initial_count + 1,
    );

    println!("Workspace creation latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Workspace creation took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_count
    });
}

#[test]
#[ignore]
fn test_new_tab_ctrl_shift_t() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    let state = inst.ui_state();
    assert_eq!(state.tab_count, 1, "new workspace should start with 1 tab");

    inst.press_ctrl_shift(Key::Unicode('t'));
    let state = inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);
    assert_eq!(state.tab_count, 2);

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_close_tab_ctrl_w() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    inst.press_ctrl(Key::Unicode('w'));
    let state = inst.wait_for_ui("1 tab", Duration::from_secs(3), |s| s.tab_count == 1);
    assert_eq!(state.tab_count, 1);

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_tab_creation_speed() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
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

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_pane_split_vertical_ctrl_shift_e() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    assert_eq!(inst.ui_state().pane_count, 1);

    inst.press_alt(Key::Unicode('e'));
    let state = inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);
    assert_eq!(state.pane_count, 2);

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_pane_split_horizontal_ctrl_shift_o() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    inst.press_alt_shift(Key::Unicode('e'));
    let state = inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);
    assert_eq!(state.pane_count, 2);

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_close_pane_ctrl_shift_w() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    inst.press_alt(Key::Unicode('e'));
    inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);

    inst.press_ctrl_shift(Key::Unicode('w'));
    let state = inst.wait_for_ui("1 pane", Duration::from_secs(3), |s| s.pane_count == 1);
    assert_eq!(state.pane_count, 1);

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_pane_split_speed() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    let elapsed = measure_ui_latency(
        &mut inst,
        "pane split speed",
        |i| i.press_alt(Key::Unicode('e')),
        |s| s.pane_count == 2,
    );

    println!("Pane split latency: {}ms", elapsed.as_millis());
    assert!(
        elapsed.as_millis() < MAX_UI_RESPONSE_MS,
        "Pane split took {}ms, exceeds {}ms limit",
        elapsed.as_millis(),
        MAX_UI_RESPONSE_MS,
    );

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_keyboard_not_sent_to_terminal_when_settings_open() {
    let mut inst = shared();

    // 목록의 첫 서피스가 활성 터미널이라는 보장이 없어 설정을 열기 전에 포커스된 ID를 얻는다.
    let sid = inst
        .debug_focused_surface()
        .expect("mark 를 찍을 포커스된 surface");

    inst.call("surface.set_mark", serde_json::json!({ "surface_id": sid }));

    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings open", Duration::from_secs(3), |s| {
        s.settings_modal_is_up()
    });

    // 모달이 입력을 받는 경우와 구별하도록 본창에 직접 입력해 본창의 차단을 확인한다.
    inst.type_text_into_main_window("hello_should_not_appear");
    std::thread::sleep(Duration::from_millis(500));

    let result = inst.call(
        "surface.read_since_mark",
        serde_json::json!({ "surface_id": sid, "strip_ansi": true }),
    );
    let output = result["text"].as_str().unwrap_or("");
    assert!(
        !output.contains("hello_should_not_appear"),
        "Terminal should NOT receive keyboard input when settings is open. Got: {}",
        output,
    );

    // Escape로는 설정이 닫히지 않으므로 닫기 요청으로 정리한다.
    inst.close_active_modal();
    inst.wait_for_ui("settings modal is gone", Duration::from_secs(3), |s| {
        !s.settings_modal_is_up()
    });

    // 입력이 전혀 도달하지 않아도 부정 단정은 통과할 수 있어 모달을 닫은 뒤 같은 경로로 출력도 확인한다.
    inst.call("surface.set_mark", serde_json::json!({ "surface_id": sid }));
    inst.type_text("echo overlay_control_marker");
    inst.press_key(Key::Return);
    std::thread::sleep(Duration::from_millis(1000));
    let control = inst.call(
        "surface.read_since_mark",
        serde_json::json!({ "surface_id": sid, "strip_ansi": true }),
    );
    let control = control["text"].as_str().unwrap_or("");
    assert!(
        control.contains("overlay_control_marker"),
        "설정을 닫은 뒤에도 입력을 확인하지 못했다. 앞의 미전송 결과만으로 모달의 입력 차단을 입증할 수 없다: {control}"
    );
}

#[test]
#[ignore]
fn test_keyboard_sent_to_terminal_when_no_overlay() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    std::thread::sleep(Duration::from_millis(500));

    // 새 워크스페이스를 만든 뒤 포커스된 ID를 얻어 이전 터미널에 입력하지 않도록 한다.
    let sid = inst
        .debug_focused_surface()
        .expect("새 workspace 의 포커스된 surface");

    inst.call("surface.set_mark", serde_json::json!({ "surface_id": sid }));

    inst.type_text("echo gui_test_marker");
    inst.press_key(Key::Return);

    std::thread::sleep(Duration::from_millis(1000));

    let result = inst.call(
        "surface.read_since_mark",
        serde_json::json!({ "surface_id": sid, "strip_ansi": true }),
    );
    let output = result["text"].as_str().unwrap_or("");
    assert!(
        output.contains("gui_test_marker"),
        "Terminal should receive keyboard input when no overlay is open. Got: {}",
        output,
    );

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_settings_window_is_interactive() {
    let mut inst = shared();

    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings modal is up", Duration::from_secs(3), |s| {
        s.settings_modal_is_up()
    });

    std::thread::sleep(Duration::from_millis(300));

    inst.close_active_modal();
    inst.wait_for_ui("settings modal is gone", Duration::from_secs(3), |s| {
        !s.settings_modal_is_up()
    });

    inst.press_ctrl(Key::Unicode(','));
    inst.wait_for_ui("settings modal is up again", Duration::from_secs(3), |s| {
        s.settings_modal_is_up()
    });

    let state = inst.ui_state();
    assert!(
        state.settings_modal_is_up(),
        "settings should still be open after the open/close round trip"
    );

    inst.close_active_modal();
    inst.wait_for_ui("settings modal is gone", Duration::from_secs(3), |s| {
        !s.settings_modal_is_up()
    });
}

#[test]
#[ignore]
fn test_full_workflow_workspace_pane_tab() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    let state = inst.ui_state();
    assert_eq!(state.pane_count, 1);
    assert_eq!(state.tab_count, 1);

    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    inst.press_alt(Key::Unicode('e'));
    inst.wait_for_ui("2 panes", Duration::from_secs(3), |s| s.pane_count == 2);

    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws+1", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 2
    });

    let prev_ws_key = char::from_digit((initial_ws + 1) as u32, 10).unwrap();
    inst.press_alt(Key::Unicode(prev_ws_key));
    inst.wait_for_ui("switch back", Duration::from_secs(3), |s| {
        s.active_workspace == initial_ws
    });

    let state = inst.ui_state();
    assert_eq!(state.pane_count, 2, "workspace should still have 2 panes");

    inst.press_ctrl_shift(Key::Unicode('w'));
    inst.wait_for_ui("1 pane", Duration::from_secs(3), |s| s.pane_count == 1);

    inst.press_ctrl(Key::Unicode('w'));
    inst.wait_for_ui("1 tab", Duration::from_secs(3), |s| s.tab_count == 1);

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws-1", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });
    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws restored", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_settings_open_speed_repeated() {
    let mut inst = shared();

    // 키보드로 여는 시간만 측정한다. 닫기는 IPC로 처리하므로 같은 지연 측정에 섞지 않는다.
    let mut latencies = Vec::new();
    for _ in 0..5 {
        let start = Instant::now();
        inst.press_ctrl(Key::Unicode(','));
        inst.wait_for_ui("settings modal is up", Duration::from_secs(3), |s| {
            s.settings_modal_is_up()
        });
        latencies.push(start.elapsed());

        inst.close_active_modal();
        inst.wait_for_ui("settings modal is gone", Duration::from_secs(3), |s| {
            !s.settings_modal_is_up()
        });
    }

    let avg_ms = latencies.iter().map(|d| d.as_millis()).sum::<u128>() / latencies.len() as u128;
    let max_ms = latencies.iter().map(|d| d.as_millis()).max().unwrap_or(0);

    println!(
        "Settings open: avg={}ms, max={}ms over {} iterations",
        avg_ms,
        max_ms,
        latencies.len()
    );

    assert!(
        max_ms < MAX_UI_RESPONSE_MS,
        "Settings open max latency {}ms exceeds {}ms limit",
        max_ms,
        MAX_UI_RESPONSE_MS,
    );
}

#[test]
#[ignore]
fn test_workspace_switch_speed() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
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

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

#[test]
#[ignore]
fn test_tab_switch_speed() {
    let mut inst = shared();

    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });

    inst.press_ctrl_shift(Key::Unicode('t'));
    inst.wait_for_ui("2 tabs", Duration::from_secs(3), |s| s.tab_count == 2);

    // 탭 수는 전환해도 같으므로 active_tab의 실제 변화를 기다려 시간을 측정한다.
    let mut latencies = Vec::new();
    for _ in 0..5 {
        let from = inst.ui_state().active_tab;
        let start = Instant::now();
        inst.press_ctrl(Key::Tab);
        inst.wait_for_ui("tab forward", Duration::from_secs(3), move |s| {
            s.active_tab != from
        });
        latencies.push(start.elapsed());

        let from = inst.ui_state().active_tab;
        let start = Instant::now();
        inst.press_ctrl_shift(Key::Tab);
        inst.wait_for_ui("tab back", Duration::from_secs(3), move |s| {
            s.active_tab != from
        });
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

    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

// 실제 OS IME 이벤트 대신 IPC로 preedit를 설정하고 단축키는 실제 키 입력으로 보낸다.
// 팝업 포커스로 인한 clear는 열린 search_bar를 비활성화한 뒤 다시 포커스하는 상태로 재현한다.

#[test]
#[ignore]
fn test_ime_preedit_flushed_on_non_popup_shortcut() {
    let mut inst = shared();
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });
    std::thread::sleep(Duration::from_millis(500)); // shell ready

    let pre = inst.call("surface.ime_preedit", serde_json::json!({ "text": "한" }));
    let sid = pre["surface_id"]
        .as_u64()
        .expect("preedit response surface_id");
    assert_eq!(pre["preedit_active"], serde_json::json!(true));

    // preedit 자체는 화면에 에코되지 않아 mark 이후 출력으로 단축키의 전송을 확인한다.
    inst.call("surface.set_mark", serde_json::json!({ "surface_id": sid }));

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

    inst.press_ctrl(Key::Unicode('b'));
    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

/// search_bar를 닫지 않고 포커스만 해제한 뒤 다시 find를 눌러 preedit 폐기를 검사한다.
#[test]
#[ignore]
fn test_ime_preedit_cleared_on_popup_focus_shortcut() {
    let mut inst = shared();
    let initial_ws = inst.ui_state().workspace_count;
    inst.press_alt(Key::Unicode('n'));
    inst.wait_for_ui("ws created", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws + 1
    });
    std::thread::sleep(Duration::from_millis(500));

    inst.press_ctrl(Key::Unicode('f'));
    std::thread::sleep(Duration::from_millis(300));
    let (w, h) = inst.client_size();
    inst.click_at(w / 2, h * 3 / 4); // 상단 앵커 search_bar 를 피해 터미널 영역 클릭
    std::thread::sleep(Duration::from_millis(200));

    // 팝업이 열리지 않아도 뒤의 부정 단정이 통과할 수 있어 먼저 열림 상태를 확인한다.
    let popups = inst.call("debug.host_popup.list", serde_json::json!({}));
    let search_open = popups["popups"].as_array().is_some_and(|list| {
        list.iter().any(|p| {
            p["id"] == serde_json::json!("search_bar") && p["open"] == serde_json::json!(true)
        })
    });
    assert!(
        search_open,
        "search_bar가 열리지 않아 preedit 폐기 시나리오의 전제를 만족하지 못했다: {popups}"
    );
    // debug.host_popup.list는 포커스를 반환하지 않아 비포커스 상태까지 직접 확인하지는 못한다.

    let pre = inst.call("surface.ime_preedit", serde_json::json!({ "text": "한" }));
    let sid = pre["surface_id"]
        .as_u64()
        .expect("preedit response surface_id");
    // preedit가 실제로 생기지 않아도 미전송 단정은 통과하므로 설정 결과를 확인한다.
    assert_eq!(
        pre["preedit_active"],
        serde_json::json!(true),
        "preedit 설정이 확인되지 않아 폐기 동작을 검사할 수 없다"
    );
    inst.call("surface.set_mark", serde_json::json!({ "surface_id": sid }));

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

    inst.press_key(Key::Escape);
    inst.press_alt_shift(Key::Unicode('w'));
    inst.wait_for_ui("ws closed", Duration::from_secs(3), |s| {
        s.workspace_count == initial_ws
    });
}

// 포인터는 debug IPC로 winit·egui 입력 경로에 주입한다. GUI 인스턴스와 디스플레이는 필요하다.
// 레이아웃 좌표가 있는 활성 워크스페이스를 사용한다. IPC로 만든 워크스페이스는 자동 활성화되지 않는다.

/// 주입 후 상태 반영을 기다리는 고정 간격이다. 반영 완료를 직접 동기화하지는 않는다.
fn settle() {
    std::thread::sleep(Duration::from_millis(150));
}

#[test]
#[ignore]
fn click_to_activate_moves_focus() {
    let inst = shared();

    let focused_before = inst
        .debug_focused_surface()
        .expect("an initially focused surface");

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

    assert_ne!(
        inst.debug_focused_surface(),
        Some(new_sid),
        "IPC split must not move focus (focus independence)"
    );

    inst.inject_mouse(new_sid, 0.5, 0.5, "press", 0);
    inst.inject_mouse(new_sid, 0.5, 0.5, "release", 0);
    settle();

    assert_eq!(
        inst.debug_focused_surface(),
        Some(new_sid),
        "click-to-activate should move focus to the clicked surface"
    );

    inst.call("surface.close", json!({ "surface_id": new_sid }));
    settle();
}

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
    assert_ne!(
        (sel["start"]["col"].clone(), sel["start"]["row"].clone()),
        (sel["end"]["col"].clone(), sel["end"]["row"].clone()),
        "selection start and end should differ"
    );
}

#[test]
#[ignore]
fn right_click_opens_terminal_menu() {
    let inst = shared();

    let sid = inst
        .debug_focused_surface()
        .expect("a focused terminal surface");

    inst.inject_mouse(sid, 0.5, 0.5, "press", 2);
    // Linux에서는 컨텍스트 메뉴를 release에서 열므로 press와 release를 모두 보낸다.
    inst.inject_mouse(sid, 0.5, 0.5, "release", 2);
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

#[test]
#[ignore]
fn right_click_explorer_never_falls_back_to_surface_menu() {
    let inst = shared();

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
    // IPC로 만든 탭은 자동 선택되지 않으므로 사용자 전환을 재현해 렌더링한다.
    let last = created["tab_count"]
        .as_u64()
        .expect("tab.create should return tab_count")
        - 1;
    inst.call("debug.switch_tab", json!({ "index": last }));
    settle();

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

/// 1000·1002·1003은 독립 플래그라 먼저 모두 끄고 원하는 모드 하나를 켠다.
/// cat -v로 수신 바이트를 관측하며 mark는 호출자가 출력 준비 후 설정한다.
#[cfg(test)]
fn enable_mouse_tracking(inst: &gui_common::GuiTestInstance, sid: u64, mode: &str) {
    inst.call(
        "surface.send",
        json!({ "surface_id": sid, "text": format!(
            "printf '\\e[?1000l\\e[?1002l\\e[?1003l\\e[?{mode}h\\e[?1006h'; cat -v\n"
        ) }),
    );
    std::thread::sleep(Duration::from_millis(400));
}

#[cfg(test)]
fn read_since_mark(inst: &gui_common::GuiTestInstance, sid: u64) -> String {
    let res = inst.call(
        "surface.read_since_mark",
        json!({ "surface_id": sid, "strip_ansi": true }),
    );
    res["text"].as_str().unwrap_or("").to_string()
}

#[test]
#[ignore]
fn hover_motion_reported_only_for_mode_1003() {
    let inst = shared();
    inst.focus();
    let sid = inst.first_surface_id();

    enable_mouse_tracking(&inst, sid, "1003");
    inst.call("surface.set_mark", json!({ "surface_id": sid }));
    for fx in [0.30_f32, 0.45, 0.60, 0.75] {
        inst.inject_mouse(sid, fx, 0.5, "move", 0);
    }
    settle();
    let out = read_since_mark(&inst, sid);
    assert!(
        out.contains("^[[<35;"),
        "1003 hover should emit cb=35 motion reports, got: {out:?}"
    );

    inst.call("surface.set_mark", json!({ "surface_id": sid }));
    inst.inject_mouse(sid, 0.75, 0.5, "move", 0);
    settle();
    assert!(
        !read_since_mark(&inst, sid).contains("^[[<35;"),
        "same-cell motion must be deduped"
    );

    inst.call(
        "surface.send",
        json!({ "surface_id": sid, "text": "\u{3}" }),
    );
    std::thread::sleep(Duration::from_millis(300));
    enable_mouse_tracking(&inst, sid, "1002");
    inst.call("surface.set_mark", json!({ "surface_id": sid }));
    for fx in [0.30_f32, 0.45, 0.60] {
        inst.inject_mouse(sid, fx, 0.5, "move", 0);
    }
    settle();
    let out = read_since_mark(&inst, sid);
    assert!(
        !out.contains("^[[<35;"),
        "1002 must not report button-less hover, got: {out:?}"
    );
    inst.call(
        "surface.send",
        json!({ "surface_id": sid, "text": "\u{3}" }),
    );
}

#[test]
#[ignore]
fn hover_motion_never_reaches_a_non_focused_surface() {
    let inst = shared();
    inst.focus();
    let focused = inst
        .debug_focused_surface()
        .expect("an initially focused surface");

    let res = inst.call(
        "split",
        json!({ "level": "surface", "direction": "vertical", "target_surface": focused }),
    );
    let other = res["new_surface_id"].as_u64().expect("new_surface_id");
    settle();

    // 비포커스 대상에서 변화가 없음을 검사하기 전에 포커스된 대상의 hover 출력으로 주입 동작을 확인한다.
    enable_mouse_tracking(&inst, focused, "1003");
    inst.call("surface.set_mark", json!({ "surface_id": focused }));
    for fx in [0.30_f32, 0.50, 0.70] {
        inst.inject_mouse(focused, fx, 0.5, "move", 0);
    }
    settle();
    let control = read_since_mark(&inst, focused);
    assert!(
        control.contains("^[[<35;"),
        "포커스된 서피스의 hover 보고를 찾지 못해 주입 경로의 동작을 확인할 수 없다: {control:?}"
    );
    // 공유 인스턴스에 모드를 남기지 않는다. Ctrl+C는 cat만 끝내고 추적 모드를 복원하지 않는다.
    inst.call(
        "surface.send",
        json!({ "surface_id": focused, "text": "\u{3}" }),
    );
    std::thread::sleep(Duration::from_millis(300));
    inst.call(
        "surface.send",
        json!({ "surface_id": focused, "text": "printf '\\e[?1003l\\e[?1006l'\n" }),
    );
    std::thread::sleep(Duration::from_millis(300));
    inst.call("surface.set_mark", json!({ "surface_id": focused }));

    enable_mouse_tracking(&inst, other, "1003");
    inst.call("surface.set_mark", json!({ "surface_id": other }));
    for fx in [0.30_f32, 0.50, 0.70] {
        inst.inject_mouse(other, fx, 0.5, "move", 0);
    }
    settle();

    assert!(
        !read_since_mark(&inst, other).contains("^[[<35;"),
        "hover must not leak into a non-focused surface"
    );
    assert_eq!(
        inst.debug_focused_surface(),
        Some(focused),
        "hover must not move focus"
    );

    inst.call(
        "surface.send",
        json!({ "surface_id": other, "text": "\u{3}" }),
    );
    inst.call("surface.close", json!({ "surface_id": other }));
}

#[test]
#[ignore]
fn drag_motion_carries_the_pressed_button() {
    let inst = shared();
    inst.focus();
    let sid = inst.first_surface_id();

    enable_mouse_tracking(&inst, sid, "1002");
    for (button, press, motion) in [
        (2_u8, "^[[<2;", "^[[<34;"),
        (1, "^[[<1;", "^[[<33;"),
        (0, "^[[<0;", "^[[<32;"),
    ] {
        inst.call("surface.set_mark", json!({ "surface_id": sid }));
        inst.inject_mouse(sid, 0.30, 0.5, "press", button);
        inst.inject_mouse(sid, 0.45, 0.5, "move", button);
        inst.inject_mouse(sid, 0.60, 0.5, "move", button);
        inst.inject_mouse(sid, 0.60, 0.5, "release", button);
        settle();
        let out = read_since_mark(&inst, sid);
        assert!(
            out.contains(press),
            "button {button} press should be reported, got: {out:?}"
        );
        assert!(
            out.contains(motion),
            "button {button} drag should report motion with its own bits, got: {out:?}"
        );
    }

    inst.call(
        "surface.send",
        json!({ "surface_id": sid, "text": "\u{3}" }),
    );
    std::thread::sleep(Duration::from_millis(300));
    inst.call(
        "surface.send",
        json!({ "surface_id": sid, "text": "printf '\\e[?1002l\\e[?1003l'; cat -v\n" }),
    );
    std::thread::sleep(Duration::from_millis(400));
    inst.call("surface.set_mark", json!({ "surface_id": sid }));
    inst.inject_mouse(sid, 0.30, 0.5, "press", 2);
    inst.inject_mouse(sid, 0.50, 0.5, "move", 2);
    inst.inject_mouse(sid, 0.50, 0.5, "release", 2);
    settle();
    assert!(
        !read_since_mark(&inst, sid).contains("^[[<"),
        "tracking off must not report any mouse bytes"
    );
    assert_eq!(
        inst.debug_selection()["present"],
        json!(false),
        "a right drag must not start a local text selection"
    );
    inst.call(
        "surface.send",
        json!({ "surface_id": sid, "text": "\u{3}" }),
    );
}

/// 팔레트 붙여넣기는 터미널 키 입력을 거치지 않으므로 붙여넣기 처리 자체가 사용자 입력을 기록해야 한다.
/// 이전 입력 기록의 유효 시간이 지난 뒤 실행하고 실제 출력과 idle_seconds 감소를 함께 확인한다.
#[test]
#[ignore]
fn test_palette_paste_records_user_typing() {
    let inst = shared();
    let sid = inst
        .debug_focused_surface()
        .expect("a focused surface to paste into");
    let is_typing = || inst.call("surface.is_typing", json!({ "surface_id": sid }));

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let idle = is_typing()["idle_seconds"].as_f64().unwrap();
        if !(0.0..=3.0).contains(&idle) {
            break;
        }
        assert!(Instant::now() < deadline, "idle never exceeded 3s: {idle}");
        std::thread::sleep(Duration::from_millis(200));
    }

    let mark = "PALETTE_PASTE_MARK";
    inst.call("clipboard.set_text", json!({ "text": mark }));
    inst.call(
        "debug.host_popup.open",
        json!({ "popup_id": "command_palette" }),
    );
    std::thread::sleep(Duration::from_millis(300));
    inst.call("debug.inject_egui_text", json!({ "text": "paste" }));
    std::thread::sleep(Duration::from_millis(300));
    inst.call("debug.inject_egui_key", json!({ "key": "Enter" }));

    let deadline = Instant::now() + Duration::from_secs(3);
    let screen = loop {
        let text = inst.call("surface.screen_text", json!({ "surface_id": sid }))["text"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        if text.contains(mark) || Instant::now() >= deadline {
            break text;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        screen.contains(mark),
        "palette paste did not reach the input: {screen:?}"
    );

    let after = is_typing();
    let idle = after["idle_seconds"].as_f64().unwrap();
    assert!(
        (0.0..3.0).contains(&idle),
        "palette paste must record user input, got idle_seconds={idle}"
    );
    assert_eq!(after["typing"], json!(true), "got {after}");

    inst.call(
        "surface.send",
        json!({ "surface_id": sid, "text": "\u{15}" }),
    );
}
