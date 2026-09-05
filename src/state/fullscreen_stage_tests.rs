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
    drop(ctx.run(egui::RawInput::default(), |ctx| {
        crate::adapters::ui::draw_fullscreen_stage(ctx, state, engine);
    }));
}

/// 일반 프레임 경로(무대가 없을 때 도는 쪽)를 한 번 돌린다.
fn run_normal_frame(state: &mut crate::state::AppState, engine: &mut crate::core::CoreState) {
    let ctx = egui::Context::default();
    // 위와 같은 이유로 `FullOutput` 은 버린다 — 검사 대상은 프레임 후의 `state`.
    drop(ctx.run(egui::RawInput::default(), |ctx| {
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
    }));
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
    use std::sync::atomic::Ordering;

    let (mut state, mut engine) = test_state();
    // 교체 계약은 정의가 둘 이상일 때만 실제로 걷힌다. 두 번째 무대는 테이블에
    // `#[cfg(test)]` 로 등록돼 있다(`fullscreen::defs::TEST_STAGE_ID`).
    let b = fullscreen::defs::TEST_STAGE_ID;
    assert!(state.open_fullscreen_stage(b));
    let closes_before = fullscreen::defs::TEST_STAGE_CLOSES.load(Ordering::Relaxed);

    assert!(state.open_fullscreen_stage("blank"));
    // B 가 정리되고 A 만 남는다 — 거절이 아니라 교체가 계약이다.
    assert_eq!(state.fullscreen_stage_id(), Some("blank"));
    assert_eq!(state.stage_closed_queue, vec![b]);

    // 큐에 들어간 것으로 끝이 아니라, 다음 프레임에 훅이 **실제로** 발화해야 한다.
    run_stage_frame(&mut state, &mut engine);
    assert!(state.stage_closed_queue.is_empty());
    assert_eq!(
        fullscreen::defs::TEST_STAGE_CLOSES.load(Ordering::Relaxed),
        closes_before + 1,
        "교체로 밀려난 무대의 on_close 가 발화하지 않았다"
    );
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

// ── 첫 실콘텐츠 소비자(알림 무대) 배선 ────────────────────────────────────────

/// 화면 rect 를 갖는 일반 프레임 — popup hit-test 가 실제 좌표로 돌아야 하므로
/// `run_normal_frame` 과 달리 `RawInput` 을 호출자가 채운다.
fn run_normal_frame_with_input(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    raw: egui::RawInput,
) {
    let ctx = egui::Context::default();
    drop(ctx.run(raw, |ctx| {
        draw_popups(
            ctx,
            state,
            engine,
            &[],
            crate::model::PhysicalRect {
                x: crate::model::PhysicalPx(0.0),
                y: crate::model::PhysicalPx(0.0),
                width: crate::model::PhysicalPx(1920.0),
                height: crate::model::PhysicalPx(1080.0),
            },
            1.0,
        );
    }));
}

fn screen_input() -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1920.0, 1080.0),
        )),
        ..Default::default()
    }
}

/// popup.rs 의 private `close_btn_rect()`/`fullscreen_btn_rect()` 산술 복제 —
/// `popup_close_tests` 가 X 버튼 좌표를 구하는 방식과 같은 관례다(공개 필드
/// `pos`/`size` + 공개 함수만 사용).
fn fullscreen_btn_center(pos: egui::Pos2, size: egui::Vec2) -> egui::Pos2 {
    let title_h = crate::adapters::ui::popup::title_bar_height().value();
    let btn = 20.0;
    let close_center_x = pos.x + size.x - btn * 0.5 - 4.0;
    egui::pos2(
        close_center_x - btn - crate::adapters::ui::popup::title_btn_gap(),
        pos.y + title_h * 0.5,
    )
}

/// 타이틀바 전체화면 버튼 클릭 → 무대 진입. **원본 popup 은 열린 채 남는다** —
/// 무대에 올라간 것은 이 popup 이 아니라 같은 형상의 별개 콘텐츠이기 때문이다.
#[test]
fn clicking_the_popup_fullscreen_button_opens_the_stage_and_keeps_the_popup() {
    let (mut state, mut engine) = test_state();
    state
        .popups
        .open_at_focused("notifications", egui::pos2(400.0, 300.0)); // intent-exempt: 테스트 하네스.

    // sizer 가 없는 popup 이지만 등록 시 zoom 이 곱해지므로 실제 size 를 읽는다.
    let (pos, size) = {
        let p = state.popups.get_mut("notifications").expect("등록된 popup");
        (p.pos, p.size)
    };

    let mut raw = screen_input();
    raw.events.push(egui::Event::PointerButton {
        pos: fullscreen_btn_center(pos, size),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    run_normal_frame_with_input(&mut state, &mut engine, raw);

    assert_eq!(
        state.fullscreen_stage_id(),
        Some(fullscreen::notifications::NOTIFICATIONS_STAGE_ID)
    );
    assert!(
        state.popups.is_open("notifications"),
        "무대는 popup 을 닫지 않는다 — 덮을 뿐이다"
    );
}

/// 버튼을 선언하지 않은 popup 은 같은 좌표를 눌러도 무대가 뜨지 않는다
/// (플래그가 rect 를 만들지 않으므로 hit-test 자체가 성립하지 않는다).
#[test]
fn the_same_click_on_a_popup_without_the_flag_does_nothing() {
    let (mut state, mut engine) = test_state();
    state
        .popups
        .open_at_focused("rename", egui::pos2(400.0, 300.0)); // intent-exempt: 테스트 하네스.
    let (pos, size) = {
        let p = state.popups.get_mut("rename").expect("등록된 popup");
        (p.pos, p.size)
    };

    let mut raw = screen_input();
    raw.events.push(egui::Event::PointerButton {
        pos: fullscreen_btn_center(pos, size),
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::default(),
    });
    run_normal_frame_with_input(&mut state, &mut engine, raw);

    assert!(!state.fullscreen_stage_active());
}

/// "별개 데이터" 를 직접 단정한다 — 무대의 스크롤 상태는 **원본 popup 의 것과 다른
/// egui 엔트리**다. popup 이 먼저 자기 스크롤 상태를 만든 뒤 같은 `Context` 에서 무대
/// 프레임을 돌렸을 때 엔트리 수가 1 → 2 로 늘고 무대가 기록한 id 가 살아 있으면,
/// 무대 id ≠ popup id 다(무대가 popup 의 것을 재사용했다면 수가 그대로 1 이다).
///
/// 무대 콘텐츠 Ui 를 `def.id` 로 salt 하는 배선이 사라지는 회귀를 여기서 잡는다.
#[test]
fn the_stage_scroll_state_is_a_different_entry_from_the_popups() {
    type ScrollState = egui::containers::scroll_area::State;
    let scroll_entries = |ctx: &egui::Context| ctx.memory(|m| m.data.count::<ScrollState>());

    let (mut state, mut engine) = test_state();
    let ctx = egui::Context::default();

    // ① popup 만 열린 일반 프레임 — 알림 목록의 ScrollArea 가 자기 상태를 만든다.
    state
        .popups
        .open_at_focused("notifications", egui::pos2(400.0, 300.0)); // intent-exempt: 테스트 하네스.
    drop(ctx.run(screen_input(), |ctx| {
        draw_popups(
            ctx,
            &mut state,
            &mut engine,
            &[],
            crate::model::PhysicalRect {
                x: crate::model::PhysicalPx(0.0),
                y: crate::model::PhysicalPx(0.0),
                width: crate::model::PhysicalPx(1920.0),
                height: crate::model::PhysicalPx(1080.0),
            },
            1.0,
        );
    }));
    assert_eq!(
        scroll_entries(&ctx),
        1,
        "popup 이 자기 스크롤 상태를 만들어야 이후 비교가 성립한다"
    );

    // ② 같은 Context 에서 무대 프레임.
    assert!(state.open_fullscreen_stage(fullscreen::notifications::NOTIFICATIONS_STAGE_ID));
    drop(ctx.run(screen_input(), |ctx| {
        crate::adapters::ui::draw_fullscreen_stage(ctx, &mut state, &mut engine);
    }));
    let stage_scroll_id = fullscreen::notifications::recorded_scroll_id(&ctx)
        .expect("무대 콘텐츠가 자기 스크롤 상태 id 를 기록해야 한다");
    assert!(
        ctx.data_mut(|d| d.get_persisted::<ScrollState>(stage_scroll_id))
            .is_some()
    );
    assert_eq!(
        scroll_entries(&ctx),
        2,
        "무대가 popup 의 스크롤 상태를 그대로 쓰고 있다 — 무대에서 목록을 내리면 \
         원본 popup 의 스크롤까지 따라 움직인다"
    );
}

/// 무대 사이의 격리 — 같은 콘텐츠를 **다른 id 로** 올린 두 무대는 서로 다른 스크롤
/// 상태를 쓴다. 이것이 셸의 `id_salt(def.id)` 가 실제로 하는 일이고, popup 과의 비교로는
/// 드러나지 않는다(무대와 popup 은 애초에 다른 `Area` 라 salt 없이도 갈린다).
#[test]
fn two_stages_with_the_same_content_do_not_share_scroll_state() {
    let (mut state, mut engine) = test_state();
    let ctx = egui::Context::default();
    let draw_stage = |state: &mut crate::state::AppState,
                      engine: &mut crate::core::CoreState,
                      id: &'static str| {
        assert!(state.open_fullscreen_stage(id));
        drop(ctx.run(screen_input(), |ctx| {
            crate::adapters::ui::draw_fullscreen_stage(ctx, state, engine);
        }));
        fullscreen::notifications::recorded_scroll_id(&ctx).expect("무대가 id 를 기록해야 한다")
    };

    let a = draw_stage(
        &mut state,
        &mut engine,
        fullscreen::notifications::NOTIFICATIONS_STAGE_ID,
    );
    let b = draw_stage(
        &mut state,
        &mut engine,
        fullscreen::defs::TEST_TWIN_STAGE_ID,
    );
    assert_ne!(
        a, b,
        "무대 콘텐츠 Ui 의 무대 id salt 가 사라졌다 — 두 무대가 스크롤 위치를 공유한다"
    );
}

/// 알림 무대는 자체 상태(목록 스크롤 위치)를 갖고, 무대가 닫히면 `on_close` 가
/// 그것을 지운다 — 무대 정의 타입의 정리 훅이 **실콘텐츠에서** 도는지 확인.
#[test]
fn notifications_stage_clears_its_own_scroll_state_on_close() {
    let (mut state, mut engine) = test_state();
    assert!(state.open_fullscreen_stage(fullscreen::notifications::NOTIFICATIONS_STAGE_ID));

    let ctx = egui::Context::default();
    drop(ctx.run(screen_input(), |ctx| {
        crate::adapters::ui::draw_fullscreen_stage(ctx, &mut state, &mut engine);
    }));
    let scroll_id = fullscreen::notifications::recorded_scroll_id(&ctx)
        .expect("무대 콘텐츠가 자기 스크롤 상태 id 를 기록해야 한다");
    assert!(
        ctx.data_mut(|d| d.get_persisted::<egui::containers::scroll_area::State>(scroll_id))
            .is_some(),
        "ScrollArea 가 실제로 상태를 남겨야 이후 정리 단정이 의미가 있다"
    );

    assert!(state.close_fullscreen_stage());
    // 닫은 다음 프레임은 일반 프레임이다 — 그쪽 drain 이 훅을 발화시킨다.
    drop(ctx.run(screen_input(), |ctx| {
        crate::adapters::ui::draw_popups(
            ctx,
            &mut state,
            &mut engine,
            &[],
            crate::model::PhysicalRect {
                x: crate::model::PhysicalPx(0.0),
                y: crate::model::PhysicalPx(0.0),
                width: crate::model::PhysicalPx(1920.0),
                height: crate::model::PhysicalPx(1080.0),
            },
            1.0,
        );
    }));
    assert!(fullscreen::notifications::recorded_scroll_id(&ctx).is_none());
    assert!(
        ctx.data_mut(|d| d.get_persisted::<egui::containers::scroll_area::State>(scroll_id))
            .is_none(),
        "무대 자체 상태가 종료 후에도 남아 있다"
    );
}
