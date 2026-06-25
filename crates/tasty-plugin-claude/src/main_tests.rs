//! `main_tests` 단위 테스트.

use super::*;
use state::{ChildEntry, ClaudeState};

fn entry(child_surface_id: u32, index: u32) -> ChildEntry {
    ChildEntry {
        child_surface_id,
        index,
        cwd: None,
        role: None,
        nickname: None,
    }
}

#[test]
fn set_idle_state_true_sets_idle() {
    let mut state = ClaudeState::default();
    let res = handle_set_idle_state(&mut state, &json!({ "surface_id": 5, "idle": true })).unwrap();
    assert_eq!(res, json!({ "ok": true }));
    assert_eq!(state.state_of(5), "idle");
}

#[test]
fn set_idle_state_false_clears_idle_and_needs_input() {
    let mut state = ClaudeState::default();
    state.set_idle(5, true);
    state.set_needs_input(5, true);
    handle_set_idle_state(&mut state, &json!({ "surface_id": 5, "idle": false })).unwrap();
    assert_eq!(state.state_of(5), "active");
}

#[test]
fn set_idle_state_missing_surface_id_returns_error() {
    let mut state = ClaudeState::default();
    let err = handle_set_idle_state(&mut state, &json!({ "idle": true })).unwrap_err();
    // 호스트는 No focused surface (-32000) 반환 — 호환 보존.
    assert_eq!(err.code, -32000);
}

#[test]
fn set_idle_state_missing_idle_param_returns_invalid_params() {
    let mut state = ClaudeState::default();
    let err = handle_set_idle_state(&mut state, &json!({ "surface_id": 5 })).unwrap_err();
    assert_eq!(err.code, -32602);
}

#[test]
fn set_needs_input_true() {
    let mut state = ClaudeState::default();
    let res = handle_set_needs_input(&mut state, &json!({ "surface_id": 7, "needs_input": true }))
        .unwrap();
    assert_eq!(res, json!({ "ok": true }));
    assert_eq!(state.state_of(7), "needs_input");
}

#[test]
fn parent_returns_active_when_known() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 1));
    let res = handle_parent(&state, &json!({ "surface_id": 100 })).unwrap();
    assert_eq!(res, json!({ "parent_surface_id": 10, "status": "active" }));
}

#[test]
fn parent_returns_closed_when_marked() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 1));
    state.mark_parent_closed(10);
    let res = handle_parent(&state, &json!({ "surface_id": 100 })).unwrap();
    assert_eq!(res, json!({ "parent_surface_id": 10, "status": "closed" }));
}

#[test]
fn parent_returns_none_when_not_registered() {
    let state = ClaudeState::default();
    let res = handle_parent(&state, &json!({ "surface_id": 999 })).unwrap();
    assert_eq!(res, json!({ "parent_surface_id": null, "status": "none" }));
}

#[test]
fn parent_missing_surface_id_is_invalid_params() {
    let state = ClaudeState::default();
    let err = handle_parent(&state, &json!({})).unwrap_err();
    assert_eq!(err.code, -32602);
}

// ─── step 04b: children/wait/kill helper tests ──────────────────────────

#[test]
fn children_base_entries_empty_when_no_children() {
    let state = ClaudeState::default();
    assert!(children_base_entries(&state, 10).is_empty());
}

#[test]
fn children_base_entries_includes_state_and_metadata() {
    let mut state = ClaudeState::default();
    state.register_child(
        10,
        ChildEntry {
            child_surface_id: 100,
            index: 1,
            cwd: Some("/tmp".into()),
            role: Some("worker".into()),
            nickname: Some("alpha".into()),
        },
    );
    state.set_needs_input(100, true);
    let entries = children_base_entries(&state, 10);
    assert_eq!(entries.len(), 1);
    let e = &entries[0];
    assert_eq!(e["child_surface_id"], 100);
    assert_eq!(e["index"], 1);
    assert_eq!(e["cwd"], "/tmp");
    assert_eq!(e["role"], "worker");
    assert_eq!(e["nickname"], "alpha");
    assert_eq!(e["state"], "needs_input");
    // foreground_process/foreground_pid는 IPC enrichment 단계 — 본 layer에서는
    // 키 자체가 존재하지 않아야 한다.
    assert!(e.get("foreground_process").is_none());
    assert!(e.get("foreground_pid").is_none());
}

#[test]
fn wait_decide_returns_exited_when_child_unknown() {
    let state = ClaudeState::default();
    assert_eq!(wait_decide(&state, 10, 1), WaitDecision::Exited);
}

#[test]
fn wait_decide_returns_check_existence_when_child_in_state() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 2));
    assert_eq!(
        wait_decide(&state, 10, 2),
        WaitDecision::CheckExistence(100)
    );
}

// ─── wait-fix Gate4: find_child None 의 (a)/(b)/(c) 순수 분류 ───────────────

/// (a) child_surface_id 를 --child 자리에 입력 → InvalidChildSurfaceId 이고
/// correct_index 가 그 child 의 올바른 index 로 채워진다.
#[test]
fn wait_decide_detects_child_surface_id_misuse_with_correct_index() {
    let mut state = ClaudeState::default();
    let i = state.next_child_index(10); // 1
    state.register_child(10, entry(170, i)); // surface_id 170, index 1
    assert_eq!(
        wait_decide(&state, 10, 170),
        WaitDecision::InvalidChildSurfaceId {
            owner: 10,
            correct_index: Some(1),
        }
    );
}

/// (a) surface_id 가 호출 parent 가 아닌 다른 parent 의 child 인 경우 → owner
/// 필드가 그 다른 parent 로 세팅된다 (handle_wait 메시지 분기 입력 검증).
#[test]
fn wait_decide_surface_id_owner_differs_from_calling_parent() {
    let mut state = ClaudeState::default();
    let i = state.next_child_index(20); // 1
    state.register_child(20, entry(200, i)); // parent 20 의 child
    // 호출 parent 는 10 인데 200 은 parent 20 소유.
    assert_eq!(
        wait_decide(&state, 10, 200),
        WaitDecision::InvalidChildSurfaceId {
            owner: 20,
            correct_index: Some(1),
        }
    );
}

/// (b) child_index > high-water → NeverIssued{highest} 에 올바른 high-water.
#[test]
fn wait_decide_never_issued_when_above_high_water() {
    let mut state = ClaudeState::default();
    let i = state.next_child_index(10); // 1 → high_water=1
    state.register_child(10, entry(100, i));
    assert_eq!(state.high_water(10), Some(1));
    assert_eq!(
        wait_decide(&state, 10, 5),
        WaitDecision::NeverIssued { highest: 1 }
    );
}

/// (c) unregister 로 정리된 index (N <= high-water, find_child None) → Exited.
#[test]
fn wait_decide_exited_for_cleaned_up_index() {
    let mut state = ClaudeState::default();
    let i1 = state.next_child_index(10); // 1
    state.register_child(10, entry(100, i1));
    let i2 = state.next_child_index(10); // 2
    state.register_child(10, entry(101, i2));
    state.unregister_child(101); // index 2 제거; parent 10 에 100 남아 high_water 보존.
    assert!(state.find_child(10, 2).is_none());
    assert_eq!(state.high_water(10), Some(2));
    assert_eq!(wait_decide(&state, 10, 2), WaitDecision::Exited);
}

/// (c) high-water None (한 번도 spawn 안 한 parent / 자식 0명) → Exited.
#[test]
fn wait_decide_exited_when_no_high_water() {
    let state = ClaudeState::default();
    assert_eq!(state.high_water(99), None);
    assert_eq!(wait_decide(&state, 99, 1), WaitDecision::Exited);
}

/// 경계값 N == high-water (정리된 최댓값) → Exited (off-by-one 가드: (b) 미해당).
#[test]
fn wait_decide_boundary_index_equals_high_water_is_exited() {
    let mut state = ClaudeState::default();
    let i1 = state.next_child_index(10); // 1
    state.register_child(10, entry(100, i1));
    let i2 = state.next_child_index(10); // 2
    state.register_child(10, entry(101, i2));
    let i3 = state.next_child_index(10); // 3 → high_water=3
    state.register_child(10, entry(102, i3));
    state.unregister_child(102); // 최댓값 index 3 제거; 100/101 남아 high_water=3 보존.
    assert_eq!(state.high_water(10), Some(3));
    assert!(state.find_child(10, 3).is_none());
    // N(3) == high_water(3) 이므로 NeverIssued(N > hw)에 해당하지 않고 (c) Exited.
    assert_eq!(wait_decide(&state, 10, 3), WaitDecision::Exited);
}

/// (회귀 가드) 정당한 현존 index → CheckExistence (오류 분기 미진입).
#[test]
fn wait_decide_valid_existing_index_returns_check_existence() {
    let mut state = ClaudeState::default();
    let i = state.next_child_index(10); // 1
    state.register_child(10, entry(100, i));
    assert_eq!(
        wait_decide(&state, 10, 1),
        WaitDecision::CheckExistence(100)
    );
}

// ─── G.F.b: wait_any_decide 순수 함수 ─────────────────────────────────

/// 모든 children 이 state 에 등록돼 있을 때 각각의 CheckExistence 가
/// 입력 순서 그대로 수집된다.
#[test]
fn wait_any_decide_collects_all_candidates_when_present() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 1));
    state.register_child(10, entry(101, 2));
    state.register_child(10, entry(102, 3));

    let result = wait_any_decide(&state, 10, &[1, 2, 3]);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], (1, WaitDecision::CheckExistence(100)));
    assert_eq!(result[1], (2, WaitDecision::CheckExistence(101)));
    assert_eq!(result[2], (3, WaitDecision::CheckExistence(102)));
}

/// 일부 child 가 state 에 없을 때 그 자리에 Exited 가 들어가고 나머지
/// 순서는 보존된다.
#[test]
fn wait_any_decide_marks_missing_children_as_exited() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 1));
    state.register_child(10, entry(102, 3));
    // child_index 2 는 state 에 없음.

    let result = wait_any_decide(&state, 10, &[1, 2, 3]);
    assert_eq!(result.len(), 3);
    assert_eq!(result[0], (1, WaitDecision::CheckExistence(100)));
    assert_eq!(result[1], (2, WaitDecision::Exited));
    assert_eq!(result[2], (3, WaitDecision::CheckExistence(102)));
}

/// R9 회피: children=[CheckExistence, Exited, CheckExistence] 순서일 때
/// 결과 Vec 의 첫 원소는 Exited 가 아니라 첫 child 의 CheckExistence.
/// caller 가 a 부터 평가하므로 a 가 terminal 이면 b 의 Exited 보다 우선.
#[test]
fn wait_any_decide_preserves_input_order_when_mixed_state() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 1));
    // child 2 는 state 에 없음 → Exited.
    state.register_child(10, entry(102, 3));

    let result = wait_any_decide(&state, 10, &[1, 2, 3]);
    assert_eq!(result[0].0, 1);
    assert!(matches!(result[0].1, WaitDecision::CheckExistence(_)));
    assert_eq!(result[1].0, 2);
    assert!(matches!(result[1].1, WaitDecision::Exited));
}

/// empty children slice 는 빈 Vec 반환 — caller 가 None-result 로 pending 결정.
#[test]
fn wait_any_decide_returns_empty_for_empty_input() {
    let state = ClaudeState::default();
    let result = wait_any_decide(&state, 10, &[]);
    assert!(result.is_empty());
}

// ─── G.F.c: handle_wait_any IPC 핸들러 ────────────────────────────────

/// 전원 active 일 때 응답은 `{"state":"pending"}` — child_index 키 없음 (R10).
/// host call 은 모든 surface 가 살아있음(exists=true), state_of 는 항상 "active".
#[test]
fn wait_any_response_returns_pending_when_all_active() {
    let decisions = vec![
        (1u32, WaitDecision::CheckExistence(100)),
        (2u32, WaitDecision::CheckExistence(101)),
        (3u32, WaitDecision::CheckExistence(102)),
    ];
    let resp = wait_any_response(&decisions, |_| true, |_| "active");
    assert_eq!(resp["state"], "pending");
    assert!(resp.get("child_index").is_none());
}

/// R9 회피 (응답 단계): decisions=[CheckExistence(100), Exited] 이고
/// surface 100 이 idle 일 때 응답은 `{state:"idle", child_index:1}`.
/// 뒤의 Exited (child 2) 가 앞의 idle (child 1) 을 가로채지 않음.
#[test]
fn wait_any_response_returns_first_idle_even_when_later_child_exited() {
    let decisions = vec![
        (1u32, WaitDecision::CheckExistence(100)),
        (2u32, WaitDecision::Exited),
    ];
    let resp = wait_any_response(
        &decisions,
        |sid| {
            assert_eq!(sid, 100, "should only check surface 100 before terminal");
            true
        },
        |sid| {
            assert_eq!(sid, 100);
            "idle"
        },
    );
    assert_eq!(resp["state"], "idle");
    assert_eq!(resp["child_index"], 1);
}

/// 회귀 가드: NeverIssued variant 도 wait_any 에서 해당 child_index 의 terminal
/// "exited" 로 매핑된다 (기존 wait-any 테스트는 high_water 미설정이라 Exited 로만
/// 빠져 이 arm 을 안 태운다). host/state 주입 closure 는 호출되면 안 됨.
#[test]
fn wait_any_response_treats_never_issued_as_exited() {
    let decisions = vec![(7u32, WaitDecision::NeverIssued { highest: 3 })];
    let resp = wait_any_response(
        &decisions,
        |_| panic!("exists_fn must not be called for NeverIssued"),
        |_| panic!("state_of_fn must not be called for NeverIssued"),
    );
    assert_eq!(resp["state"], "exited");
    assert_eq!(resp["child_index"], 7);
}

/// 회귀 가드: InvalidChildSurfaceId variant 도 wait_any 에서 해당 child_index 의
/// terminal "exited" 로 매핑된다. host/state 주입 closure 는 호출되면 안 됨.
#[test]
fn wait_any_response_treats_invalid_child_surface_id_as_exited() {
    let decisions = vec![(
        9u32,
        WaitDecision::InvalidChildSurfaceId {
            owner: 20,
            correct_index: Some(2),
        },
    )];
    let resp = wait_any_response(
        &decisions,
        |_| panic!("exists_fn must not be called for InvalidChildSurfaceId"),
        |_| panic!("state_of_fn must not be called for InvalidChildSurfaceId"),
    );
    assert_eq!(resp["state"], "exited");
    assert_eq!(resp["child_index"], 9);
}

/// empty `--children` 입력은 invalid_params (G.F-Q3). params parsing 단계에서
/// 잡히므로 host 가 없어도 검증 가능.
#[test]
fn parse_wait_any_params_errors_on_empty_children() {
    let err = parse_wait_any_params(&json!({
        "surface_id": 10,
        "children": "",
    }))
    .unwrap_err();
    assert_eq!(err.code, -32602, "expected invalid_params, got {err:?}");
}

#[test]
fn kill_finalize_removes_child_and_persists_only_when_needed() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 1));
    state.register_child(10, entry(101, 2));
    assert_eq!(state.list_children(10).len(), 2);
    kill_finalize(&mut state, 100);
    assert_eq!(state.list_children(10).len(), 1);
    assert_eq!(state.list_children(10)[0].index, 2);
    // unregister된 자식의 idle/needs_input 데이터도 함께 사라져야 한다.
    assert_eq!(state.parent_of_child(100), None);
}

// ─── step 04c: broadcast/tell helper tests ──────────────────────────────

#[test]
fn broadcast_targets_includes_all_children_without_filter() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 1));
    state.register_child(10, entry(101, 2));
    let ids = broadcast_targets(&state, 10, None);
    assert_eq!(ids, vec![100, 101]);
}

#[test]
fn broadcast_targets_filters_by_role() {
    let mut state = ClaudeState::default();
    state.register_child(
        10,
        ChildEntry {
            child_surface_id: 100,
            index: 1,
            cwd: None,
            role: Some("planner".into()),
            nickname: None,
        },
    );
    state.register_child(
        10,
        ChildEntry {
            child_surface_id: 101,
            index: 2,
            cwd: None,
            role: Some("worker".into()),
            nickname: None,
        },
    );
    state.register_child(
        10,
        ChildEntry {
            child_surface_id: 102,
            index: 3,
            cwd: None,
            role: Some("worker".into()),
            nickname: None,
        },
    );
    let ids = broadcast_targets(&state, 10, Some("worker"));
    assert_eq!(ids, vec![101, 102]);
}

#[test]
fn broadcast_targets_empty_when_unknown_parent() {
    let state = ClaudeState::default();
    assert!(broadcast_targets(&state, 999, None).is_empty());
}

#[test]
fn build_tell_pty_text_single_line_plain_cr() {
    // 단일라인은 평문 + 즉시 제출 \r (paste 미지원 수신측 회귀 위험 0).
    assert_eq!(build_tell_pty_text("hello"), "hello\r");
}

#[test]
fn build_tell_pty_text_multi_line_bracketed() {
    // "a\nb" → ESC[200~ a\nb ESC[201~ (멀티라인만 paste, 개행 그대로, 제출 \r 미포함).
    assert_eq!(build_tell_pty_text("a\nb"), "\u{1b}[200~a\nb\u{1b}[201~");
}

#[test]
fn build_tell_pty_text_trailing_backslash_single_line() {
    // 개행 없는 단일라인이므로 평문 + \r (paste 아님).
    assert_eq!(build_tell_pty_text("foo\\"), "foo\\\r");
}

#[test]
fn build_tell_pty_text_three_lines() {
    // "x\ny\nz" → ESC[200~ x\ny\nz ESC[201~ (제출 \r 미포함).
    assert_eq!(
        build_tell_pty_text("x\ny\nz"),
        "\u{1b}[200~x\ny\nz\u{1b}[201~"
    );
}

#[test]
fn build_tell_pty_text_empty_message() {
    // "" → 개행 없으므로 단일라인 경로: 평문 + \r.
    assert_eq!(build_tell_pty_text(""), "\r");
}

// ─── step 04d.1: launch helper tests ────────────────────────────────────

#[test]
fn build_launch_command_no_task() {
    assert_eq!(build_launch_command(None), "claude");
}

#[test]
fn build_launch_command_with_simple_task() {
    // shell_escape는 안전한 문자열을 그대로 둔다.
    assert_eq!(build_launch_command(Some("fix")), "claude --task fix");
}

// ─── step 04d.2: respawn helper tests ───────────────────────────────────

#[test]
fn update_child_metadata_noop_when_all_none() {
    let mut state = ClaudeState::default();
    state.register_child(10, entry(100, 1));
    let updated = update_child_metadata(&mut state, 10, 1, None, None, None);
    assert!(!updated, "should report no update when all fields are None");
}

#[test]
fn update_child_metadata_overwrites_only_given_fields() {
    let mut state = ClaudeState::default();
    state.register_child(
        10,
        ChildEntry {
            child_surface_id: 100,
            index: 1,
            cwd: Some("/old".into()),
            role: Some("old_role".into()),
            nickname: Some("old_nick".into()),
        },
    );
    let updated = update_child_metadata(&mut state, 10, 1, Some("/new"), None, Some("new_nick"));
    assert!(updated);
    let e = state.find_child(10, 1).unwrap();
    assert_eq!(e.cwd.as_deref(), Some("/new"));
    // role은 None이었으므로 보존되어야 한다.
    assert_eq!(e.role.as_deref(), Some("old_role"));
    assert_eq!(e.nickname.as_deref(), Some("new_nick"));
}

#[test]
fn update_child_metadata_returns_false_when_child_missing() {
    let mut state = ClaudeState::default();
    // 자식 등록 없음. 그래도 cwd가 주어졌으므로 attempt는 발생 — 그러나
    // update_child가 child 없음으로 false 반환 → wrapper도 false.
    let updated = update_child_metadata(&mut state, 10, 1, Some("/x"), None, None);
    assert!(!updated);
}

#[test]
fn build_launch_command_with_spaces_gets_escaped() {
    // 공백이 있으면 quote가 붙는다 — shell_escape의 표준 동작.
    let out = build_launch_command(Some("fix the bug"));
    assert!(out.starts_with("claude --task "), "prefix wrong: {out}");
    // 'fix the bug'으로 single-quote escape 되거나 다른 안전 escape.
    assert!(out.contains("fix the bug"), "task body missing: {out}");
    assert_ne!(out, "claude --task fix the bug", "must be escaped");
}

#[test]
fn kill_finalize_handles_nested_parent_case() {
    // child가 또 다른 parent를 가진 경우 (nested claude). mark_parent_closed가
    // 그 자식을 parent로 보고 closed_parents에 넣어야 한다.
    let mut state = ClaudeState::default();
    // 100은 10의 자식이면서 200/201의 부모.
    state.register_child(10, entry(100, 1));
    state.register_child(100, entry(200, 1));
    state.register_child(100, entry(201, 2));
    kill_finalize(&mut state, 100);
    // 100 자체는 10의 자식 자리에서 사라진다.
    assert_eq!(state.list_children(10).len(), 0);
    // 그러나 100을 부모로 하는 자식들은 그대로이고, 100이 closed로 마킹된다.
    assert!(state.is_parent_closed(100));
    assert_eq!(state.list_children(100).len(), 2);
}
