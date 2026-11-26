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
