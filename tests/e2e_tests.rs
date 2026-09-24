//! 시나리오별 테스트는 인스턴스를 공유하고 전용 워크스페이스로 격리한다.
//! 전역 목록은 다른 시나리오와 함께 바뀌므로 전체 길이 대신 자기 항목의 존재를 확인한다.

mod common;

use common::{TastyInstance, TestWorkspace};
use serde_json::json;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

/// 창이나 공용 설정을 바꾸는 시나리오는 write 잠금으로 단독 실행한다. 나머지는 read 잠금으로 병렬 실행한다.
/// 잠금 제거의 안전성은 확인하지 않았다. 잠금은 동시 실행만 막으므로 만든 창과 공용 설정은 별도로 정리해야 한다.
static WINDOW_EXCLUSIVE: RwLock<()> = RwLock::new(());

/// 다른 테스트의 panic이 이 테스트까지 실패시키지 않도록 poison은 무시한다.
fn lane() -> RwLockReadGuard<'static, ()> {
    WINDOW_EXCLUSIVE.read().unwrap_or_else(|e| e.into_inner())
}

fn exclusive_lane() -> RwLockWriteGuard<'static, ()> {
    WINDOW_EXCLUSIVE.write().unwrap_or_else(|e| e.into_inner())
}

/// 반환한 잠금 가드는 시나리오가 끝날 때까지 _lane에 보관한다.
fn scenario(
    name: &str,
) -> (
    &'static TastyInstance,
    TestWorkspace,
    u64,
    u64,
    RwLockReadGuard<'static, ()>,
) {
    let lane = lane();
    let tasty = common::shared();
    let ws = tasty.create_workspace(name);
    let sid = ws.surface_id;
    tasty.wait_for_shell(sid);
    let pid = tasty.first_pane_id_in_workspace(ws.id);
    (tasty, ws, sid, pid, lane)
}

#[test]
fn read_only_queries() {
    let (tasty, _ws, sid, pid, _lane) = scenario("e2e-read-only");

    let info = tasty.call("system.info", json!({}));
    assert_eq!(
        info.get("version").and_then(|v| v.as_str()),
        Some(env!("CARGO_PKG_VERSION")),
    );
    assert!(info["workspace_count"].as_u64().unwrap() >= 1);

    let tree = tasty.call("tree", json!({}));
    let tree_arr = tree.as_array().unwrap();
    assert!(!tree_arr.is_empty());
    assert!(tree_arr[0].get("name").is_some());

    let ui = tasty.call("ui.state", json!({}));
    assert_eq!(ui["settings_open_requested"], false);
    // 키가 빠진 경우와 명시적인 null을 구별한다.
    assert_eq!(ui["modal_open"], false);
    assert!(
        ui.get("active_modal_kind").is_some(),
        "`ui.state` 가 `active_modal_kind` 키를 아예 안 냈다 — 소비자 쪽에서는 이것이 \
         '모달이 없다' 와 구별되지 않는다"
    );
    assert_eq!(ui["active_modal_kind"], serde_json::Value::Null);
    assert_eq!(ui["notification_panel_open"], false);
    assert!(ui["workspace_count"].as_u64().unwrap() >= 1);
    assert!(ui["pane_count"].as_u64().unwrap() >= 1);
    assert!(ui["tab_count"].as_u64().unwrap() >= 1);

    let surfaces = tasty.call("surface.list", json!({}));
    assert!(
        surfaces
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"].as_u64() == Some(sid)),
        "surface.list 가 내 workspace 의 surface={sid} 를 빠뜨림: {surfaces:?}"
    );
    let panes = tasty.call("pane.list", json!({}));
    assert!(
        panes
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"].as_u64() == Some(pid)),
        "pane.list 가 내 workspace 의 pane={pid} 를 빠뜨림: {panes:?}"
    );

    let text = tasty.screen_text_of(sid);
    assert!(!text.trim().is_empty());

    let cursor = tasty.call("surface.cursor_position", json!({"surface_id": sid}));
    assert!(cursor.get("x").is_some());
    assert!(cursor.get("y").is_some());

    let tabs = tasty.call("tab.list", json!({"pane_id": pid}));
    assert!(!tabs["tabs"].as_array().unwrap().is_empty());
}

/// OSC 7의 cwd가 표시 이름에 반영되는지 GUI와 헤드리스에서 확인한다.
/// 하네스의 /bin/sh는 OSC 제목을 자동 전송하지 않으므로 제목 우선순위와 섞이지 않는다.
/// printf의 이스케이프 해석이 다른 Windows는 제외한다.
#[cfg(not(windows))]
#[test]
fn an_osc7_cwd_becomes_the_tab_name() {
    // 표시 이름은 tree에서 읽는다. tab.list는 원본 name을 반환한다.
    fn tab_names(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::Object(o) => {
                if o.contains_key("surface")
                    && let Some(n) = o.get("name").and_then(|n| n.as_str())
                {
                    out.push(n.to_string());
                }
                o.values().for_each(|c| tab_names(c, out));
            }
            serde_json::Value::Array(a) => a.iter().for_each(|c| tab_names(c, out)),
            _ => {}
        }
    }
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-osc7-tab-name");
    let dir = format!("tasty-osc7-{}", std::process::id());
    tasty.send_text(
        sid,
        &format!("printf '\\033]7;file://localhost/tmp/{dir}\\007'\r"),
    );
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let mut names = Vec::new();
        tab_names(&tasty.call("tree", json!({})), &mut names);
        if names.iter().any(|n| n == &dir) {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "OSC 7 cwd({dir}) 가 탭 이름이 되지 않았다: {names:?} / 화면: {}",
            tasty.screen_text_of(sid)
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn workspace_list_rows_carry_mirror_and_id() {
    let (tasty, ws, _sid, _pid, _lane) = scenario("e2e-workspace-list-shape");

    let ws_rows = tasty
        .call("workspace.list", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!ws_rows.is_empty(), "workspace.list 가 비었다");
    for row in &ws_rows {
        assert_eq!(
            row.get("mirror").and_then(|v| v.as_bool()),
            Some(false),
            "workspace.list 행에 mirror:false 가 없다: {row:?}"
        );
        assert!(
            row.get("id").and_then(|v| v.as_u64()).is_some(),
            "workspace.list 행에 id 가 없다: {row:?}"
        );
    }
    assert!(
        ws_rows.iter().any(|row| row["id"].as_u64() == Some(ws.id)),
        "workspace.list 가 방금 만든 workspace={} 를 빠뜨림: {ws_rows:?}",
        ws.id
    );
}

#[test]
fn markdown_recent_is_read_only() {
    let _lane = lane();
    let tasty = common::shared();

    let recent = tasty.call("markdown.recent", json!({}));
    let recent_arr = recent["recent"]
        .as_array()
        .expect("markdown.recent returns { recent: [...] }");
    assert!(recent_arr.len() <= 10, "recent 은 최대 10개");
    let recent2 = tasty.call("markdown.recent", json!({}));
    assert!(recent2["recent"].is_array());
}

#[test]
fn notification_create_then_list() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-notification");

    let created = tasty.call(
        "notification.create",
        json!({"title": "Test", "body": "Hello", "surface_id": sid}),
    );
    let notifs = tasty.call("notification.list", json!({}));
    let rows = notifs.as_array().cloned().unwrap_or_default();
    assert!(!rows.is_empty(), "notification.list 가 비었다: {created:?}");
    assert!(
        rows.iter().any(|n| n["surface_id"].as_u64() == Some(sid)),
        "notification.list 가 내 surface={sid} 의 알림을 빠뜨림: {rows:?}"
    );
}

#[test]
fn terminal_echo_and_mark_read() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-terminal-echo");

    tasty.set_mark(sid);
    let echo_cmd = if cfg!(windows) {
        "echo hello\r\n"
    } else {
        "echo hello\n"
    };
    tasty.send_text(sid, echo_cmd);
    let output = tasty.wait_for_output(sid, "hello", Duration::from_secs(5));
    assert!(output.contains("hello"));

    tasty.set_mark(sid);
    let echo_cmd = if cfg!(windows) {
        "echo test_marker\r\n"
    } else {
        "echo test_marker\n"
    };
    tasty.send_text(sid, echo_cmd);
    let output = tasty.wait_for_output(sid, "test_marker", Duration::from_secs(5));
    assert!(output.contains("test_marker"));
}

/// 실제 IPC로 출력 스캐너 커서의 전진과 에이전트 mark와의 독립성을 확인한다.
#[test]
fn terminal_scan_cursor_is_separate_from_the_agent_mark() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-scan-cursor");

    let first = tasty.call(
        "surface.read_since_scan_mark",
        json!({"surface_id": sid, "strip_ansi": true}),
    );
    assert!(
        first.get("text").and_then(|v| v.as_str()).is_some(),
        "surface.read_since_scan_mark 응답에 text가 없다: {first:?}"
    );
    assert_eq!(first["surface_id"].as_u64(), Some(sid));

    let scan = |strip_ansi: bool| -> String {
        tasty.call(
            "surface.read_since_scan_mark",
            json!({"surface_id": sid, "strip_ansi": strip_ansi}),
        )["text"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    // 커서가 전진하므로 출력이 나뉘어 오면 읽은 조각을 이어 붙인다.
    let wait_scan = |needle: &str| -> String {
        let start = std::time::Instant::now();
        let mut seen = String::new();
        loop {
            seen.push_str(&scan(true));
            if seen.contains(needle) {
                return seen;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "scan 커서에서 '{needle}' 를 못 봤다. 본 것:\n{seen}"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let echo = |marker: &str| {
        let cmd = if cfg!(windows) {
            format!("echo {marker}\r\n")
        } else {
            format!("echo {marker}\n")
        };
        tasty.send_text(sid, &cmd);
    };

    echo("scan_marker_one");
    wait_scan("scan_marker_one");
    assert!(
        !scan(true).contains("scan_marker_one"),
        "커서가 전진하지 않았다 — 같은 구간이 다시 왔다"
    );

    // 출력이 도착한 뒤 mark를 세워야 set_mark가 scan 커서까지 옮기는 결함을 찾을 수 있다.
    echo("scan_marker_two");
    // scan으로 기다리면 커서가 전진하므로 mark를 움직이지 않는 조회로 출력 도착을 확인한다.
    tasty.wait_for_output(sid, "scan_marker_two", Duration::from_secs(10));
    tasty.set_mark(sid);
    let seen = wait_scan("scan_marker_two");
    assert!(
        seen.contains("scan_marker_two"),
        "set_mark 이 scan 커서를 밀었다 — 그 앞에 이미 와 있던 출력이 사라졌다: {seen}"
    );

    // scan으로 먼저 읽은 뒤에도 에이전트 mark에서는 같은 출력을 읽을 수 있어야 한다.
    echo("scan_marker_three");
    wait_scan("scan_marker_three");
    let since_mark = tasty.read_since_mark(sid);
    assert!(
        since_mark.contains("scan_marker_three"),
        "scan 읽기가 에이전트의 mark 를 밀었다 — mark 이후 구간에서 출력이 사라졌다: \
         {since_mark}"
    );
}

/// 소비자별 커서와 응답 필드가 실제 IPC에서도 유지되는지 확인한다.
#[test]
fn terminal_output_reads_from_a_consumer_held_position() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-output-cursor");

    let read = |params: serde_json::Value| -> serde_json::Value {
        let mut p = json!({"surface_id": sid, "strip_ansi": true});
        for (k, v) in params.as_object().expect("object") {
            p[k.as_str()] = v.clone();
        }
        tasty.call("surface.read_since_mark", p)
    };
    let echo = |marker: &str| {
        let cmd = if cfg!(windows) {
            format!("echo {marker}\r\n")
        } else {
            format!("echo {marker}\n")
        };
        tasty.send_text(sid, &cmd);
    };

    let first = read(json!({}));
    let stream = first["stream"]
        .as_str()
        .unwrap_or_else(|| panic!("응답에 stream 칸이 없다: {first:?}"))
        .to_string();
    for key in [
        "cursor",
        "next_cursor",
        "raw_bytes",
        "retention_start",
        "retention_end",
        "skipped",
    ] {
        assert!(
            first[key].as_u64().is_some(),
            "응답에 {key} 칸이 없다: {first:?}"
        );
    }
    assert_eq!(first["skipped"].as_u64(), Some(0));

    let mut a = first["next_cursor"].as_u64().expect("next_cursor");
    let b = a;
    echo("cursor_marker_one");
    tasty.wait_for_output(sid, "cursor_marker_one", Duration::from_secs(10));

    let a_read = read(json!({"cursor": a, "stream": stream}));
    assert!(
        a_read["text"]
            .as_str()
            .unwrap_or_default()
            .contains("cursor_marker_one"),
        "A 가 자기 위치 이후를 못 받았다: {a_read:?}"
    );
    a = a_read["next_cursor"].as_u64().expect("next_cursor");

    let b_read = read(json!({"cursor": b, "stream": stream}));
    assert!(
        b_read["text"]
            .as_str()
            .unwrap_or_default()
            .contains("cursor_marker_one"),
        "A 의 읽기가 B 의 위치를 움직였다: {b_read:?}"
    );

    let a_again = read(json!({"cursor": a, "stream": stream}));
    assert!(
        !a_again["text"]
            .as_str()
            .unwrap_or_default()
            .contains("cursor_marker_one"),
        "B 의 읽기가 A 의 위치를 되돌렸다: {a_again:?}"
    );

    echo("cursor_marker_two");
    tasty.wait_for_output(sid, "cursor_marker_two", Duration::from_secs(10));
    tasty.set_mark(sid);
    let after_mark = read(json!({"cursor": a, "stream": stream}));
    assert!(
        after_mark["text"]
            .as_str()
            .unwrap_or_default()
            .contains("cursor_marker_two"),
        "set_mark 이 소비자의 위치를 밀었다 — 그 앞에 이미 와 있던 출력이 사라졌다: \
         {after_mark:?}"
    );

    // 호출자가 번역 가능한 메시지에 의존하지 않도록 구조화된 reason을 비교한다.
    let refusal = |params: serde_json::Value| -> String {
        let mut p = json!({"surface_id": sid});
        for (k, v) in params.as_object().expect("object") {
            p[k.as_str()] = v.clone();
        }
        let resp = tasty.call_raw("surface.read_since_mark", p);
        resp["error"]["data"]["reason"]
            .as_str()
            .unwrap_or_else(|| panic!("거절에 사유 칸이 없다: {resp:?}"))
            .to_string()
    };
    assert_eq!(
        refusal(json!({"cursor": 0})),
        "cursor_without_stream",
        "표지 없는 위치를 받아들이면 재사용된 surface id 위에서 남의 출력을 잇는다"
    );
    assert_eq!(
        refusal(json!({"cursor": 0, "stream": "not-this-stream"})),
        "stream_mismatch"
    );
    let end = read(json!({}))["retention_end"].as_u64().expect("end");
    assert_eq!(
        refusal(json!({"cursor": end + 1_000_000, "stream": stream})),
        "cursor_ahead_of_stream"
    );

    // 원문 바이트와 커서 증가량을 비교한다. 이 ASCII 출력만으로는 원문 길이와 text 길이를 구별할 수 없다.
    // 길이가 다른 입력은 버퍼와 응답 생성 함수의 단위 시험에서 검사한다.
    let capped = read(json!({"cursor": a, "stream": stream, "max_bytes": 4}));
    assert!(capped["raw_bytes"].as_u64().is_some_and(|n| n <= 4));
    assert_eq!(
        capped["next_cursor"].as_u64(),
        Some(a + capped["raw_bytes"].as_u64().expect("raw_bytes")),
        "next_cursor 는 원문 바이트로 전진한다"
    );
}

#[test]
fn terminal_send_key_and_send_to() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-terminal-keys");

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

    for key in &["up", "down"] {
        let result = tasty.call("surface.send_key", json!({"surface_id": sid, "key": key}));
        assert_eq!(result["sent"], true, "Failed to send key: {}", key);
    }

    tasty.set_mark(sid);
    tasty.call(
        "surface.send_to",
        json!({"surface_id": sid, "text": "echo targeted\n"}),
    );
    let output = tasty.wait_for_output(sid, "targeted", Duration::from_secs(5));
    assert!(output.contains("targeted"));
}

#[test]
fn terminal_send_combo_and_abort() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-terminal-combo");

    let result = tasty.call(
        "surface.send_combo",
        json!({"surface_id": sid, "key": "x", "modifiers": ["alt"]}),
    );
    assert_eq!(result["sent"], true);
    // zsh에서 Alt+X가 명령 입력 모드를 바꿀 수 있어 Ctrl+G로 빠져나온다. 사용자 dotfile은 격리돼 있다.
    tasty.call(
        "surface.send_combo",
        json!({"surface_id": sid, "key": "g", "modifiers": ["ctrl"]}),
    );
    tasty.set_mark(sid);
    tasty.send_text(sid, "echo __abort_ok__\n");
    tasty.wait_for_output(sid, "__abort_ok__", Duration::from_secs(3));
}

#[test]
fn surface_completion_reaches_the_pipeline() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-completion");

    // IPC 응답만 확인하며 실제 highlight 렌더링은 검사하지 않는다.
    let completion = tasty.call("surface.completion", json!({ "surface_id": sid }));
    assert_eq!(completion["ok"], true);
    assert_eq!(completion["surface_id"].as_u64().unwrap(), sid);
}

#[test]
fn surface_attention_raise_and_clear() {
    // 렌더링 중 포커스된 서피스의 attention이 지워질 수 있으므로 활성화하지 않은 새 워크스페이스를 사용한다.
    let _lane = lane();
    let tasty = common::shared();
    let att_ws = tasty.create_workspace("attention-clear-e2e");
    let att_sid = att_ws.surface_id;
    let raise_kind = |kind: &str| {
        let r = tasty.call(
            "surface.completion",
            json!({ "surface_id": att_sid, "kind": kind }),
        );
        assert_eq!(r["ok"], true);
    };
    let attention_kind =
        || tasty.call("surface.attention.get", json!({ "surface_id": att_sid }))["kind"].clone();

    raise_kind("needs_input");
    assert_eq!(attention_kind(), "needs_input");

    // 늦게 도착한 해제가 새 attention을 지우지 않도록 kind 불일치를 확인한다.
    let mismatched = tasty.call(
        "surface.attention.clear",
        json!({ "surface_id": att_sid, "kind": "completion" }),
    );
    assert_eq!(mismatched["ok"], true);
    assert_eq!(mismatched["cleared"], false);
    assert_eq!(mismatched["previous_kind"], "needs_input");
    assert_eq!(attention_kind(), "needs_input");

    let cleared = tasty.call("surface.attention.clear", json!({ "surface_id": att_sid }));
    assert_eq!(cleared["cleared"], true);
    assert_eq!(cleared["previous_kind"], "needs_input");
    assert!(attention_kind().is_null());

    let again = tasty.call("surface.attention.clear", json!({ "surface_id": att_sid }));
    assert_eq!(again["ok"], true);
    assert_eq!(again["cleared"], false);
    assert!(again["previous_kind"].is_null());

    raise_kind("completion");
    assert_eq!(attention_kind(), "completion");
    let matched = tasty.call(
        "surface.attention.clear",
        json!({ "surface_id": att_sid, "kind": "completion" }),
    );
    assert_eq!(matched["cleared"], true);
    assert!(attention_kind().is_null());

    assert!(
        tasty
            .call_raw("surface.attention.clear", json!({ "surface_id": 999_999 }))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("surface.attention.get", json!({ "surface_id": 999_999 }))
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw(
                "surface.attention.clear",
                json!({ "surface_id": att_sid, "kind": "bogus" })
            )
            .get("error")
            .is_some()
    );
    assert!(
        tasty
            .call_raw("surface.attention.clear", json!({}))
            .get("error")
            .is_some()
    );
}

// Windows 기본 셸은 printf 이스케이프를 동일하게 해석하지 않아 Unix에서만 실행한다.
#[cfg(not(windows))]
#[test]
fn dim_sgr2_survives_to_the_renderer() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-dim-sgr2");

    tasty.set_mark(sid);
    tasty.send_text(sid, "clear; printf '\\033[2mD\\033[0mN\\n'\n");
    tasty.wait_for_output(sid, "DN", Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(200));

    // 기본 조회는 dim 셀을 숨기므로 검사할 D가 포함되도록 show_dim을 켠다.
    let text = tasty.call(
        "surface.screen_text",
        json!({"surface_id": sid, "show_dim": true}),
    )["text"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let row = text
        .lines()
        .position(|l| l.starts_with("DN"))
        .unwrap_or_else(|| panic!("DN row not found in screen_text:\n{text}")) as u64;

    let dim = tasty.call(
        "debug.cell_info",
        json!({"surface_id": sid, "row": row, "col": 0}),
    );
    assert_eq!(dim["text"], "D");
    assert_eq!(dim["intensity"], "half");

    let normal = tasty.call(
        "debug.cell_info",
        json!({"surface_id": sid, "row": row, "col": 1}),
    );
    assert_eq!(normal["text"], "N");
    assert_eq!(normal["intensity"], "normal");

    let dim_glyph = tasty.call(
        "debug.glyph_color",
        json!({"surface_id": sid, "row": row, "col": 0}),
    );
    let normal_glyph = tasty.call(
        "debug.glyph_color",
        json!({"surface_id": sid, "row": row, "col": 1}),
    );
    assert_ne!(
        dim_glyph["fg"]["hex"], normal_glyph["fg"]["hex"],
        "renderer must dim fg distinctly from normal fg",
    );
}

#[test]
fn hook_set_list_unset() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-hooks");

    let hook_result = tasty.call(
        "hook.set",
        json!({"surface_id": sid, "event": "bell", "command": "echo hooked"}),
    );
    let hook_id = hook_result["hook_id"].as_u64().unwrap();
    assert!(hook_id > 0);

    let hooks = tasty.call("hook.list", json!({}));
    assert!(
        hooks
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h["id"].as_u64() == Some(hook_id) || h["hook_id"].as_u64() == Some(hook_id)),
        "hook.list 가 방금 만든 hook={hook_id} 를 빠뜨림: {hooks:?}"
    );

    tasty.call("hook.unset", json!({"hook_id": hook_id}));
    let hooks_after = tasty.call("hook.list", json!({}));
    assert!(
        hooks_after
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["id"].as_u64() != Some(hook_id) && h["hook_id"].as_u64() != Some(hook_id)),
        "unset 후에도 hook={hook_id} 가 남아있다: {hooks_after:?}"
    );
}

#[test]
fn structural_mutations_within_one_workspace() {
    let (tasty, ws, _sid, pid, _lane) = scenario("e2e-structural");

    // 다른 시나리오가 동시에 페인을 바꾸므로 이 워크스페이스의 페인만 센다.
    let panes_in_ws = || {
        tasty
            .call("pane.list", json!({}))
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter(|p| p["workspace_id"].as_u64() == Some(ws.id))
            .count()
    };

    let panes_before = panes_in_ws();
    let split_result = tasty.call(
        "split",
        json!({"level": "pane", "direction": "vertical", "target_pane": pid}),
    );
    let new_pane_id = split_result["new_pane_id"].as_u64().unwrap();
    assert_eq!(panes_in_ws(), panes_before + 1);

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

    let close_pane_result = tasty.call("pane.close", json!({"pane_id": new_pane_id}));
    assert_eq!(close_pane_result["closed"], true);
    assert_eq!(panes_in_ws(), panes_before);

    let result = tasty.call("pane.close", json!({"pane_id": pid}));
    assert_eq!(result["closed"], false);

    let tab_list = tasty.call("tab.list", json!({"pane_id": pid}));
    let sole_tab_id = tab_list["tabs"].as_array().unwrap()[0]["id"]
        .as_u64()
        .unwrap();
    let result = tasty.call("tab.close", json!({"tab_id": sole_tab_id}));
    assert_eq!(result["closed"], false);
}

#[test]
fn workspace_create_appears_in_the_list() {
    let _lane = lane();
    let tasty = common::shared();
    let created = tasty.call("workspace.create", json!({"name": "e2e-ws-create"}));
    let ws_id = created["id"].as_u64().expect("workspace.create returns id");
    let rows = tasty
        .call("workspace.list", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        rows.iter().any(|w| w["id"].as_u64() == Some(ws_id)),
        "workspace.list 가 방금 만든 workspace={ws_id} 를 빠뜨림: {rows:?}"
    );
}

#[test]
fn tab_close_guard_is_tab_scoped_not_pane_scoped() {
    let (tasty, _ws, sid, pid, _lane) = scenario("e2e-tab-close-guard");

    let tab_list = tasty.call("tab.list", json!({"pane_id": pid}));
    let own_tab_id = tab_list["tabs"].as_array().unwrap()[0]["id"]
        .as_u64()
        .unwrap();

    tasty.call("tab.create", json!({"pane_id": pid}));
    let tab_list = tasty.call("tab.list", json!({"pane_id": pid}));
    let sibling_tab_id = tab_list["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"].as_u64().unwrap() != own_tab_id)
        .unwrap()["id"]
        .as_u64()
        .unwrap();

    let result = tasty.call(
        "tab.close",
        json!({"tab_id": sibling_tab_id, "caller_surface_id": sid}),
    );
    assert_eq!(result["closed"], true);

    // 마지막 탭 보호와 구별하기 위해 형제 탭을 다시 만든 뒤 자기 탭 닫기를 확인한다.
    tasty.call("tab.create", json!({"pane_id": pid}));
    let result = tasty.call_raw(
        "tab.close",
        json!({"tab_id": own_tab_id, "caller_surface_id": sid}),
    );
    assert!(
        result.get("error").is_some(),
        "expected tab.close to refuse closing the caller's own tab, got {result:?}"
    );
}

#[test]
fn renderer_resolves_dim_and_plain_glyph_colors() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-glyph-color");

    // 화면을 지우고 첫 행에 일반 Z와 dim Z를 순서대로 주입한다.
    let dim_seq = "1b5b324a1b5b485a1b5b326d5a1b5b306d";
    let fed = tasty.call(
        "debug.feed_bytes",
        json!({"surface_id": sid, "bytes": dim_seq}),
    );
    assert!(fed["fed"].as_u64().unwrap() > 0);

    let plain = tasty.call(
        "debug.cell_info",
        json!({"surface_id": sid, "row": 0, "col": 0}),
    );
    assert_eq!(plain["text"], "Z", "plain cell text mismatch");
    assert_eq!(
        plain["intensity"], "normal",
        "plain cell should have intensity=normal"
    );

    let dim = tasty.call(
        "debug.cell_info",
        json!({"surface_id": sid, "row": 0, "col": 1}),
    );
    assert_eq!(dim["text"], "Z", "dim cell text mismatch");
    assert_eq!(
        dim["intensity"], "half",
        "dim cell should have intensity=half (SGR 2 reached termwiz)"
    );

    let plain_color = tasty.call(
        "debug.glyph_color",
        json!({"surface_id": sid, "row": 0, "col": 0}),
    );
    let dim_color = tasty.call(
        "debug.glyph_color",
        json!({"surface_id": sid, "row": 0, "col": 1}),
    );
    assert_eq!(plain_color["in_bounds"], true);
    assert_eq!(dim_color["in_bounds"], true);

    let plain_fg = &plain_color["fg"];
    let dim_fg = &dim_color["fg"];

    let pr = plain_fg["r"].as_f64().unwrap();
    let pg = plain_fg["g"].as_f64().unwrap();
    let pb = plain_fg["b"].as_f64().unwrap();
    assert!(
        pr > 0.5 && pg > 0.5 && pb > 0.5,
        "plain fg should be bright"
    );

    assert_ne!(
        plain_fg, dim_fg,
        "dim cell fg should differ from plain cell fg (SGR 2 must dim)",
    );

    // dim 전경색은 각 채널에서 일반 전경색과 배경색 사이에 있어야 한다.
    let plain_bg = &plain_color["bg"];
    let br = plain_bg["r"].as_f64().unwrap();
    let bg = plain_bg["g"].as_f64().unwrap();
    let bb = plain_bg["b"].as_f64().unwrap();
    let dr = dim_fg["r"].as_f64().unwrap();
    let dg = dim_fg["g"].as_f64().unwrap();
    let db = dim_fg["b"].as_f64().unwrap();
    let between = |a: f64, b: f64, x: f64| (a.min(b)..=a.max(b)).contains(&x);
    assert!(
        between(pr, br, dr) && between(pg, bg, dg) && between(pb, bb, db),
        "dim fg ({dr},{dg},{db}) must lie between plain fg ({pr},{pg},{pb}) and bg ({br},{bg},{bb})",
    );
}

#[test]
fn error_paths_reject_malformed_calls() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-error-paths");

    let resp = tasty.call_raw("nonexistent.method", json!({}));
    assert!(resp.get("error").is_some());
    assert_eq!(resp["error"]["code"].as_i64().unwrap(), -32601);

    let resp = tasty.call_raw(
        "surface.send_combo",
        json!({"surface_id": sid, "modifiers": ["ctrl"]}),
    );
    assert!(resp.get("error").is_some());

    let resp = tasty.call_raw(
        "surface.send_to",
        json!({"surface_id": 99999, "text": "hello"}),
    );
    assert!(resp.get("error").is_some());

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
            .call_raw("surface.completion", json!({}))
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

#[test]
fn headless_pty_spawn_write_wait_kill() {
    let _lane = lane();
    let tasty = common::shared();

    let spawned = tasty.call("pty.spawn", json!({}));
    let pty_id = spawned["pty_id"]
        .as_u64()
        .expect("pty.spawn returns pty_id");
    assert!(
        pty_id >= 0x8000_0000,
        "pty id 는 surface id 와 disjoint 한 고범위여야: {pty_id}"
    );

    let listed = tasty.call("pty.list", json!({}));
    assert!(
        listed["ptys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"].as_u64() == Some(pty_id)),
        "pty.list 가 방금 spawn 한 pty 를 빠뜨림: {listed:?}"
    );

    tasty.call(
        "pty.write",
        json!({ "id": pty_id, "text": "echo PTY_E2E_MARK\n" }),
    );
    let read_start = std::time::Instant::now();
    loop {
        let r = tasty.call("pty.read", json!({ "id": pty_id }));
        if r["text"].as_str().unwrap_or("").contains("PTY_E2E_MARK") {
            break;
        }
        if read_start.elapsed() > Duration::from_secs(5) {
            panic!("pty.read 가 echo 출력을 반영하지 못함: {r:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    tasty.call("pty.write", json!({ "id": pty_id, "text": "exit 5\n" }));
    let wait_start = std::time::Instant::now();
    let exited = loop {
        let w = tasty.call("pty.wait", json!({ "id": pty_id }));
        if w["exited"].as_bool() == Some(true) {
            break w;
        }
        if wait_start.elapsed() > Duration::from_secs(10) {
            let r = tasty.call("pty.read", json!({ "id": pty_id }));
            let screen = r["text"].as_str().unwrap_or("<pty.read 실패>");
            let tail: String = screen
                .chars()
                .rev()
                .take(48)
                .collect::<Vec<char>>()
                .into_iter()
                .rev()
                .collect();
            panic!(
                "pty.wait 가 종료를 감지하지 못함: {w:?} — 관측(pty.read): 화면 {}(scrollback={}, \
                 alt_screen={}), 꼬리=\"{}\"",
                if screen.trim().is_empty() {
                    "빈 화면"
                } else {
                    "화면 내용 있음"
                },
                r["scrollback_len"],
                r["alt_screen"],
                tail.escape_default(),
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(exited["exit_code"].as_i64(), Some(5), "진짜 exit-code 회수");
    assert_eq!(exited["success"], false);

    tasty.call("pty.kill", json!({ "id": pty_id }));
    let listed2 = tasty.call("pty.list", json!({}));
    assert!(
        listed2["ptys"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["id"].as_u64() != Some(pty_id)),
        "kill 후 pty.list 에 남아있으면 안 됨: {listed2:?}"
    );
}

#[test]
fn headless_pty_attach_surface_promotes_to_a_tab() {
    let (tasty, _ws, _sid, pid, _lane) = scenario("e2e-pty-promote");

    let promo = tasty.call("pty.spawn", json!({}));
    let promo_id = promo["pty_id"].as_u64().expect("pty.spawn returns pty_id");
    let attached = tasty.call(
        "pty.attach_surface",
        json!({ "id": promo_id, "pane_id": pid }),
    );
    let new_surface = attached["surface_id"]
        .as_u64()
        .expect("attach_surface returns surface_id");
    assert_eq!(attached["pane_id"].as_u64(), Some(pid));
    let surfaces_now = tasty.call("surface.list", json!({}));
    assert!(
        surfaces_now
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"].as_u64() == Some(new_surface)),
        "승격된 surface 가 surface.list 에 없음: {surfaces_now:?}"
    );
    let listed = tasty.call("pty.list", json!({}));
    assert!(
        listed["ptys"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["id"].as_u64() != Some(promo_id)),
        "승격된 pty 는 headless 목록에서 빠져야 함: {listed:?}"
    );
}

/// window.list에 표시 여부가 없어 X 서버의 IsViewable을 직접 조회한다.
/// 하네스 디스플레이를 사용한다. inherit + Wayland에서는 Xlib로 다른 종류의 ID를 조회하면 프로세스가 종료될 수 있어 None을 반환한다.
#[cfg(all(target_os = "linux", feature = "gui"))]
fn x11_window_is_viewable(xid: u64) -> Result<Option<bool>, String> {
    use x11_dl::xlib;
    let declared = std::env::var("TASTY_E2E_DISPLAY").unwrap_or_default();
    let name = if declared.trim() == "inherit" {
        if std::env::var("WAYLAND_DISPLAY").is_ok_and(|v| !v.is_empty()) {
            return Ok(None);
        }
        std::env::var("DISPLAY").map_err(|_| "DISPLAY is not set".to_string())?
    } else {
        declared.trim().to_string()
    };
    let x = xlib::Xlib::open().map_err(|e| format!("Xlib::open: {e}"))?;
    let cname = std::ffi::CString::new(name.clone()).map_err(|e| e.to_string())?;
    // SAFETY: cname은 NUL 종단 문자열이며 반환된 포인터가 null인지 확인한다.
    let dpy = unsafe { (x.XOpenDisplay)(cname.as_ptr()) };
    if dpy.is_null() {
        return Err(format!("XOpenDisplay({name}) failed"));
    }
    // SAFETY: XWindowAttributes는 정수·포인터로 구성된 C 구조체이며 0으로 초기화할 수 있다.
    let mut attrs: xlib::XWindowAttributes = unsafe { std::mem::zeroed() };
    // SAFETY: dpy는 열린 연결이고 xid는 같은 디스플레이에서 만든 X11 창 ID다. attrs는 쓰기 가능한 지역 변수다.
    let status = unsafe { (x.XGetWindowAttributes)(dpy, xid as xlib::Window, &mut attrs) };
    // SAFETY: 열린 연결을 닫고 이후 dpy를 사용하지 않는다.
    unsafe { (x.XCloseDisplay)(dpy) };
    if status == 0 {
        return Err(format!("XGetWindowAttributes(0x{xid:x}) failed"));
    }
    Ok(Some(attrs.map_state == xlib::IsViewable))
}

/// 5초 안에 창이 보이는지 확인한다. Wayland로 조회를 생략하면 경고를 남긴다.
#[cfg(all(target_os = "linux", feature = "gui"))]
fn wait_x11_window_viewable(xid: u64) {
    let start = std::time::Instant::now();
    loop {
        match x11_window_is_viewable(xid) {
            Ok(None) => {
                tracing::warn!("inherit + Wayland — skipping the agent window map state check");
                return;
            }
            Ok(Some(true)) => return,
            Ok(Some(false)) if start.elapsed() < Duration::from_secs(5) => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Ok(Some(false)) => {
                panic!("에이전트 창 0x{xid:x} 이 5 초 안에 보이지(IsViewable) 않았다")
            }
            Err(e) => panic!("에이전트 창 0x{xid:x} 의 map state 를 못 읽었다: {e}"),
        }
    }
}

/// 창 생성이 필요해 헤드리스 CI에서는 이 시나리오를 제외한다.
/// 제외 이름의 일치는 headless_skip_names_are_exact 검사에서 확인한다.
#[test]
fn multi_window_owner_routing() {
    // 포커스가 첫 창에 남은 상태에서 새 창의 서피스에 요청해 소유 창 라우팅을 확인한다.
    let _lane = exclusive_lane();
    let tasty = common::shared();
    let ws = tasty.create_workspace("e2e-multi-window");
    let sid = ws.surface_id;
    tasty.wait_for_shell(sid);

    let ids_before: std::collections::HashSet<u64> = tasty
        .call("surface.list", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s["id"].as_u64())
        .collect();

    let focused_window = |tasty: &common::TastyInstance| -> Option<u64> {
        tasty
            .call("window.list", json!({}))
            .as_array()
            .and_then(|ws| ws.iter().find(|w| w["focused"] == true))
            .and_then(|w| w["id"].as_u64())
    };
    let focused_before = focused_window(tasty);
    assert!(
        focused_before.is_some(),
        "창 생성 전 포커스된 창을 찾지 못해 포커스 유지 여부를 비교할 수 없다"
    );

    let create_resp = tasty.call("window.create", json!({}));
    assert_eq!(
        create_resp["created"], true,
        "window.create 성공 응답이 created=true 를 실어야 한다: {create_resp:?}"
    );
    assert!(
        create_resp["window_id"].as_u64().is_some(),
        "window.create 성공 응답에 window_id 가 있어야 한다: {create_resp:?}"
    );
    assert_eq!(
        focused_window(tasty),
        focused_before,
        "window.create 뒤 window.list 의 focused 는 원래 창이어야 한다: {create_resp:?}"
    );
    #[cfg(all(target_os = "linux", feature = "gui"))]
    wait_x11_window_viewable(
        create_resp["window_id"]
            .as_u64()
            .expect("window.create 성공 응답에 window_id 가 있어야 한다"),
    );

    let start = std::time::Instant::now();
    let new_sid = loop {
        let arr = tasty
            .call("surface.list", json!({}))
            .as_array()
            .cloned()
            .unwrap_or_default();
        let new_ids: Vec<u64> = arr
            .iter()
            .filter_map(|s| s["id"].as_u64())
            .filter(|id| !ids_before.contains(id))
            .collect();
        if let Some(&id) = new_ids.first() {
            if arr
                .iter()
                .any(|s| s["id"].as_u64() == Some(id) && s["pty_ready"].as_bool() == Some(true))
            {
                break id;
            }
        }
        if start.elapsed() > Duration::from_secs(10) {
            panic!("second window surface did not appear in 10s. surface.list = {arr:?}");
        }
        std::thread::sleep(Duration::from_millis(100));
    };

    let surfaces = tasty
        .call("surface.list", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        surfaces.iter().any(|s| s["id"].as_u64() == Some(sid)),
        "surface.list 가 첫 윈도우의 surface={sid} 를 빠뜨림: {surfaces:?}"
    );
    assert!(
        surfaces.iter().any(|s| s["id"].as_u64() == Some(new_sid)),
        "surface.list 가 두번째 윈도우의 surface={new_sid} 를 빠뜨림: {surfaces:?}"
    );

    let workspaces = tasty
        .call("workspace.list", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        workspaces.len() >= 2,
        "workspace.list 가 모든 engine 의 workspace 를 합쳐 반환해야: {workspaces:?}"
    );

    let ws_ids: std::collections::HashSet<u64> =
        workspaces.iter().filter_map(|w| w["id"].as_u64()).collect();
    let tree = tasty
        .call("tree", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default();
    let tree_ids: std::collections::HashSet<u64> =
        tree.iter().filter_map(|w| w["id"].as_u64()).collect();
    assert_eq!(
        tree_ids, ws_ids,
        "tree와 workspace.list의 워크스페이스 집합이 다르다. 창별 수집 범위를 확인한다. tree={tree:?} workspace.list={workspaces:?}"
    );

    tasty.set_mark(sid);
    let send_first = tasty.call(
        "surface.send",
        json!({"surface_id": sid, "text": "echo W1_owner_route\n"}),
    );
    assert_eq!(
        send_first["sent"], true,
        "owner-based routing 으로 첫 윈도우 surface 에 send 가능해야: {send_first:?}"
    );
    let out = tasty.wait_for_output(sid, "W1_owner_route", Duration::from_secs(5));
    assert!(
        out.contains("W1_owner_route"),
        "첫 윈도우 surface 가 명령을 실행하지 못함: {out:?}"
    );

    tasty.set_mark(new_sid);
    let send_second = tasty.call(
        "surface.send",
        json!({"surface_id": new_sid, "text": "echo W2_owner_route\n"}),
    );
    assert_eq!(send_second["sent"], true);
    let out2 = tasty.wait_for_output(new_sid, "W2_owner_route", Duration::from_secs(5));
    assert!(out2.contains("W2_owner_route"));

    // 잠금은 남은 창을 정리하지 않는다. 뒤의 시나리오에 영향을 주지 않도록 닫힘까지 확인한다.
    let win_id = create_resp["window_id"]
        .as_u64()
        .expect("window.create 가 window_id 를 줬다");
    let close_resp = tasty.call_raw("window.close", json!({ "id": win_id }));
    assert!(
        close_resp.get("error").is_none(),
        "만든 창을 닫지 못하면 잔여가 남는다: {close_resp}"
    );
    let closing = std::time::Instant::now();
    loop {
        let n = tasty
            .call("window.list", json!({}))
            .as_array()
            .map(|a| a.len())
            .unwrap_or(0);
        if n <= 1 {
            break;
        }
        assert!(
            closing.elapsed() < Duration::from_secs(5),
            "창을 닫으라고 했는데 5 초가 지나도 window.list 가 {n} 이다"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 플러그인 매니저를 실제로 초기화한 인스턴스에서 목록 조회 응답을 확인한다.
#[test]
fn plugin_list_answers_without_a_window() {
    let _lane = lane();
    let tasty = common::shared();
    let resp = tasty.call_raw("plugin.list", json!({}));

    assert!(
        resp.get("error").is_none(),
        "plugin.list 가 에러로 답했다: {resp}"
    );
    let plugins = resp
        .get("result")
        .and_then(|r| r.get("plugins"))
        .and_then(|p| p.as_array())
        .unwrap_or_else(|| panic!("plugin.list 응답에 plugins 배열이 없다: {resp}"));

    // 설치 개수는 홈 상태에 따라 달라지므로 배열과 항목 형태만 확인한다.
    for p in plugins {
        assert!(
            p.get("id").and_then(|v| v.as_str()).is_some(),
            "plugin 항목에 id 가 없다: {p}"
        );
    }
}

/// 미설치 플러그인 오류와 매니저 부재 오류를 구별한다. 매니저 초기화를 빠뜨려도 통과하지 않아야 한다.
#[test]
fn plugin_show_distinguishes_an_unknown_plugin_from_a_missing_manager() {
    let _lane = lane();
    let tasty = common::shared();
    let resp = tasty.call_raw(
        "plugin.show",
        json!({"id": "no-such-plugin-in-any-installation"}),
    );

    let err = resp
        .get("error")
        .unwrap_or_else(|| panic!("없는 plugin 인데 성공으로 답했다: {resp}"));
    let code = err.get("code").and_then(|c| c.as_i64());
    assert_eq!(
        code,
        Some(-32003),
        "없는 plugin 은 -32003 이어야 한다(-32000 이면 매니저 자체가 안 세워진 것): {resp}"
    );
}

/// 공유 인스턴스의 플러그인 상태를 바꾸지 않도록 id 없이 호출한다. 인자 오류 응답으로 핸들러가 있는지 확인한다.
#[test]
fn lifecycle_toggles_answer_without_a_window() {
    let _lane = lane();
    let tasty = common::shared();
    for method in ["plugin.enable", "plugin.disable"] {
        let resp = tasty.call_raw(method, json!({}));
        let code = resp
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_i64());
        assert_eq!(
            code,
            Some(-32602),
            "{method} 는 arm 이 있어 인자 오류로 답해야 한다. `-32017` 이면 그 조합에 \
             arm 이 없는 것이고, `-32601` 이면 표에서 이름이 빠진 것이다: {resp}"
        );
    }
}

/// 헤드리스의 remove·grant는 지원하지 않는다. 조회·토글 허용 범위와 구별해 검사한다.
#[cfg(not(feature = "gui"))]
#[test]
fn the_remaining_lifecycle_methods_are_still_absent_in_a_headless_daemon() {
    let _lane = lane();
    let tasty = common::shared();
    for method in ["plugin.remove", "plugin.grant"] {
        let resp = tasty.call_raw(method, json!({"id": "anything"}));
        let code = resp
            .get("error")
            .and_then(|e| e.get("code"))
            .and_then(|c| c.as_i64());
        assert_eq!(
            code,
            Some(-32017),
            "헤드리스에서 {method} 는 아직 arm 이 없는 메서드여야 한다. `-32601` 이 \
             왔다면 표에서 이름이 빠진 것이고, 그러면 호출자가 오타와 구분할 수 없다: {resp}"
        );
    }
}

/// 헤드리스에는 파일 식별 결과를 열 창이 없으므로 dispatch를 수락하면 안 된다.
#[cfg(not(feature = "gui"))]
#[test]
fn file_dispatch_is_refused_rather_than_accepted_in_a_headless_daemon() {
    let _lane = lane();
    let tasty = common::shared();
    let resp = tasty.call_raw(
        "file_handler.dispatch",
        json!({"path": "/definitely/not/a/tasty/test/page.html", "depth": "cheap"}),
    );
    let code = resp
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_i64());
    assert_eq!(
        code,
        Some(-32017),
        "헤드리스는 파일을 열 수 없으니 dispatch 를 수락했다고 답하면 안 된다: {resp}"
    );
    assert!(
        resp.get("result").is_none(),
        "거절 응답에 result 가 같이 실리면 호출자가 성공으로 읽는다: {resp}"
    );
}

/// 헤드리스에는 mirror 요청 큐를 attach로 보내는 GUI 경로가 없어 수락하면 결과를 돌려줄 수 없다.
/// mirror 구조 변경 거절은 core::attach_runtime의 별도 단위 시험에서 확인한다.
#[cfg(not(feature = "gui"))]
#[test]
fn mirror_forward_requests_are_refused_by_name_in_a_headless_daemon() {
    let _lane = lane();
    let tasty = common::shared();
    let mut messages = Vec::new();
    for (method, params) in [
        (
            "git_viewer.query",
            json!({"kind": "status", "local_surface_id": 1}),
        ),
        ("markdown_mirror.content_request", json!({"surface_id": 1})),
    ] {
        let resp = tasty.call_raw(method, params);
        let error = resp.get("error");
        assert_eq!(
            error.and_then(|e| e.get("code")).and_then(|c| c.as_i64()),
            Some(-32017),
            "헤드리스는 {method} 를 수락하면 안 된다 — 결과를 보낼 쪽이 없다: {resp}"
        );
        assert!(
            resp.get("result").is_none(),
            "거절 응답에 result 가 같이 실리면 호출자가 성공으로 읽는다: {resp}"
        );
        let message = error
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or_default()
            .to_string();
        assert!(
            message.contains(method),
            "거절 문구가 무엇이 거절됐는지 말해야 한다: {message}"
        );
        messages.push(message);
    }
    assert_ne!(
        messages[0], messages[1],
        "두 자리의 거절 사유가 같은 문자열이면 무엇이 거절했는지 못 가른다"
    );
}

/// 헤드리스는 레이아웃 슬롯을 점유하지 않는다. 필드 누락과 명시적인 null을 구별한다.
#[cfg(not(feature = "gui"))]
#[test]
fn a_headless_daemon_answers_that_it_holds_no_layout_slot() {
    let _lane = lane();
    let tasty = common::shared();
    let info = tasty.call("system.info", json!({}));
    assert!(
        info.as_object()
            .is_some_and(|o| o.contains_key("layout_slot")),
        "system.info 가 layout_slot 칸을 아예 안 냈다 — 소비자 쪽에서 null 과 구별되지 않는다: {info}"
    );
    assert_eq!(
        info["layout_slot"],
        serde_json::Value::Null,
        "헤드리스는 레이아웃 슬롯을 잡지 않는다: {info}"
    );
}

/// 실제 부팅 로그가 기본 warn 필터에서 복원 미지원 안내를 내보내는지 확인한다.
/// 메시지 반환값만 검사하면 호출 누락이나 로그 레벨 변경을 찾지 못한다.
#[cfg(not(feature = "gui"))]
#[test]
fn a_headless_daemon_warns_at_boot_that_restore_layout_is_ignored() {
    let _lane = lane();
    let tasty = common::TastyInstance::spawn_with_restore_layout();
    // 메인 루프 응답을 기다려 부팅 안내 이후에 검사한다. 이어지는 폴링은 stderr 수집 지연을 기다린다.
    tasty.call("system.info", json!({}));
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let line = loop {
        if let Some(line) = tasty.find_stderr(|l| l.contains("general.restore_layout is on")) {
            break line;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "헤드리스 부팅 로그에서 restore_layout 미지원 안내를 찾지 못했다. 호출·로그 레벨·수집 상태를 확인한다:\n{}",
            tasty.startup_diagnostics()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    // 파이프에도 ANSI 색이 들어올 수 있어 레벨 토큰 비교 전에 제거한다.
    let mut plain = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            plain.push(c);
        }
    }
    assert!(
        plain.split_whitespace().any(|token| token == "WARN"),
        "부팅 고지가 warn 이 아니다 — 기본 필터가 warn 이라 이 줄이 보인 것은 필터가 바뀐 탓일 수 있다: {plain}"
    );
    assert!(
        plain.contains("does not save or restore layouts"),
        "고지가 무엇을 안 하는지 말하지 않는다: {plain}"
    );
}

/// OS 열기는 기존 브라우저 등 사용자 프로세스에 영향을 줄 수 있어 debug 기록 모드로만 검사한다.
/// 이 모드가 없는 release 빌드에서는 실행하지 않는다. 격리 홈과 디스플레이만으로 OS 열기를 막을 수는 없다.
#[cfg(all(feature = "gui", debug_assertions))]
#[test]
fn directory_dispatch_is_recorded_instead_of_opened_under_the_harness() {
    let _lane = lane();
    let tasty = common::shared();
    let dir = tasty.tasty_home().join("os-open-probe-dir");
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let resp = tasty.call_raw(
        "file_handler.dispatch",
        json!({"path": dir.to_str().unwrap(), "depth": "cheap"}),
    );
    assert_eq!(
        resp.pointer("/result/accepted"),
        Some(&json!(true)),
        "gui 는 디렉토리 dispatch 를 접수해야 한다: {resp}"
    );
    let log = tasty.os_open_log();
    let needle = dir.to_str().unwrap().to_string();
    let deadline = std::time::Instant::now() + Duration::from_secs(15);
    loop {
        let text = std::fs::read_to_string(&log).unwrap_or_default();
        if text
            .lines()
            .any(|l| l.starts_with("open_uri\t") && l.contains(&needle))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "디렉토리 OS 열기가 {} 에 기록되지 않았다 — 기록 대신 실제로 띄웠을 수 있다. 기록 내용: {text:?}",
            log.display()
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 공유 user 설정을 바꾸므로 단독 실행하고 파일 삭제·reload로 복구한다.
#[test]
fn file_handler_reload_reports_the_entries_it_dropped() {
    let _lane = exclusive_lane();
    let tasty = common::shared();
    let path = tasty.tasty_home().join("file-handlers.toml");
    assert!(
        !path.exists(),
        "격리 홈에 user 설정이 이미 있으면 복원 기준이 없다: {}",
        path.display()
    );
    std::fs::write(
        &path,
        r#"
[[handler]]
id = "md-as-html"
detector = "markdown"
[handler.action]
kind = "system"

[[handler]]
id = "user/no-action"
detector = "markdown"

[[handler]]
id = "user/good"
detector = "markdown"
[handler.action]
kind = "system"

[[handler]]
id = "com.example.absent/viewer"
priority = 10
"#,
    )
    .expect("user 설정 쓰기");

    let resp = tasty.call("file_handler.reload", json!({}));

    // 응답 단정에 실패해도 설정이 남지 않도록 먼저 복구한다.
    std::fs::remove_file(&path).expect("user 설정 지우기");
    let restored = tasty.call("file_handler.reload", json!({}));

    assert_eq!(resp["exists"], json!(true), "기존 필드는 그대로다: {resp}");
    assert!(resp["path"].is_string(), "기존 필드는 그대로다: {resp}");
    assert_eq!(
        resp["rejected"],
        json!([
            {"id": "md-as-html", "reason": "missing_owner_prefix"},
            {"id": "user/no-action", "reason": "missing_detector_or_action"},
            {"id": "com.example.absent/viewer", "reason": "target_not_contributed"},
        ]),
        "적용하지 않은 항목과 사유가 응답에 있어야 한다: {resp}"
    );
    assert_eq!(
        restored["rejected"],
        json!([]),
        "적용하지 않은 것이 없으면 빈 배열이다: {restored}"
    );
}

/// 병합 결과뿐 아니라 출처별 원본도 확인한다. 공용 설정을 바꾸므로 단독 실행하고 원래 상태로 복구한다.
#[test]
fn file_handler_detectors_reports_the_merged_state_and_each_source() {
    let _lane = exclusive_lane();
    let tasty = common::shared();
    let path = tasty.tasty_home().join("file-handlers.toml");
    assert!(
        !path.exists(),
        "격리 홈에 user 설정이 이미 있으면 복원 기준이 없다: {}",
        path.display()
    );
    let find = |resp: &serde_json::Value, id: &str| -> serde_json::Value {
        resp["detectors"]
            .as_array()
            .and_then(|a| a.iter().find(|d| d["id"] == json!(id)).cloned())
            .unwrap_or_else(|| panic!("`{id}` detector 가 목록에 없다: {resp}"))
    };

    let before = tasty.call("file_handler.detectors", json!({}));
    let html = find(&before, "html");
    assert_eq!(
        html["contributions"]
            .as_array()
            .map(|a| a.iter().map(|c| c["origin"].clone()).collect::<Vec<_>>()),
        Some(vec![json!("host")]),
        "user 설정이 없으면 host 한 출처다: {html}"
    );
    assert_eq!(html["disabled"], json!(false), "{html}");
    assert!(
        html["rules"].as_array().is_some_and(|r| r
            .iter()
            .any(|r| r["kind"] == json!("extension") && r["origin"] == json!("host"))),
        "rule 은 설정 파일 키와 출처로 적힌다: {html}"
    );

    std::fs::write(
        &path,
        r#"
[[detector]]
id = "html"
display_name_i18n_key = "user.html"
"#,
    )
    .expect("user 설정 쓰기");
    tasty.call("file_handler.reload", json!({}));
    let after = tasty.call("file_handler.detectors", json!({}));

    std::fs::remove_file(&path).expect("user 설정 지우기");
    tasty.call("file_handler.reload", json!({}));
    let restored = tasty.call("file_handler.detectors", json!({}));

    let html = find(&after, "html");
    assert_eq!(
        html["display_name_i18n_key"],
        json!("user.html"),
        "user patch 가 병합 결과에 반영된다: {html}"
    );
    let contribs = html["contributions"].as_array().expect("contributions");
    assert_eq!(contribs.len(), 2, "host · user 두 출처: {html}");
    assert!(
        contribs.iter().any(|c| c["origin"] == json!("user")
            && c["display_name_i18n_key"] == json!("user.html")
            && c["disabled"].is_null()),
        "user 원본이 적은 그대로 실린다(켜기/끄기는 안 적어 null): {html}"
    );
    assert_eq!(
        find(&restored, "html")["contributions"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "user 설정을 지우고 reload 하면 host 한 출처로 돌아간다: {restored}"
    );
}

/// 없는 대상을 지정한 요청은 다른 창으로 넘기지 않고 두 빌드 조합에서 모두 거절해야 한다.
#[test]
fn a_request_naming_an_unowned_target_is_rejected() {
    let _lane = lane();
    let tasty = common::shared();

    let resp = tasty.call_raw(
        "workspace.create",
        json!({ "workspace_id": 999_999, "name": "unowned-target-probe" }),
    );
    assert!(
        resp.get("error").is_some(),
        "없는 대상을 지정한 요청이 거절되지 않았다: {resp}"
    );
    let msg = resp["error"]["message"].as_str().unwrap_or_default();
    assert!(
        msg.contains("999999") && msg.contains("workspace"),
        "에러가 무엇을 못 찾았는지 말해야 고칠 수 있다: {resp}"
    );

    // image 플러그인이 host로 전달하는 호출도 확인한다. 헤드리스에는 해당 host 핸들러가 없어 이 경우는 GUI에서만 검사한다.
    #[cfg(feature = "gui")]
    {
        let via_plugin = tasty.call_raw(
            "image.open",
            json!({ "surface_id": 999_999, "path": "/tmp/does-not-exist.png" }),
        );
        let via_msg = via_plugin
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        assert!(
            via_msg.contains("surface 999999"),
            "plugin 을 경유한 호출도 지목한 대상이 없으면 같은 이유로 거절돼야 한다: \
             {via_plugin}"
        );
    }

    // 플러그인 소유 namespace는 호스트 대상 검사로 차단하지 않고 플러그인에 전달한다.
    let forwarded = tasty.call_raw("markdown.recent", json!({ "surface_id": 999_999 }));
    assert!(
        forwarded.get("result").is_some(),
        "plugin namespace 의 메서드는 id 를 실어도 forward 돼야 한다: {forwarded}{}",
        common::bundle_staging_note()
    );

    let ok = tasty.call_raw("workspace.create", json!({ "name": "no-target-probe" }));
    assert!(
        ok.get("result").is_some(),
        "대상을 지목하지 않은 생성 요청은 계속 동작해야 한다: {ok}"
    );
}

/// 환경 의존적인 성공 대신 메서드 부재·빌드 미지원 오류가 아닌지 검사해 라우팅을 확인한다.
#[test]
fn app_layer_methods_that_need_no_window_answer_in_both_combos() {
    let _lane = lane();
    let tasty = common::shared();

    for method in [
        "clipboard.set_text",
        "remote.workspaces",
        "agent.task_await",
        "approval.await",
    ] {
        let resp = tasty.call_raw(method, json!({}));
        let code = resp["error"]["code"].as_i64();
        // 메서드 표 누락과 핸들러 누락은 다른 코드이므로 둘 다 확인한다.
        assert_ne!(
            code,
            Some(-32601),
            "`{method}` 는 창이 없어도 답이 정의된다 — 두 조합에서 라우팅돼야 한다: {resp}"
        );
        assert_ne!(
            code,
            Some(-32017),
            "`{method}` 의 arm 이 이 조합에서 사라졌다 — 창을 안 보는데 게이트 뒤로 \
             들어갔다는 뜻이다: {resp}"
        );
    }

    let resp = tasty.call_raw("window.list", json!({}));
    let code = resp["error"]["code"].as_i64();
    #[cfg(feature = "gui")]
    assert_ne!(code, Some(-32601), "gui 는 window.list 에 답한다: {resp}");
    #[cfg(not(feature = "gui"))]
    assert_eq!(
        code,
        Some(-32017),
        "헤드리스의 window.list는 빌드 미지원 오류여야 한다. 메서드 표 누락과 구별한다: {resp}"
    );
}

/// 플랫폼 미지원과 없는 메서드를 구별하는 실제 응답을 확인한다.
#[test]
fn a_platform_gated_debug_method_says_why_not_that_it_is_missing() {
    let _lane = lane();
    let tasty = common::shared();

    for method in ["surface.raw_key", "surface.switch_input_source"] {
        let resp = tasty.call_raw(method, json!({}));
        let code = resp["error"]["code"].as_i64();

        #[cfg(all(target_os = "macos", feature = "gui"))]
        assert_ne!(
            code,
            Some(-32601),
            "macOS gui 에서는 실제 핸들러가 받는다: {resp}"
        );

        #[cfg(not(all(target_os = "macos", feature = "gui")))]
        {
            assert_eq!(
                code,
                Some(-32015),
                "지원하지 않는 플랫폼에서는 메서드 부재 대신 -32015와 사유를 반환해야 한다: {resp}"
            );
            let msg = resp["error"]["message"].as_str().unwrap_or_default();
            assert!(
                msg.contains("macOS-only"),
                "코드만으로는 무엇이 부족한지 알 수 없다 — 사유가 함께 와야 한다: {resp}"
            );
        }
    }
}

/// 창 없이 가능한 theme 조회와 렌더러가 필요한 webview 변경을 구별한다.
#[test]
fn an_engine_query_that_reads_no_window_answers_in_both_combos() {
    let _lane = lane();
    let tasty = common::shared();

    let resp = tasty.call_raw("theme.query", json!({}));
    assert!(
        resp.get("result").is_some(),
        "`theme.query` 는 전역 Theme 과 settings 만 읽는다 — 두 조합에서 답해야 한다: {resp}"
    );
    assert!(
        resp["result"].get("colors").is_some(),
        "색상표가 실려야 한다 — 라우팅만 되고 빈 답이면 호출자에게 쓸모가 없다: {resp}"
    );

    let resp = tasty.call_raw("webview.set_url", json!({}));
    let code = resp["error"]["code"].as_i64();
    #[cfg(feature = "gui")]
    assert_ne!(
        code,
        Some(-32601),
        "gui 는 webview.set_url 에 답한다: {resp}"
    );
    #[cfg(not(feature = "gui"))]
    assert_eq!(
        code,
        Some(-32017),
        "헤드리스의 webview.set_url은 메서드 부재와 구별되는 빌드 미지원 오류여야 한다: {resp}"
    );
}
/// 여러 대상 종류에 없는 ID를 지정했을 때 거절하는지 확인하고, 있는 ID의 일부 요청도 함께 검사한다.
#[test]
fn an_unowned_target_is_rejected_for_every_resource_kind() {
    let _lane = lane();
    let tasty = common::shared();
    const MISSING: u64 = 999_999;

    // 플러그인 namespace는 호스트 소유 검사를 거치지 않으므로 호스트 메서드를 선택한다.
    let cases: Vec<(&str, &str, &str, serde_json::Value)> = vec![
        (
            "workspace",
            "workspace_id",
            "workspace.create",
            json!({ "name": "unowned-kind-probe" }),
        ),
        (
            "workspace",
            "target_workspace_id",
            "workspace.move",
            json!({}),
        ),
        ("surface", "surface_id", "surface.close", json!({})),
        ("surface", "surface", "terminal.children", json!({})),
        ("surface", "parent", "terminal.kill", json!({})),
        ("surface", "target", "surface.split", json!({})),
        (
            "surface",
            "to_surface_id",
            "message.send",
            json!({ "message": "x" }),
        ),
        ("tab", "tab_id", "tab.close", json!({})),
        ("pane", "pane_id", "pane.close", json!({})),
        ("pane", "pane", "tab.new", json!({})),
        (
            "pane",
            "target_pane_id",
            "split",
            json!({ "level": "pane" }),
        ),
        ("headless pty", "id", "pty.write", json!({ "data": "x" })),
        ("surface hook", "hook_id", "hook.unset", json!({})),
        (
            "output observer",
            "observer_id",
            "output.observe_stop",
            json!({}),
        ),
    ];

    for (kind, key, method, base) in &cases {
        let mut params = base.clone();
        params[*key] = json!(MISSING);
        let resp = tasty.call_raw(method, params);
        let msg = resp
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        assert!(
            !msg.contains("Method not found"),
            "{method}를 찾지 못해 대상 ID 검사에 도달하지 않았다: {resp}"
        );
        assert!(
            msg.contains(&MISSING.to_string()),
            "{key}({kind}) 를 없는 id 로 실었는데 그것을 말하는 거절이 안 나왔다 \
             — 지목한 대상이 없는데 조용히 성공했을 수 있다: {resp}"
        );
        assert!(
            msg.contains(kind),
            "거절이 **무엇을** 못 찾았는지 말해야 고칠 수 있다({kind} 를 기대): {resp}"
        );
    }

    // 있는 대상의 요청은 다른 이유로 실패해도 허용한다. 여기서는 대상 소유 검사 오류만 확인한다.
    let tree = tasty.call("tree", json!({}));
    let live_ws = tree[0]["id"].as_u64().expect("살아 있는 workspace id");
    let surfaces = tasty.call("surface.list", json!({}));
    let live_surface = surfaces
        .as_array()
        .and_then(|a| a.first())
        .and_then(|s| s["id"].as_u64())
        .expect("살아 있는 surface id");
    let panes = tasty.call("pane.list", json!({}));
    let live_pane = panes
        .as_array()
        .and_then(|a| a.first())
        .and_then(|p| p["id"].as_u64())
        .expect("살아 있는 pane id");

    let live: Vec<(&str, &str, serde_json::Value, u64)> = vec![
        (
            "workspace_id",
            "workspace.create",
            json!({ "name": "live-kind-probe" }),
            live_ws,
        ),
        ("surface", "terminal.children", json!({}), live_surface),
        ("pane", "tab.new", json!({}), live_pane),
    ];
    for (key, method, base, id) in live {
        let mut params = base.clone();
        params[key] = json!(id);
        let resp = tasty.call_raw(method, params);
        let msg = resp
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or_default();
        assert!(
            !msg.contains("no live"),
            "{key}={id} 는 살아 있는데 소유 검사가 걸렸다 — 검사가 너무 넓다: {resp}"
        );
    }
}

/// 창을 사용하지 않는 debug 메서드의 라우팅을 확인한다. 이 debug 실행으로 release 격리까지 검증하지는 않는다.
#[test]
fn debug_surfaces_that_read_no_window_answer_in_both_combos() {
    let _lane = lane();
    let tasty = common::shared();

    for method in [
        "debug.lua.eval",
        "debug.event_bus.list_subscribers",
        "debug.event_bus.publish",
        "debug.event_bus.trace",
        "debug.extension.invoke_hook",
        "debug.popup.list",
        "debug.fullscreen.list",
    ] {
        let resp = tasty.call_raw(method, json!({}));
        let code = resp["error"]["code"].as_i64();
        assert_ne!(
            code,
            Some(-32601),
            "창을 사용하지 않는 debug 메서드 {method}가 라우팅되지 않았다: {resp}"
        );
        assert_ne!(
            code,
            Some(-32017),
            "`{method}` 의 arm 이 이 조합에서 사라졌다: {resp}"
        );
    }

    // tool 조회와 fullscreen 열기는 창이 필요하다. popup은 헤드리스에 닫는 경로가 없어 열기도 허용하지 않는다.
    for method in [
        "debug.tool.list",
        "debug.popup.open",
        "debug.fullscreen.open",
    ] {
        let resp = tasty.call_raw(method, json!({}));
        let code = resp["error"]["code"].as_i64();
        #[cfg(feature = "gui")]
        assert_ne!(code, Some(-32601), "gui 는 `{method}` 에 답한다: {resp}");
        #[cfg(not(feature = "gui"))]
        assert_eq!(
            code,
            Some(-32017),
            "헤드리스에서는 이 메서드가 빌드 미지원 오류를 반환해야 한다: {resp}"
        );
    }
}

/// 여러 클라이언트의 응답을 확인한다. 기본 예산에서는 처리 중단이 보장되지 않으므로 재깨움 검증은 아래 별도 시험에서 한다.
#[test]
fn concurrent_requests_are_all_answered() {
    let _lane = lane();
    let tasty = common::shared();
    const CLIENTS: usize = 16;

    let rows: Vec<serde_json::Value> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..CLIENTS)
            .map(|_| s.spawn(|| tasty.call("workspace.list", json!({}))))
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("동시 요청 스레드가 패닉했다"))
            .collect()
    });

    assert_eq!(rows.len(), CLIENTS, "응답 수가 요청 수와 다르다");
    for (i, row) in rows.iter().enumerate() {
        assert!(
            row.as_array().is_some_and(|a| !a.is_empty()),
            "{i} 번째 동시 요청의 workspace.list 가 비었다: {row:?}"
        );
    }
}

/// debug 시간 예산을 0으로 주어 한 명령 뒤 회차를 중단하고 남은 요청의 응답을 확인한다.
/// 응답에 상한을 두어 재깨움이 누락되면 무한히 기다리지 않게 한다.
#[test]
fn concurrent_requests_are_all_answered_when_every_round_is_cut() {
    let _lane = lane();
    let tasty =
        common::TastyInstance::spawn_with_env(&[("TASTY_DEBUG_IPC_ROUND_TIME_BUDGET_MS", "0")]);
    const CLIENTS: usize = 16;
    const ANSWER_BOUND: Duration = Duration::from_secs(10);
    let port = tasty.port();

    let answers: Vec<Result<serde_json::Value, String>> = std::thread::scope(|s| {
        let handles: Vec<_> = (0..CLIENTS)
            .map(|i| {
                s.spawn(move || {
                    use std::io::{BufRead, BufReader, Write};
                    let mut stream = std::net::TcpStream::connect(("127.0.0.1", port))
                        .map_err(|e| format!("connect: {e}"))?;
                    stream
                        .set_read_timeout(Some(ANSWER_BOUND))
                        .map_err(|e| format!("timeout: {e}"))?;
                    let line = json!({"jsonrpc": "2.0", "id": i, "method": "workspace.list", "params": {}});
                    writeln!(stream, "{line}").map_err(|e| format!("write: {e}"))?;
                    let mut got = String::new();
                    BufReader::new(stream)
                        .read_line(&mut got)
                        .map_err(|e| format!("no answer within {ANSWER_BOUND:?}: {e}"))?;
                    serde_json::from_str(got.trim()).map_err(|e| format!("json: {e}: {got}"))
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("동시 요청 스레드가 패닉했다"))
            .collect()
    });

    let unanswered: Vec<String> = answers
        .iter()
        .enumerate()
        .filter_map(|(i, a)| a.as_ref().err().map(|e| format!("{i}: {e}")))
        .collect();
    assert!(
        unanswered.is_empty(),
        "처리 회차 중단 뒤 응답이 없는 요청이다. 재깨움과 연결 오류를 확인한다: {unanswered:?}"
    );
    for (i, a) in answers.iter().enumerate() {
        let resp = a.as_ref().expect("checked above");
        assert!(
            resp["result"].as_array().is_some_and(|a| !a.is_empty()),
            "{i} 번째 요청의 workspace.list 가 비었다: {resp}"
        );
    }
    let rounds = tasty.call("system.pressure", json!({}));
    assert!(
        rounds["queue_dispatch"]["rounds_stopped_by_time"]
            .as_u64()
            .is_some_and(|n| n > 0),
        "시간 예산으로 중단된 회차가 없어 재깨움 경로를 확인할 수 없다: {}",
        rounds["queue_dispatch"]
    );
}
