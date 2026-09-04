//! e2e 시나리오 — **시나리오 하나에 `#[test]` 하나**.
//!
//! 인스턴스는 여전히 test binary 당 1 개다(`common::shared()`, ADR-0090). 격리
//! 단위는 프로세스가 아니라 workspace 이므로 각 시나리오는 [`scenario`] 로 자기
//! workspace 를 잡고 그 안의 surface/pane 만 건드린다 — 그래서 전역 목록
//! (`pane.list` / `workspace.list` / `hook.list` / `pty.list` / notification) 위에서는
//! 길이 산술 대신 "내 것이 있는가/없는가" 로 판정한다(`tests/common/mod.rs` 의
//! `shared()` doc 경고).
//!
//! **왜 한 함수가 아닌가.** 전체를 한 `#[test]` 에 직렬로 두면 앞에서 하나가 죽는
//! 순간 뒤의 전부가 실행되지 않고, CI 는 그 하나를 위해 파일 전체를 `--skip` 하게
//! 된다 — 그러면 GUI 를 요구하지 않는 시나리오까지 같이 사라진다. 실제로 헤드리스
//! 조합에서 이 파일은 통째로 skip 되어 있었고, 벽은 마지막 시나리오의
//! `window.create` 하나뿐이었다. 지금은 창을 요구하는 단언이
//! [`multi_window_owner_routing`] 한 곳에 모여 있어 그것만 skip 하면 된다.

mod common;

use common::{TastyInstance, TestWorkspace};
use serde_json::json;
use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::time::Duration;

/// 창을 만드는 시나리오와 나머지를 갈라 놓는 차선(lane) 잠금. 나머지는 read 로
/// 서로 병렬이고, 창을 만드는 하나만 write 로 단독이다.
///
/// **왜 필요한가 — 실측(기본 gui 조합, Xvfb).** `multi_window_owner_routing` 이 두
/// 번째 창을 만들면 그 창이 포커스를 가져가는데, **owner 를 params 에서 못 푸는
/// 메서드는 포커스된 창으로 라우팅된다**: `src/app/request_owner.rs` 의 `Kind` 는
/// surface / workspace / pane 셋뿐이라 `tab.close {tab_id}` 와 `pty.*` 의 headless
/// pty id 는 owner 를 못 찾고 `focused_view_id` 로 떨어진다. 그래서 첫 창의 tab/pty
/// 를 겨눈 호출이 두 번째 창의 engine 으로 가 "not found" 가 됐다(3 건 실패).
/// 창이 하나뿐인 헤드리스 조합에서는 이 형태가 나타나지 않는다.
///
/// 그건 제품 축의 사실이고 이 파일이 고칠 것이 아니다 — 테스트는 창을 만드는
/// 시나리오를 나머지와 **겹치지 않게** 돌려서 그 축을 건드리지 않는다.
static WINDOW_EXCLUSIVE: RwLock<()> = RwLock::new(());

/// 창을 만들지 않는 시나리오의 차선. poison 은 무시한다 — 다른 테스트가 panic 한
/// 사실이 이 테스트의 판정을 바꾸지 않고, 바꾸면 실패 원인이 뒤바뀐다.
fn lane() -> RwLockReadGuard<'static, ()> {
    WINDOW_EXCLUSIVE.read().unwrap_or_else(|e| e.into_inner())
}

/// 창을 만드는 시나리오의 차선 — 나머지가 다 끝난 뒤 단독으로 돈다.
fn exclusive_lane() -> RwLockWriteGuard<'static, ()> {
    WINDOW_EXCLUSIVE.write().unwrap_or_else(|e| e.into_inner())
}

/// 시나리오 하나의 격리 단위를 잡는다 — 차선 + 공유 인스턴스 + 전용 workspace +
/// 그 workspace 의 surface/pane. 반환 순서는 `(instance, workspace, surface, pane, lane)`.
/// 마지막 값은 테스트가 끝날 때까지 살아 있어야 하므로 `_lane` 으로 받아 둔다.
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

// ========== Read-only queries ==========

#[test]
fn read_only_queries() {
    let (tasty, _ws, sid, pid, _lane) = scenario("e2e-read-only");

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

    // surface.list / pane.list — 전역 목록이므로 "내 것이 보이는가" 로 본다.
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

    // screen_text
    let text = tasty.screen_text_of(sid);
    assert!(!text.trim().is_empty());

    // cursor_position
    let cursor = tasty.call("surface.cursor_position", json!({"surface_id": sid}));
    assert!(cursor.get("x").is_some());
    assert!(cursor.get("y").is_some());

    // tab.list — pane 스코프라 그대로 안전하다.
    let tabs = tasty.call("tab.list", json!({"pane_id": pid}));
    assert!(!tabs["tabs"].as_array().unwrap().is_empty());
}

#[test]
fn workspace_list_rows_carry_mirror_and_id() {
    // workspace.list 는 mirror(원격 attach client 인지) 를 함께 실어야 한다 — GUI
    // 사이드바만 알던 정보라 에이전트가 조작 전에 판별할 수단이 없었다. 로컬 인스턴스
    // 에는 mirror 워크스페이스가 없으므로 전부 false 다(true 케이스는 실제 attach 가
    // 필요해 두 인스턴스 실측으로 확인한다).
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
}

#[test]
fn notification_create_then_list() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-notification");

    let created = tasty.call(
        "notification.create",
        json!({"title": "Test", "body": "Hello", "surface_id": sid}),
    );
    // 전역 목록이라 길이가 아니라 "내가 만든 것이 보이는가" 로 본다.
    let notifs = tasty.call("notification.list", json!({}));
    let rows = notifs.as_array().cloned().unwrap_or_default();
    assert!(!rows.is_empty(), "notification.list 가 비었다: {created:?}");
    assert!(
        rows.iter().any(|n| n["surface_id"].as_u64() == Some(sid)),
        "notification.list 가 내 surface={sid} 의 알림을 빠뜨림: {rows:?}"
    );
}

// ========== Terminal I/O ==========

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
}

#[test]
fn terminal_send_key_and_send_to() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-terminal-keys");

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
}

#[test]
fn terminal_send_combo_and_abort() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-terminal-combo");

    let result = tasty.call(
        "surface.send_combo",
        json!({"surface_id": sid, "key": "x", "modifiers": ["alt"]}),
    );
    assert_eq!(result["sent"], true);
    // zsh ZLE 의 경우 Alt+X 는 execute-named-cmd 위젯을 호출하여 prompt 가
    // "execute: " 로 바뀐다. 후속 단언이 일반 명령으로 동작하도록 Ctrl+G 로
    // mode 를 abort 시킨다. (사용자 dotfile 이 ^G 를 rebind 할 수 있으나,
    // 본 테스트는 HOME/ZDOTDIR 을 격리해 stock 셸 동작을 보장한다.)
    tasty.call(
        "surface.send_combo",
        json!({"surface_id": sid, "key": "g", "modifiers": ["ctrl"]}),
    );
    // sentinel echo 로 abort 가 실제로 풀려 prompt 가 명령을 받을 수 있는
    // 상태인지 deterministic 하게 확인.
    tasty.set_mark(sid);
    tasty.send_text(sid, "echo __abort_ok__\n");
    tasty.wait_for_output(sid, "__abort_ok__", Duration::from_secs(3));
}

// ========== surface.completion (highlight producer) ==========

#[test]
fn surface_completion_reaches_the_pipeline() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-completion");

    // completion IPC 가 CLI→핸들러→intent→cascade 전 경로로 라우팅되어 success 를
    // 돌려주는지(=method_not_found 아님) 확인. highlight 발동 자체는 host 렌더라
    // 헤드리스로 관측 불가 — 여기선 파이프라인 도달만 검증한다.
    let completion = tasty.call("surface.completion", json!({ "surface_id": sid }));
    assert_eq!(completion["ok"], true);
    assert_eq!(completion["surface_id"].as_u64().unwrap(), sid);
}

// ========== surface.attention.{get,clear} (해제 표면 왕복) ==========

#[test]
fn surface_attention_raise_and_clear() {
    // raise 는 되지만 해제가 없던 비대칭의 회귀 가드. 해제 producer 두 개(실 렌더
    // 포커스·알림 읽음)는 전부 GUI 로컬 사건이라 IPC 로 관측/구동할 수 없어, 이
    // 왕복이 해제 축을 프로토콜 레벨에서 실행하는 유일한 경로다.
    //
    // 대상은 포커스 surface 가 아니라 **IPC 로 새로 만든 워크스페이스의 surface** 다
    // — `gpu.rs` 가 매 렌더 프레임 실-포커스 surface 의 attention 을 지우므로 포커스
    // surface 위에서는 raise 가 프레임 하나를 못 넘긴다. IPC 로 만든 워크스페이스는
    // active 를 전환하지 않아(원칙 1·3) 그 surface 는 렌더 포커스를 얻지 않는다.
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
}

// ========== Dim (SGR 2) renderer regression ==========

// printf is a posix builtin; shell on Windows is cmd.exe by default which does not
// interpret \033 escapes the same way, so we restrict to Unix.
#[cfg(not(windows))]
#[test]
fn dim_sgr2_survives_to_the_renderer() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-dim-sgr2");

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

// ========== Hooks ==========

#[test]
fn hook_set_list_unset() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-hooks");

    let hook_result = tasty.call(
        "hook.set",
        json!({"surface_id": sid, "event": "bell", "command": "echo hooked"}),
    );
    let hook_id = hook_result["hook_id"].as_u64().unwrap();
    assert!(hook_id > 0);

    // hook.list 는 전역이라 길이 산술 대신 멤버십으로 본다.
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

// ========== Structural mutations ==========

#[test]
fn structural_mutations_within_one_workspace() {
    let (tasty, ws, _sid, pid, _lane) = scenario("e2e-structural");

    // 이 workspace 안의 pane 수만 센다 — 전역 `pane.list` 길이는 다른 시나리오가
    // 동시에 pane 을 만들고 닫으므로 산술이 성립하지 않는다.
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

    // split pane
    let panes_before = panes_in_ws();
    let split_result = tasty.call(
        "split",
        json!({"level": "pane", "direction": "vertical", "target_pane": pid}),
    );
    let new_pane_id = split_result["new_pane_id"].as_u64().unwrap();
    assert_eq!(panes_in_ws(), panes_before + 1);

    // create tab — tab.list 는 pane 스코프라 그대로 안전하다.
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
    assert_eq!(panes_in_ws(), panes_before);

    // close last pane → should refuse (sole-pane 판정은 workspace 레이아웃 단위다)
    let result = tasty.call("pane.close", json!({"pane_id": pid}));
    assert_eq!(result["closed"], false);

    // close last tab → should refuse
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
    // 전역 길이 델타(`before + 1`)는 병렬 시나리오가 동시에 workspace 를 만들면
    // 깨진다 — 만든 workspace 가 목록에 있는지로 본다.
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

// ========== tab.close self-protection guard is tab-scoped, not pane-scoped ==========

#[test]
fn tab_close_guard_is_tab_scoped_not_pane_scoped() {
    // Regression: the guard used to check "does caller belong to the same PANE as the
    // target tab", which wrongly blocked closing a sibling tab. tab.close only affects
    // that tab (and its own SurfaceGroup), so the guard must match that blast radius.
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

    // caller (sid) lives in own_tab_id, not sibling_tab_id → closing the sibling must succeed.
    let result = tasty.call(
        "tab.close",
        json!({"tab_id": sibling_tab_id, "caller_surface_id": sid}),
    );
    assert_eq!(result["closed"], true);

    // Re-create the sibling so own_tab_id is no longer the last tab, then confirm closing
    // the tab that actually contains the caller is still refused (the guard's real purpose).
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

// ========== Renderer color resolution (debug.glyph_color) ==========

#[test]
fn renderer_resolves_dim_and_plain_glyph_colors() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-glyph-color");

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
}

// ========== Error paths ==========

#[test]
fn error_paths_reject_malformed_calls() {
    let (tasty, _ws, sid, _pid, _lane) = scenario("e2e-error-paths");

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
}

// ========== headless PTY (pty.*) — 6+1 메서드 통합 흐름 ==========

#[test]
fn headless_pty_spawn_write_wait_kill() {
    // spawn → list → write → read → wait(exit_code) → kill. Surface 없이 돌던 PTY 가
    // 진짜 exit-code 를 회수하는지 end-to-end 로 검증한다.
    let _lane = lane();
    let tasty = common::shared();

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
}

#[test]
fn headless_pty_attach_surface_promotes_to_a_tab() {
    // 살아있는 headless PTY 를 실제 Tab 으로 승격 — 승격 시 실제 surface 로 등장하고
    // headless 목록에서는 빠지는지 확인한다.
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

// ========== Multi-window: owner-based routing + list 전체 순회 ==========

/// **이 파일에서 유일하게 창을 요구하는 시나리오다.** `window.create` 는 gui 라우터의
/// `app_methods` step 에만 있어 헤드리스 데몬에서는 `-32601` 이 난다 — 배선 결함이
/// 아니라 창이 없다는 사실 그 자체이므로, 헤드리스 조합 CI 는 **이 이름 하나만**
/// `--skip` 한다(`.github/workflows/crossplatform-check.yml`,
/// `tests/headless_skip_names_are_exact.rs` 가 그 이름의 정확성을 강제한다).
#[test]
fn multi_window_owner_routing() {
    // 두 번째 main window 를 생성하고, focused 가 새 윈도우로 전환되어도
    // 첫 윈도우의 surface 가 IPC 로 접근 가능한지 검증. CLAUDE.md "포커스 독립".
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

    // workspace.list 도 전체 순회.
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

// ========== plugin 읽기 표면 — 창 없이 답하는가 ==========

/// `plugin.list` 가 창 없이 답한다.
///
/// 이전에는 헤드리스에서 `-32601`(그런 메서드 없다)이었다. 그것은 `plugin.*`
/// 관리 표면이 통째로 없다는 뜻이었고, CLI 전용 실행 형태인 헤드리스에서
/// `docs/identity.md` 원칙 2(에이전트 기능은 IPC + CLI 양면)에 정면으로 걸렸다.
///
/// **왜 통합 테스트여야 하는가.** 이 경로의 단위 테스트
/// (`src/adapters/ipc/handler/plugin.rs`)는 매니저가 **있을 때** 에러가 아니라는 것을
/// 못 잰다 — `PluginManager` 는 waker 와 registry port 를 요구해 단위 테스트가 만들 수
/// 없고, `tasty-host-plugin` 의 테스트용 생성자는 그 크레이트의 `#[cfg(test)]` 라 여기서
/// 보이지 않는다. 그래서 단위 테스트만 두면 "모든 응답이 에러" 여도 통과한다. 라우팅
/// 표(`READONLY_METHODS`)와 dispatch arm 이 실제로 이어져 있다는 것도 여기서만 잰다.
///
/// 헤드리스 조합에서는 테스트가 띄우는 바이너리 자체가 헤드리스라, 이 단언은 그
/// 조합에서 **창이 없는 데몬**을 상대로 돈다. gui 조합에서도 성립해야 한다 — 두
/// 라우터가 같은 함수를 쓰기 때문이다.
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

    // 개수는 홈 상태에 달렸으므로 세지 않는다. 배열이라는 것과, 있다면 각 항목이
    // 매니페스트에서 나온 모양이라는 것만 본다.
    for p in plugins {
        assert!(
            p.get("id").and_then(|v| v.as_str()).is_some(),
            "plugin 항목에 id 가 없다: {p}"
        );
    }
}

/// `plugin.show` 는 매니저가 있어도 **없는 plugin 이름**에는 다른 에러를 준다.
///
/// 이것이 위 테스트의 대조다 — 위만 있으면 "모든 plugin.* 가 무조건 성공" 이어도
/// 통과한다. 여기서 요구하는 것은 성공/실패가 **입력에 따라 갈린다**는 것이다.
/// `-32000`(매니저 없음)이 아니라 `-32003`(그 plugin 이 설치돼 있지 않음)이어야
/// 매니저가 실제로 세워져 조회가 수행됐다는 뜻이 된다.
///
/// **두 코드의 차이가 이 축의 계측기다.** `-32000` 은 "물어볼 대상 자체가 없다",
/// `-32003` 은 "물어봤고 그런 것이 없더라" 다. 앞의 것으로 느슨하게 고치면 이
/// 테스트는 매니저가 한 번도 안 세워져도 통과한다 — 즉 검증하려던 것을 정확히
/// 놓친다. 코드를 바꿔야 한다면 무엇이 계측되는지 먼저 다시 세워라.
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

/// 수명주기 메서드는 **여전히 없다** — 이 축이 연 것은 읽기 표면뿐이다.
///
/// **이 테스트는 위 둘이 빨개질 때 초록으로 남아야 한다.** 배선
/// (`src/boot/headless_dispatch.rs` 의 읽기 전용 가로채기)을 죽이면 위 두 테스트는
/// `-32601` 로 실패하는데, 이것은 그대로 통과한다. 셋이 함께 빨개지면 그 변이는
/// "무언가 깨졌다" 만 말하고, 배선이 **정확히 그 둘을 만든다**는 것은 말하지 못한다.
/// 이 비대칭이 이 세 테스트의 판정력이므로, 셋을 한 조건으로 묶도록 고치지 마라.
///
/// 이 단언이 없으면 위 둘은 "`plugin.*` 를 전부 열었다" 와 구별되지 않는다.
/// 헤드리스에서 `plugin.enable` 이 답하려면 `App.plugin_manager` 만으로는 부족하고
/// gui feature 로 게이트된 `app/plugin_glue` 와 `cascade_plugin_events` 가 필요하다.
/// 그 경계를 여는 것은 별도 결정이므로, 지금은 없는 것이 현재 상태다.
#[cfg(not(feature = "gui"))]
#[test]
fn lifecycle_methods_are_still_absent_in_a_headless_daemon() {
    let _lane = lane();
    let tasty = common::shared();
    let resp = tasty.call_raw("plugin.enable", json!({"id": "anything"}));

    let code = resp
        .get("error")
        .and_then(|e| e.get("code"))
        .and_then(|c| c.as_i64());
    assert_eq!(
        code,
        Some(-32601),
        "헤드리스에서 plugin.enable 은 아직 없는 메서드여야 한다: {resp}"
    );
}

/// 대상을 **지목했는데 아무 창도 안 가진** 요청은 거절된다 — 포커스된 창으로 안 샌다.
///
/// 지우기 전의 폴백은 이 요청을 포커스된 창에 넘겼고, 그래서 **존재하지 않는
/// `workspace_id` 를 실은 `workspace.create` 가 조용히 성공했다**(실측 2026-09-05:
/// 포커스된 창에 워크스페이스를 만들고 그 id 를 돌려줬다). 호출자는 자기가 지목한
/// 곳에 만들어진 줄 안다. `docs/design/policies/focus.md` 의 "silent fallback 금지".
///
/// **두 조합 모두**에서 돈다. 헤드리스는 engine 이 하나라 라우팅할 곳이 없지만
/// 판정은 같아야 한다 — 한쪽만 거절하면 같은 요청이 조합에 따라 다르게 끝난다.
#[test]
fn a_request_naming_an_unowned_target_is_rejected() {
    let _lane = lane();
    let tasty = common::shared();

    // 지목했고 아무도 안 가졌다 → 에러. 핸들러가 그 키를 무시하더라도 그렇다.
    let resp = tasty.call_raw(
        "workspace.create",
        json!({ "workspace_id": 999_999, "name": "unowned-target-probe" }),
    );
    assert!(
        resp.get("error").is_some(),
        "지목한 대상을 아무도 안 가졌으면 거절해야 한다(성공하면 포커스된 창에 \
         만들어진 것이다): {resp}"
    );
    let msg = resp["error"]["message"].as_str().unwrap_or_default();
    assert!(
        msg.contains("999999") && msg.contains("workspace"),
        "에러가 무엇을 못 찾았는지 말해야 고칠 수 있다: {resp}"
    );

    // plugin → host call 경로도 같은 판정을 받는다. 라우팅 사본이 둘이라
    // (IPC 라우터 / intent 디스패처) 한쪽만 고치면 다른 쪽이 조용히 옛 동작을 남긴다.
    // `image.*` 는 image plugin 의 namespace 이고 그 plugin 이 `image.open` 을 호스트로
    // 되던지므로, 이 호출이 그 사본을 탄다 — 응답의 `host call ... failed` 감싸기가
    // 경로를 드러낸다.
    //
    // 이 단언만 gui 인 이유는 이 축과 무관하다: 헤드리스에는 `image.open` 의 호스트
    // arm 자체가 없어서(`#[cfg(feature = "gui")]`) 되던진 호출이 소유 검사에 닿기 전에
    // "Method not found" 로 끝난다. 그건 headless 가 app 층 메서드를 떨어뜨리는
    // 별개 축이고, 그쪽이 닫히면 이 게이트도 없어진다.
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

    // plugin 이 점유한 namespace 의 메서드는 id 를 실었어도 **안 잘린다.**
    // 그 prefix 는 호스트 예약이 아니라서(번들 plugin 이 갖고 있다) plugin 이 답할 수
    // 있고, 헤드리스는 소유 검사가 engine handler 앞이라 여기서 자르면 forward 될
    // 호출을 불러 보기도 전에 죽인다. 이 단언이 그 경계를 지킨다.
    let forwarded = tasty.call_raw("markdown.recent", json!({ "surface_id": 999_999 }));
    assert!(
        forwarded.get("result").is_some(),
        "plugin namespace 의 메서드는 id 를 실어도 forward 돼야 한다: {forwarded}"
    );

    // 지목 안 한 같은 메서드는 그대로 동작한다 — 폴백을 통째로 없앤 것이 아니다.
    let ok = tasty.call_raw("workspace.create", json!({ "name": "no-target-probe" }));
    assert!(
        ok.get("result").is_some(),
        "대상을 지목하지 않은 생성 요청은 계속 동작해야 한다: {ok}"
    );
}
