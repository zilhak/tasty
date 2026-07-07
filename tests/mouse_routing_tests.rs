//! Mouse-routing injection regression net for `handle_mouse_input`.
//!
//! `handle_mouse_input` 의 좌표/라우팅 결정 수학은 이미 순수 함수(단위테스트)로
//! 격리돼 있다. 이 net 이 잡는 것은 순수테스트가 못 잡는 부분 — "블록→메서드
//! 추출이 분기 순서·가드·early-return 을 보존했는가" 하는 stateful 라우팅이다.
//!
//! 실제 데스크톱 마우스를 뺏지 않고 IPC(`debug.inject_window_mouse`)로 winit
//! 레벨 포인터 이벤트를 주입해 실제 `handle_mouse_input` 을 헤드리스 구동하고,
//! read-only debug IPC(`debug.selection`/`debug.pending_menu`/`debug.focused_surface`)
//! 로 라우팅 결과를 단언한다 (원칙 1·3: 사용자 입력 재현은 debug 격리).
//!
//! 대상은 focused 테스트 윈도우의 **active workspace** surface 다 — 주입 좌표가
//! 보이는 레이아웃에 닿아야 `surface_rect_by_id` 가 해소되기 때문. IPC 로 만든
//! workspace 는 active 전환이 없으므로(포커스 독립) 여기서는 쓰지 않는다.
//!
//! Run with: cargo test --test mouse_routing_tests -- --ignored --test-threads=1
//! (display 필요, single-thread — 한 윈도우만 OS 포커스를 가질 수 있으므로.)

mod gui_common;

use gui_common::shared;
use serde_json::json;
use std::time::Duration;

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
