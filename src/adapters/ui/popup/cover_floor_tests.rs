//! 메인 창의 host Window 덮개(명령 팔레트) 아래 창 위젯은 egui 포커스를 받지 않는다. 창 하한은 관리자의 Window
//! 덮개 가운데 맨 위의 덮음 막이 레이어다(`PopupManager::window_cover_floor`). 창처럼 배경 위젯을 그린 뒤 host 팝업
//! 층(`draw_popup_layer`)을 그리고, 키는 창 입력 큐(`WindowInput`)로 넣는다.

use super::repro::screen;
use super::{PANE_L, layout, scene};
use crate::adapters::ui::popup::command_palette::COMMAND_PALETTE_POPUP_ID;
use crate::adapters::ui::popup::focus::PopupNode;
use crate::adapters::ui::popup::frame::draw_popup_layer;
use crate::adapters::ui::popup::window_input_test_support::WindowInput;
use crate::adapters::ui::popup::{PopupInstanceId, PopupScope, defs};
use crate::gfx::gpu::text_selection::HeldPastes;
use crate::intent::OpenRequest;
use crate::runtime::engine_read::EngineRead;
use crate::state::RequestContext;
use crate::state::focus_node::FocusNode;

/// 팔레트 아래 배경 입력칸 id.
const SIDE: &str = "cover_floor_side_field";

fn tab() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: Some(egui::Key::Tab),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

/// 창처럼 배경 입력칸을 그린 뒤 host 팝업 층을 그리는 패스를 남은 몫이 없을 때까지 돌리고 빈 패스 하나를 더 그린다.
fn drain(
    ctx: &egui::Context,
    state: &mut RequestContext,
    engine: &EngineRead<'_>,
    input: &mut WindowInput,
    side: &mut String,
) {
    loop {
        let more = !input.is_empty();
        let mut held = HeldPastes::default();
        drop(input.run_pass(
            state,
            ctx,
            screen(),
            &mut held,
            &mut |_| {},
            |ctx, state| {
                let take = state.surfaces_take_keys();
                crate::adapters::ui::popup::body_keys::with_body_keys(ctx, take, || {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.add(egui::TextEdit::singleline(side).id(egui::Id::new(SIDE)));
                    });
                });
                draw_popup_layer(ctx, state, engine, &layout());
            },
        ));
        if !more {
            return;
        }
    }
}

/// `drain` 의 한 패스를 돌고, 위젯을 그리는 때(패스 첫머리의 `release_closed_focus` 뒤) egui 가 읽는 창 하한을 돌려준다.
fn floor_in_pass(
    ctx: &egui::Context,
    state: &mut RequestContext,
    engine: &EngineRead<'_>,
    input: &mut WindowInput,
    side: &mut String,
) -> Option<egui::LayerId> {
    let mut floor = None;
    let mut held = HeldPastes::default();
    drop(input.run_pass(
        state,
        ctx,
        screen(),
        &mut held,
        &mut |_| {},
        |ctx, state| {
            floor = ctx.memory(|m| m.top_modal_layer());
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add(egui::TextEdit::singleline(side).id(egui::Id::new(SIDE)));
            });
            draw_popup_layer(ctx, state, engine, &layout());
        },
    ));
    floor
}

fn open_palette(state: &mut RequestContext) -> PopupInstanceId {
    state
        .popups
        .open_request(
            &OpenRequest::new(COMMAND_PALETTE_POPUP_ID, PopupScope::Window)
                .centered()
                .focused(),
            true,
        )
        .expect("palette opens")
}

/// `iid` 의 덮음 막이 레이어(그리는 자리 `draw.rs` 의 `area_id_of(..).with(COVER_BLOCK)`).
fn cover_block(id: crate::adapters::ui::popup::PopupId, iid: PopupInstanceId) -> egui::LayerId {
    egui::LayerId::new(
        egui::Order::Foreground,
        crate::adapters::ui::popup::draw::area_id_of(id, iid)
            .with(crate::adapters::ui::popup::draw::COVER_BLOCK),
    )
}

/// T-TAB-5: Given 메인 창에 명령 팔레트(host Window 덮개)가 열려 포커스 노드다, When Tab 을 거듭 치면, Then 포커스는
/// 덮인 배경 입력칸으로 가지 않는다.
#[test]
fn tab_under_the_command_palette_stays_off_the_window_widgets() {
    let (mut state, mut session) = scene(PANE_L);
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    let mut side = String::new();
    let palette = open_palette(&mut state);
    drain(&ctx, &mut state, &read, &mut input, &mut side);
    drain(&ctx, &mut state, &read, &mut input, &mut side);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Host(palette))
    );
    for step in 0..4 {
        assert!(
            input.key(&mut state, &read, false, tab()).is_some(),
            "Tab {step} reaches egui"
        );
        drain(&ctx, &mut state, &read, &mut input, &mut side);
        assert_ne!(
            ctx.memory(|m| m.focused()),
            Some(egui::Id::new(SIDE)),
            "Tab {step} moved to the covered window widget"
        );
    }
}

/// T-TAB-11: Given 메인 창에 명령 팔레트(host Window 덮개)가 그려졌고 그 위에 plugin Window 덮개가 있어 포커스 노드가
/// plugin 덮개다, When 창 하한을 고르면, Then 하한은 아래 host 덮개(팔레트)의 덮음 막이다. plugin 덮개는 하한에 들지
/// 않는다(계획 §1.1 범위).
#[test]
fn a_plugin_window_cover_over_a_host_cover_keeps_the_host_floor() {
    let (mut state, mut session) = scene(PANE_L);
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    let mut side = String::new();
    let palette = open_palette(&mut state);
    drain(&ctx, &mut state, &read, &mut input, &mut side);
    drain(&ctx, &mut state, &read, &mut input, &mut side);
    let z = tasty_host_plugin::next_popup_z_seq();
    state.plugin_popup_key_candidates.push((42, z));
    state
        .plugin_popup_bound_scopes
        .insert(42, ("p".to_string(), Some(PopupScope::Window)));
    state
        .plugin_popup_covers
        .push(crate::adapters::ui::popup::occlusion::Occluder {
            rect: screen(),
            z_seq: z,
        });
    state.normalize_focus(&read);
    assert_eq!(
        state.focus_node(&read),
        FocusNode::Popup(PopupNode::Plugin(42)),
        "the plugin cover is the focus node"
    );
    assert_eq!(
        state.popups.window_cover_floor(),
        Some(cover_block(COMMAND_PALETTE_POPUP_ID, palette)),
        "the floor stays on the host cover"
    );
}

/// Window 덮개 정의 `id` 를 연다.
fn open_window(
    state: &mut RequestContext,
    id: crate::adapters::ui::popup::PopupId,
) -> PopupInstanceId {
    state
        .popups
        .open_request(
            &OpenRequest::new(id, PopupScope::Window)
                .centered()
                .focused(),
            true,
        )
        .unwrap_or_else(|| panic!("{id} opens"))
}

/// E1: Given 팔레트(A) 위에 튜토리얼 목록(B)이 그려져 B 가 창 하한이다, When 패스 사이에 B 를 닫고 원격 도구(C)를
/// 함께 열면, Then 다음 패스가 위젯을 그리는 때의 하한은 지난 패스에 그려진 A 다(C 는 아직 그려지지 않았다). 하한이
/// 비면 그 패스의 Tab 이 덮인 창 위젯으로 갈 수 있다. 대조: C 없이 B 만 닫아도 하한은 A 다.
#[test]
fn a_cover_opened_with_the_close_keeps_the_live_cover_below_as_the_floor() {
    use crate::adapters::ui::popup::remote_tool::REMOTE_TOOL_POPUP_ID;
    use crate::adapters::ui::tutorial::topic_popup::TUTORIAL_TOPICS_POPUP_ID;
    for open_c in [true, false] {
        let (mut state, mut session) = scene(PANE_L);
        for def in defs::all_defs() {
            state.popups.register_def(def, 1.0);
        }
        let engine = session.borrow_mut();
        let read = engine.read();
        let ctx = egui::Context::default();
        let mut input = WindowInput::default();
        let mut side = String::new();
        let palette = open_palette(&mut state);
        drain(&ctx, &mut state, &read, &mut input, &mut side);
        let topics = open_window(&mut state, TUTORIAL_TOPICS_POPUP_ID);
        drain(&ctx, &mut state, &read, &mut input, &mut side);
        drain(&ctx, &mut state, &read, &mut input, &mut side);
        assert_eq!(
            ctx.memory(|m| m.top_modal_layer()),
            Some(cover_block(TUTORIAL_TOPICS_POPUP_ID, topics)),
            "B 가 하한이다"
        );
        state.popups.close_instance(
            topics,
            crate::adapters::ui::popup::PopupCloseReason::Command,
        );
        if open_c {
            open_window(&mut state, REMOTE_TOOL_POPUP_ID);
        }
        state.normalize_focus(&read);
        assert_eq!(
            floor_in_pass(&ctx, &mut state, &read, &mut input, &mut side),
            Some(cover_block(COMMAND_PALETTE_POPUP_ID, palette)),
            "open_c={open_c}: 위젯을 그리는 때의 하한이 남은 A 가 아니다"
        );
    }
}

/// E1: Given 팔레트(A) 하나가 그려져 하한이다, When 패스 사이에 A 를 닫고 원격 도구(C)를 함께 열면, Then 지난 패스에
/// 그려진 살아 있는 덮개가 없어도 열린 덮개가 있으므로 다음 패스가 위젯을 그리는 때 닫힌 A 의 하한을 둔다(비우지
/// 않는다). 그 패스 끝이 C 를 하한으로 등록한다.
#[test]
fn a_cover_opened_with_the_only_close_keeps_the_closed_floor_for_its_first_pass() {
    use crate::adapters::ui::popup::remote_tool::REMOTE_TOOL_POPUP_ID;
    let (mut state, mut session) = scene(PANE_L);
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    let mut side = String::new();
    let palette = open_palette(&mut state);
    drain(&ctx, &mut state, &read, &mut input, &mut side);
    drain(&ctx, &mut state, &read, &mut input, &mut side);
    state.popups.close_instance(
        palette,
        crate::adapters::ui::popup::PopupCloseReason::Command,
    );
    let c = open_window(&mut state, REMOTE_TOOL_POPUP_ID);
    state.normalize_focus(&read);
    assert_eq!(
        floor_in_pass(&ctx, &mut state, &read, &mut input, &mut side),
        Some(cover_block(COMMAND_PALETTE_POPUP_ID, palette)),
        "위젯을 그리는 때 하한이 비었다"
    );
    assert_eq!(
        ctx.memory(|m| m.top_modal_layer()),
        Some(cover_block(REMOTE_TOOL_POPUP_ID, c)),
        "패스 끝이 C 를 하한으로 등록했다"
    );
}

/// Codex 8: Given 팔레트(A)가 그려져 있고 지난 패스 끝 하한이 그 패스 순서에 없는 덮개 B 의 막이 레이어다(B 는 열렸지만
/// 아직 그려지지 않았다), When 다음 패스 전에 B 를 닫으면, Then 그 아래에 놓일 덮개가 없다(포크는 순서에 없는 레이어를
/// 순서 있는 레이어보다 낮게 비교한다). 살아 있는 A 가 있으므로 하한 B 를 그대로 둔다(A 로 올리지 않는다). 포크의
/// 등록(`set_modal_layer`)은 순서 있는 레이어가 이미 등록된 패스에서 순서 없는 B 를 받지 않으므로, 이 시험은 지난 패스 끝
/// 하한을 포크 패치(`sync_top_modal_layer`)로 B 로 둔다. 대조: B 가 그려진 뒤 닫히면 A 로 내린다
/// (`a_cover_opened_with_the_close_keeps_the_live_cover_below_as_the_floor`).
#[test]
fn a_closed_floor_that_was_never_drawn_has_no_cover_below_it() {
    use crate::adapters::ui::tutorial::topic_popup::TUTORIAL_TOPICS_POPUP_ID;
    let (mut state, mut session) = scene(PANE_L);
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    let engine = session.borrow_mut();
    let read = engine.read();
    let ctx = egui::Context::default();
    let mut input = WindowInput::default();
    let mut side = String::new();
    let palette = open_palette(&mut state);
    drain(&ctx, &mut state, &read, &mut input, &mut side);
    drain(&ctx, &mut state, &read, &mut input, &mut side);
    let topics = open_window(&mut state, TUTORIAL_TOPICS_POPUP_ID);
    let b = cover_block(TUTORIAL_TOPICS_POPUP_ID, topics);
    ctx.memory_mut(|m| m.sync_top_modal_layer(Some(b)));
    assert!(
        ctx.memory(|m| m.layer_ids().all(|l| l != b)),
        "대조: B 는 지난 패스 순서에 없다"
    );
    state.popups.close_instance(
        topics,
        crate::adapters::ui::popup::PopupCloseReason::Command,
    );
    state.normalize_focus(&read);
    let floor = floor_in_pass(&ctx, &mut state, &read, &mut input, &mut side);
    assert_ne!(
        floor,
        Some(cover_block(COMMAND_PALETTE_POPUP_ID, palette)),
        "순서에 없는 닫힌 하한을 A 로 올렸다"
    );
    assert_eq!(floor, Some(b), "살아 있는 덮개가 있는데 하한을 바꿨다");
}
