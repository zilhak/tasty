//! 전체화면 무대 상태 전이 + 닫힘 단일 수렴점 테스트.
//!
//! 렌더 파이프라인의 **분기 위치** 제약(무대 분기가 offscreen 스크린샷 뒤/레이아웃
//! 앞, `render_if_dirty` 조기 반환 금지, capture+present 유지)은 GPU 컨텍스트가
//! 필요해 여기서 단정할 수 없다 — 그쪽은 `tests/fullscreen_stage_render_gate.rs`
//! 가 구조 불변식으로 고정한다.

use super::tests::test_state;
use crate::adapters::ui::draw_popups;
use crate::adapters::ui::fullscreen;

/// 무대만 올린 상태에서 헤드리스 egui 프레임을 한 번 돌린다(무대 프레임 경로).
fn run_stage_frame(state: &mut crate::state::AppState, engine: &mut crate::core::CoreState) {
    let ctx = egui::Context::default();
    // 반환된 `FullOutput` 은 이 헬퍼의 관심사가 아니다 — 여기서 보는 것은 프레임을
    // 돈 뒤의 `state` 뿐이다(그린 shape 를 세는 테스트는 따로 있다).
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        crate::adapters::ui::draw_fullscreen_stage(ctx, state, engine);
    });
}

/// 일반 프레임 경로(무대가 없을 때 도는 쪽)를 한 번 돌린다.
fn run_normal_frame(state: &mut crate::state::AppState, engine: &mut crate::core::CoreState) {
    let ctx = egui::Context::default();
    // 위와 같은 이유로 `FullOutput` 은 버린다 — 검사 대상은 프레임 후의 `state`.
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        draw_popups(
            ctx,
            state,
            engine,
            &[],
            crate::model::PhysicalRect {
                x: crate::model::PhysicalPx(0.0),
                y: crate::model::PhysicalPx(0.0),
                width: crate::model::PhysicalPx(800.0),
                height: crate::model::PhysicalPx(600.0),
            },
            1.0,
        );
    });
}

#[test]
fn fullscreen_stage_marks_overlay_open() {
    let (mut state, _engine) = test_state();
    assert!(!state.has_egui_overlay_open());
    assert!(state.open_fullscreen_stage("blank"));
    // WebView 숨김이 이 판정에 달려 있다 — 네이티브 자식 뷰라 안 그리는 것만으로는
    // 무대를 뚫고 나온다.
    assert!(state.has_egui_overlay_open());
    assert!(state.close_fullscreen_stage());
    assert!(!state.has_egui_overlay_open());
}

#[test]
fn unknown_stage_id_is_rejected() {
    let (mut state, _engine) = test_state();
    assert!(!state.open_fullscreen_stage("no-such-stage"));
    assert!(!state.fullscreen_stage_active());
    assert!(state.stage_closed_queue.is_empty());
}

#[test]
fn only_one_stage_at_a_time() {
    let (mut state, _engine) = test_state();
    // 정적 테이블에 무대가 하나뿐이면 A→B 전환을 실제 id 로 만들 수 없다 — 그 경우
    // "같은 id 재진입은 no-op" 쪽(아래 테스트)만 성립하고 이 케이스는 vacuous 다.
    let ids: Vec<_> = fullscreen::defs::all_defs().iter().map(|d| d.id).collect();
    let (Some(a), Some(b)) = (ids.first().copied(), ids.get(1).copied()) else {
        return;
    };
    assert!(state.open_fullscreen_stage(a));
    assert!(state.open_fullscreen_stage(b));
    // A 가 정리되고 B 만 남는다 — 거절이 아니라 교체가 계약이다.
    assert_eq!(state.fullscreen_stage_id(), Some(b));
    assert_eq!(state.stage_closed_queue, vec![a]);
}

#[test]
fn reopening_the_same_stage_does_not_close_and_reopen() {
    let (mut state, _engine) = test_state();
    assert!(state.open_fullscreen_stage("blank"));
    assert!(state.open_fullscreen_stage("blank"));
    assert_eq!(state.fullscreen_stage_id(), Some("blank"));
    // 닫힘 훅이 발화하면 무대 콘텐츠 상태가 날아간다 — 재진입은 no-op 이어야 한다.
    assert!(state.stage_closed_queue.is_empty());
}

#[test]
fn close_pushes_exactly_one_hook_entry_and_is_idempotent() {
    let (mut state, _engine) = test_state();
    state.open_fullscreen_stage("blank");
    assert!(state.close_fullscreen_stage());
    // 두 번째 close 는 아무것도 하지 않는다(훅 중복 발화 금지).
    assert!(!state.close_fullscreen_stage());
    assert_eq!(state.stage_closed_queue, vec!["blank"]);
}

#[test]
fn normal_frame_drains_the_close_hook_queue() {
    // 무대를 나오면 그 다음 프레임은 **일반** 프레임이라 무대 draw 경로가 돌지
    // 않는다 — 일반 프레임도 drain 해야 훅이 유실되지 않는다.
    let (mut state, mut engine) = test_state();
    state.open_fullscreen_stage("blank");
    state.close_fullscreen_stage();
    assert!(!state.stage_closed_queue.is_empty());
    run_normal_frame(&mut state, &mut engine);
    assert!(state.stage_closed_queue.is_empty());
}

#[test]
fn stage_frame_drains_the_close_hook_queue() {
    // 무대 A→B 교체는 두 무대 사이에 일반 프레임이 끼지 않는다 — 무대 프레임도
    // 같은 drain 을 돌아야 한다.
    let (mut state, mut engine) = test_state();
    state.open_fullscreen_stage("blank");
    state.stage_closed_queue.push("blank");
    run_stage_frame(&mut state, &mut engine);
    assert!(state.stage_closed_queue.is_empty());
}

#[test]
fn stage_frame_paints_only_when_a_stage_is_up() {
    let (mut state, mut engine) = test_state();
    let painted = |state: &mut crate::state::AppState, engine: &mut crate::core::CoreState| {
        let ctx = egui::Context::default();
        let out = ctx.run(egui::RawInput::default(), |ctx| {
            crate::adapters::ui::draw_fullscreen_stage(ctx, state, engine);
        });
        out.shapes.len()
    };
    assert_eq!(painted(&mut state, &mut engine), 0);
    state.open_fullscreen_stage("blank");
    assert!(painted(&mut state, &mut engine) > 0);
}
