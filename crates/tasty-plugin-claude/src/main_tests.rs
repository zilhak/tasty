//! `main_tests` 단위 테스트.

#![cfg(test)]

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
fn build_tell_pty_text_single_line_ends_with_cr() {
    assert_eq!(build_tell_pty_text("hello"), "hello\r");
}

#[test]
fn build_tell_pty_text_multi_line_uses_backslash_cr() {
    // "a\nb" → "a\<CR>b<CR>"
    assert_eq!(build_tell_pty_text("a\nb"), "a\\\rb\r");
}

#[test]
fn build_tell_pty_text_trailing_backslash_gets_space() {
    // 마지막 라인이 `\`로 끝나면 ` ` 삽입 후 `\r`.
    // "foo\\" → "foo\\ \r"
    assert_eq!(build_tell_pty_text("foo\\"), "foo\\ \r");
}

#[test]
fn build_tell_pty_text_three_lines() {
    // "x\ny\nz" → "x\<CR>y\<CR>z<CR>"
    assert_eq!(build_tell_pty_text("x\ny\nz"), "x\\\ry\\\rz\r");
}

#[test]
fn build_tell_pty_text_empty_message() {
    // "" → "\r" (single empty line + submit)
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

// ─── step 04d.3: spawn helper tests ─────────────────────────────────────

#[test]
fn caller_surface_id_reads_key_from_params() {
    assert_eq!(
        caller_surface_id(&json!({ "caller_surface_id": 42 })),
        Some(42)
    );
}

#[test]
fn caller_surface_id_missing_returns_none() {
    assert_eq!(caller_surface_id(&json!({})), None);
}

#[test]
fn caller_surface_id_wrong_type_returns_none() {
    assert_eq!(
        caller_surface_id(&json!({ "caller_surface_id": "42" })),
        None
    );
}

#[test]
fn pick_split_target_zero_surfaces_uses_vertical() {
    // empty slice: fallback path uses 0 as target.
    let (sid, dir) = pick_split_target(0, &[]);
    assert_eq!(sid, 0);
    assert_eq!(dir, "vertical");
}

#[test]
fn pick_split_target_one_surface_splits_vertical() {
    // 1 surface in tab → split vertically to create left|right (count becomes 2).
    let (sid, dir) = pick_split_target(1, &[10]);
    assert_eq!(sid, 10);
    assert_eq!(dir, "vertical");
}

#[test]
fn pick_split_target_two_surfaces_splits_first_horizontal() {
    // 2 surfaces (left|right) → split left horizontally → 3 surfaces.
    let (sid, dir) = pick_split_target(2, &[10, 20]);
    assert_eq!(sid, 10);
    assert_eq!(dir, "horizontal");
}

#[test]
fn pick_split_target_three_surfaces_splits_third_horizontal() {
    // 3 surfaces (left-top|left-bottom + right) → split right horizontally → 2x2.
    let (sid, dir) = pick_split_target(3, &[10, 20, 30]);
    assert_eq!(sid, 30);
    assert_eq!(dir, "horizontal");
}

#[test]
fn spawn_pane_cache_round_trip_via_state() {
    // resolve_or_create_spawn_pane은 HostHandle을 필요로 해서 직접 테스트는
    // 어렵지만, state-level 캐시 동작은 핵심이므로 검증한다.
    let mut state = ClaudeState::default();
    assert_eq!(state.spawn_pane_for(10, 5), None);
    state.set_spawn_pane(10, 5, 77);
    assert_eq!(state.spawn_pane_for(10, 5), Some(77));
    // 다른 (parent, workspace) 조합은 영향 없음.
    assert_eq!(state.spawn_pane_for(11, 5), None);
    assert_eq!(state.spawn_pane_for(10, 6), None);
    state.clear_spawn_pane(10, 5);
    assert_eq!(state.spawn_pane_for(10, 5), None);
}
