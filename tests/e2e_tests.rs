mod common;

use common::TastyInstance;
use serde_json::json;
use std::time::Duration;

/// 33 개 시나리오를 `#[test]` 하나에 몰아넣어 tasty 인스턴스를 1 개만 쓴다 — 창
/// spawn/kill 이 OS 포커스를 훔치기 때문이다. 이 원칙과 그 예외는
/// `docs/dev-guide/e2e-tests.md` §1(근거는 ADR-0090)에 있고
/// `tests/e2e_single_instance_guard.rs` 가 강제한다. 새로 쓰는 e2e 는 이 파일처럼
/// 전용 인스턴스를 띄우는 대신 `common::shared()` + `create_workspace()` 를 쓴다.
#[test]
#[allow(clippy::cognitive_complexity)] // complexity-exempt: 순차 e2e 스텝 나열 — 단일 tasty 인스턴스 공유(포커스 도난 최소화) 설계상 한 함수, clippy 과대계상
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
    assert!(!tabs["tabs"].as_array().unwrap().is_empty());

    // markdown.recent — 최근 markdown 목록 조회(읽기 전용, 주소창 드롭다운 공급원).
    // 격리 HOME 이라 초기 목록은 비어 있어도 무방 — 왕복 success + `recent` 배열 shape 검증.
    let recent = tasty.call("markdown.recent", json!({}));
    let recent_arr = recent["recent"]
        .as_array()
        .expect("markdown.recent returns { recent: [...] }");
    assert!(recent_arr.len() <= 10, "recent 은 최대 10개");
    // 조회가 사용자 상태(포커스 등)를 바꾸지 않았는지: 재조회가 여전히 성공.
    let recent2 = tasty.call("markdown.recent", json!({}));
    assert!(recent2["recent"].is_array());

    // notification
    tasty.call(
        "notification.create",
        json!({"title": "Test", "body": "Hello", "surface_id": sid}),
    );
    let notifs = tasty.call("notification.list", json!({}));
    assert!(!notifs.as_array().unwrap().is_empty());

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

    // send_combo
    let result = tasty.call(
        "surface.send_combo",
        json!({"surface_id": sid, "key": "x", "modifiers": ["alt"]}),
    );
    assert_eq!(result["sent"], true);
    // zsh ZLE 의 경우 Alt+X 는 execute-named-cmd 위젯을 호출하여 prompt 가
    // "execute: " 로 바뀐다. 후속 테스트가 일반 명령으로 동작하도록 Ctrl+G 로
    // mode 를 abort 시킨다. (사용자 dotfile 이 ^G 를 rebind 할 수 있으나,
    // 본 테스트는 HOME/ZDOTDIR 을 격리해 stock zsh 동작을 보장한다.)
    tasty.call(
        "surface.send_combo",
        json!({"surface_id": sid, "key": "g", "modifiers": ["ctrl"]}),
    );
    // sentinel echo 로 abort 가 실제로 풀려 prompt 가 명령을 받을 수 있는
    // 상태인지 deterministic 하게 확인.
    tasty.set_mark(sid);
    tasty.send_text(sid, "echo __abort_ok__\n");
    tasty.wait_for_output(sid, "__abort_ok__", Duration::from_secs(3));

    // ========== surface.completion (highlight producer) ==========
    // completion IPC 가 CLI→핸들러→intent→cascade 전 경로로 라우팅되어 success 를
    // 돌려주는지(=method_not_found 아님) 확인. highlight 발동 자체는 host 렌더라
    // 헤드리스로 관측 불가 — 여기선 파이프라인 도달만 검증한다.
    let completion = tasty.call("surface.completion", json!({ "surface_id": sid }));
    assert_eq!(completion["ok"], true);
    assert_eq!(completion["surface_id"].as_u64().unwrap(), sid);

    // ========== surface.attention.{get,clear} (해제 표면 왕복) ==========
    // raise 는 되지만 해제가 없던 비대칭의 회귀 가드. 해제 producer 두 개(실 렌더
    // 포커스·알림 읽음)는 전부 GUI 로컬 사건이라 IPC 로 관측/구동할 수 없어, 이
    // 왕복이 해제 축을 프로토콜 레벨에서 실행하는 유일한 경로다.
    //
    // 대상은 `sid`(첫 워크스페이스의 포커스 surface)가 아니라 **IPC 로 새로 만든
    // 워크스페이스의 surface** 다 — `gpu.rs` 가 매 렌더 프레임 실-포커스 surface 의
    // attention 을 지우므로 포커스 surface 위에서는 raise 가 프레임 하나를 못 넘긴다.
    // IPC 로 만든 워크스페이스는 active 를 전환하지 않아(원칙 1·3) 그 surface 는
    // 렌더 포커스를 얻지 않는다.
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

    // (1) raise → 조회로 kind 가 보인다.
    raise_kind("needs_input");
    assert_eq!(attention_kind(), "needs_input");

    // (2) kind 필터 불일치는 지우지 않는다 — 그 사이 더 급한 kind 로 재발동한 신호를
    //     늦게 도착한 해제가 덮지 않게 하는 계약.
    let mismatched = tasty.call(
        "surface.attention.clear",
        json!({ "surface_id": att_sid, "kind": "completion" }),
    );
    assert_eq!(mismatched["ok"], true);
    assert_eq!(mismatched["cleared"], false);
    assert_eq!(mismatched["previous_kind"], "needs_input");
    assert_eq!(attention_kind(), "needs_input");

    // (3) kind 생략 = kind 무관 해제.
    let cleared = tasty.call("surface.attention.clear", json!({ "surface_id": att_sid }));
    assert_eq!(cleared["cleared"], true);
    assert_eq!(cleared["previous_kind"], "needs_input");
    assert!(attention_kind().is_null());

    // (4) 이미 없는 상태의 재호출도 성공(idempotent).
    let again = tasty.call("surface.attention.clear", json!({ "surface_id": att_sid }));
    assert_eq!(again["ok"], true);
    assert_eq!(again["cleared"], false);
    assert!(again["previous_kind"].is_null());

    // (5) 해제 후 재발동이 정상 동작하고, 일치하는 kind 필터는 실제로 지운다.
    raise_kind("completion");
    assert_eq!(attention_kind(), "completion");
    let matched = tasty.call(
        "surface.attention.clear",
        json!({ "surface_id": att_sid, "kind": "completion" }),
    );
    assert_eq!(matched["cleared"], true);
    assert!(attention_kind().is_null());

    // (6) 존재하지 않는 surface / 알 수 없는 kind 는 명시적 에러.
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
    // surface_id 필수 (포커스 독립, 불가침 원칙 1).
    assert!(
        tasty
            .call_raw("surface.attention.clear", json!({}))
            .get("error")
            .is_some()
    );

    // ========== Dim (SGR 2) renderer regression ==========
    // printf is a posix builtin; shell on Windows is cmd.exe by default which does not
    // interpret \033 escapes the same way, so we restrict to Unix.
    #[cfg(not(windows))]
    {
        tasty.set_mark(sid);
        tasty.send_text(sid, "clear; printf '\\033[2mD\\033[0mN\\n'\n");
        tasty.wait_for_output(sid, "DN", Duration::from_secs(5));
        // Allow the renderer one frame to apply the SGR before querying cell state.
        std::thread::sleep(Duration::from_millis(200));

        // `surface.screen_text` 는 기본(`show_dim:false`)으로 dim 셀을 걸러낸다
        // (에이전트가 ghost suggestion 을 실제 입력으로 오인하지 않게 하는 기본값).
        // 여기서 찾는 "D" 가 바로 그 dim 셀이므로 명시적으로 켜서 조회한다.
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
            .unwrap_or_else(|| panic!("DN row not found in screen_text:\n{text}"))
            as u64;

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

    // workspace.list 는 mirror(원격 attach client 인지) 를 함께 실어야 한다 — GUI
    // 사이드바만 알던 정보라 에이전트가 조작 전에 판별할 수단이 없었다. 로컬 인스턴스
    // 에는 mirror 워크스페이스가 없으므로 전부 false 다(true 케이스는 실제 attach 가
    // 필요해 두 인스턴스 실측으로 확인한다).
    let ws_rows = tasty
        .call("workspace.list", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!ws_rows.is_empty(), "workspace.list 가 비었다");
    for ws in &ws_rows {
        assert_eq!(
            ws.get("mirror").and_then(|v| v.as_bool()),
            Some(false),
            "workspace.list 행에 mirror:false 가 없다: {ws:?}"
        );
        assert!(
            ws.get("id").and_then(|v| v.as_u64()).is_some(),
            "workspace.list 행에 id 가 없다: {ws:?}"
        );
    }

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
        json!({"level": "pane", "direction": "vertical", "target_pane": pid}),
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

    // ========== tab.close self-protection guard is tab-scoped, not pane-scoped ==========
    // Regression: the guard used to check "does caller belong to the same PANE as the
    // target tab", which wrongly blocked closing a sibling tab. tab.close only affects
    // that tab (and its own SurfaceGroup), so the guard must match that blast radius.
    tasty.call("tab.create", json!({"pane_id": sole_pid}));
    let tab_list = tasty.call("tab.list", json!({"pane_id": sole_pid}));
    let sibling_tab_id = tab_list["tabs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["id"].as_u64().unwrap() != sole_tab_id)
        .unwrap()["id"]
        .as_u64()
        .unwrap();

    // caller (sid) lives in sole_tab_id, not sibling_tab_id → closing the sibling must succeed.
    let result = tasty.call(
        "tab.close",
        json!({"tab_id": sibling_tab_id, "caller_surface_id": sid}),
    );
    assert_eq!(result["closed"], true);

    // Re-create the sibling so sole_tab_id is no longer the last tab, then confirm closing
    // the tab that actually contains the caller is still refused (the guard's real purpose).
    tasty.call("tab.create", json!({"pane_id": sole_pid}));
    let result = tasty.call_raw(
        "tab.close",
        json!({"tab_id": sole_tab_id, "caller_surface_id": sid}),
    );
    assert!(
        result.get("error").is_some(),
        "expected tab.close to refuse closing the caller's own tab, got {result:?}"
    );

    // ========== Renderer color resolution (debug.glyph_color) ==========

    // Inject deterministic VTE: clear screen, home cursor, plain Z, then dim Z.
    // - col 0 row 0: plain Z (intensity = normal)
    // - col 1 row 0: dim Z (intensity = half)
    //
    // Sequence: ESC[2J ESC[H Z ESC[2m Z ESC[0m
    // Hex:      1b5b324a 1b5b48 5a 1b5b326d 5a 1b5b306d
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

    // The plain cell's fg must be the default fg (no SGR fg color was set).
    let pr = plain_fg["r"].as_f64().unwrap();
    let pg = plain_fg["g"].as_f64().unwrap();
    let pb = plain_fg["b"].as_f64().unwrap();
    assert!(
        pr > 0.5 && pg > 0.5 && pb > 0.5,
        "plain fg should be bright"
    );

    // palette::compute_cell_colors blends fg toward bg for Intensity::Half,
    // so the dim cell's fg must differ from a plain cell's fg on the same row.
    assert_ne!(
        plain_fg, dim_fg,
        "dim cell fg should differ from plain cell fg (SGR 2 must dim)",
    );

    // Each channel of the dim fg should sit between the plain fg and the bg
    // (i.e. moved toward the background, not past it or in some other direction).
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
    // completion 은 surface_id 필수 (포커스 독립, 불가침 원칙 1).
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

    // ========== headless PTY (pty.*) — 6+1 메서드 통합 흐름 ==========
    // spawn → list → write → read → wait(exit_code) → kill, 그리고 별도 pty 를
    // attach_surface 로 실제 Tab 으로 승격. Surface 없이 돌던 PTY 가 진짜 exit-code 를
    // 회수하고, 승격 시 실제 surface 로 등장하는지 end-to-end 로 검증(18-a/b/c 를 잇는다).

    // spawn: bare shell(command 없음). pty id 는 disjoint 고범위(>= 0x8000_0000).
    let spawned = tasty.call("pty.spawn", json!({}));
    let pty_id = spawned["pty_id"]
        .as_u64()
        .expect("pty.spawn returns pty_id");
    assert!(
        pty_id >= 0x8000_0000,
        "pty id 는 surface id 와 disjoint 한 고범위여야: {pty_id}"
    );

    // list: 방금 만든 headless PTY 가 필터 없이 전체 목록에 등장.
    let listed = tasty.call("pty.list", json!({}));
    assert!(
        listed["ptys"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["id"].as_u64() == Some(pty_id)),
        "pty.list 가 방금 spawn 한 pty 를 빠뜨림: {listed:?}"
    );

    // write → read: 셸에 echo 를 보내고 화면 텍스트에 반영되는지 폴링.
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

    // write(exit 5) → wait: watcher 가 잡은 진짜 exit_code 를 폴링으로 확인.
    tasty.call("pty.write", json!({ "id": pty_id, "text": "exit 5\n" }));
    let wait_start = std::time::Instant::now();
    let exited = loop {
        let w = tasty.call("pty.wait", json!({ "id": pty_id }));
        if w["exited"].as_bool() == Some(true) {
            break w;
        }
        if wait_start.elapsed() > Duration::from_secs(10) {
            panic!("pty.wait 가 종료를 감지하지 못함: {w:?}");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(exited["exit_code"].as_i64(), Some(5), "진짜 exit-code 회수");
    assert_eq!(exited["success"], false);

    // kill: 이미 종료된 PTY 도 두 store 에서 회수 → list 에서 사라진다.
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

    // attach_surface: 살아있는 별도 headless PTY 를 실제 Tab 으로 승격.
    let promo = tasty.call("pty.spawn", json!({}));
    let promo_id = promo["pty_id"].as_u64().expect("second pty");
    let attached = tasty.call(
        "pty.attach_surface",
        json!({ "id": promo_id, "pane_id": pid }),
    );
    let new_surface = attached["surface_id"]
        .as_u64()
        .expect("attach_surface returns surface_id");
    assert_eq!(attached["pane_id"].as_u64(), Some(pid));
    // 승격된 surface 는 실제 surface.list 에 등장하고, headless 목록에선 사라진다.
    let surfaces_now = tasty.call("surface.list", json!({}));
    assert!(
        surfaces_now
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["id"].as_u64() == Some(new_surface)),
        "승격된 surface 가 surface.list 에 없음: {surfaces_now:?}"
    );
    let listed3 = tasty.call("pty.list", json!({}));
    assert!(
        listed3["ptys"]
            .as_array()
            .unwrap()
            .iter()
            .all(|p| p["id"].as_u64() != Some(promo_id)),
        "승격된 pty 는 headless 목록에서 빠져야 함: {listed3:?}"
    );

    // ========== Multi-window: owner-based routing + list 전체 순회 ==========
    // 두 번째 main window 를 생성하고, focused 가 새 윈도우로 전환되어도
    // 첫 윈도우의 surface 가 IPC 로 접근 가능한지 검증. CLAUDE.md "포커스 독립".
    let ids_before: std::collections::HashSet<u64> = tasty
        .call("surface.list", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|s| s["id"].as_u64())
        .collect();

    let create_resp = tasty.call("window.create", json!({}));
    // window.create 는 더 이상 fire-and-forget(`{"scheduled": true}`)이 아니라 완료
    // 채널로 생성 성공/실패를 왕복시킨다 — 성공은 `{"created": true, "window_id": …}`
    // (ADR-0122). 옛 `scheduled` 계약을 보면 Null 이 잡힌다.
    assert_eq!(
        create_resp["created"], true,
        "window.create 성공 응답이 created=true 를 실어야 한다: {create_resp:?}"
    );
    assert!(
        create_resp["window_id"].as_u64().is_some(),
        "window.create 성공 응답에 window_id 가 있어야 한다: {create_resp:?}"
    );

    // 새 윈도우의 PTY shell 이 surface.list 에 등장할 때까지 polling.
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
            // pty_ready 까지 기다리기.
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

    // surface.list 전체 순회 — 두 surface 모두 보여야.
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

    // workspace.list / pane.list 도 전체 순회.
    let workspaces = tasty
        .call("workspace.list", json!({}))
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(
        workspaces.len() >= 2,
        "workspace.list 가 모든 engine 의 workspace 를 합쳐 반환해야: {workspaces:?}"
    );

    // owner-based routing: focused 가 새 윈도우인 상태에서 첫 윈도우 surface 에 IPC.
    // (focus 는 사용자 단축키 영역이라 IPC 로 전환 안 함 — 자동 focus 가 새 윈도우.)
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

    // 두 번째 윈도우 surface 에도 send 동작.
    tasty.set_mark(new_sid);
    let send_second = tasty.call(
        "surface.send",
        json!({"surface_id": new_sid, "text": "echo W2_owner_route\n"}),
    );
    assert_eq!(send_second["sent"], true);
    let out2 = tasty.wait_for_output(new_sid, "W2_owner_route", Duration::from_secs(5));
    assert!(out2.contains("W2_owner_route"));
}
