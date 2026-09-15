use super::handle_trigger;
use crate::adapters::ui::popup::{PopupScope, file_picker::FILE_PICKER_POPUP_ID};
use crate::adapters::ui::{
    LayoutContext,
    popup::{defs, frame::draw_popup_layer},
};
use crate::plugin_bridge::popup_scope::inherit_file_picker_scope;
use crate::state::AppState;
use crate::state::tests::test_state;
use serde_json::json;
use tasty_host_plugin::manager::PopupInstance;
use tasty_ipc::caller::CallerContext;

fn parent(scope: &str, target: Option<u32>) -> PopupInstance {
    PopupInstance {
        plugin_id: "com.example.picker".into(),
        popup_id: "open".into(),
        contribute: serde_json::from_value(json!({
            "id": "open", "trigger": {"kind": "event", "event_key": "test.open"},
            "scope": scope
        }))
        .unwrap(),
        z_seq: 1,
        scope_surface: target,
    }
}

fn trigger(owner: Option<u64>) -> (AppState, crate::core::CoreState) {
    let (mut state, mut engine) = test_state();
    for def in defs::all_defs() {
        state.popups.register_def(def, 1.0);
    }
    let caller = CallerContext::Plugin {
        plugin_id: "com.example.picker".into(),
        permissions: std::sync::Arc::new(Default::default()),
    };
    let response = handle_trigger(
        &mut state,
        &mut engine,
        &caller,
        json!(1),
        &json!({"owner_popup_instance": owner}),
    );
    assert!(response.result.is_some());
    for intent in state.take_pending_intents() {
        crate::intent::popup::handle(&mut state, &intent);
    }
    (state, engine)
}

#[test]
fn trigger_inherits_declaration_and_target_not_just_target() {
    for (decl, target, expected) in [
        ("surface", Some(17), PopupScope::Surface(17)),
        ("surface", None, PopupScope::Window),
        ("window", Some(17), PopupScope::Window),
        ("window", None, PopupScope::Window),
    ] {
        let (mut state, _) = trigger(Some(7));
        let inst = parent(decl, target);
        inherit_file_picker_scope(&mut state, [(7, &inst)].into_iter());
        assert_eq!(
            state.popups.get_mut(FILE_PICKER_POPUP_ID).unwrap().scope,
            expected
        );
    }
}

#[test]
fn ownerless_and_foreign_owner_cannot_inherit_another_plugins_scope() {
    for owner in [None, Some(8), Some(7)] {
        let (mut state, _) = trigger(owner);
        let mut inst = parent("surface", Some(17));
        if owner == Some(7) {
            inst.plugin_id = "com.example.other".into();
        }
        // A prior child must not leave its scope on the singleton host picker.
        state.popups.get_mut(FILE_PICKER_POPUP_ID).unwrap().scope = PopupScope::Surface(99);
        inherit_file_picker_scope(&mut state, [(7, &inst)].into_iter());
        assert_eq!(
            state.popups.get_mut(FILE_PICKER_POPUP_ID).unwrap().scope,
            PopupScope::Window
        );
    }
}

#[test]
fn hidden_child_keeps_selection_and_request_without_paint_hit_or_keyboard_gate() {
    let (mut state, mut engine) = trigger(Some(7));
    let inst = parent("surface", Some(17));
    inherit_file_picker_scope(&mut state, [(7, &inst)].into_iter());
    let data = state.dialogs.file_picker.as_mut().unwrap();
    data.selected = vec!["draft.md".into()];
    data.load = crate::state::FpLoadState::Loading {
        request_id: 123,
        sent_at: std::time::Instant::now(),
    };
    let request = data.requester.as_ref().unwrap().request_id;
    let dir = data.current_dir.clone();
    let ctx = egui::Context::default();
    let rect = egui::Rect::from_min_size(egui::pos2(100.0, 60.0), egui::vec2(400.0, 360.0));
    let mut layout = LayoutContext {
        active_workspace: 0,
        pane_rects: vec![],
        surface_rects: vec![(17, rect)],
        active_tabs: vec![],
    };
    let mut draw = |state: &mut AppState, layout: &LayoutContext, events| {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        };
        drop(ctx.run(raw, |ctx| draw_popup_layer(ctx, state, &mut engine, layout)));
    };
    draw(&mut state, &layout, vec![]);
    assert!(state.popups.has_focused());
    assert!(AppState::keyboard_overlay_open(&state));
    assert!(state.has_egui_overlay_open());
    let popup = state.popups.get_mut(FILE_PICKER_POPUP_ID).unwrap();
    assert_eq!(
        popup.size,
        rect.size(),
        "existing clamp follows inherited surface bounds"
    );
    assert!(rect.contains_rect(egui::Rect::from_min_size(popup.pos, popup.size)));
    let z = state.host_popup_hittest[0].z_seq;
    layout.surface_rects.clear();
    layout.active_workspace = 1;
    draw(
        &mut state,
        &layout,
        vec![
            egui::Event::PointerButton {
                pos: rect.center(),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
            egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    assert!(state.popups.is_open(FILE_PICKER_POPUP_ID));
    assert!(state.popups.get_mut(FILE_PICKER_POPUP_ID).unwrap().focused);
    assert!(!state.popups.has_focused());
    assert!(!state.popups.is_focused(FILE_PICKER_POPUP_ID));
    assert!(state.popups.focused_dismissal_target().is_none());
    assert!(!AppState::keyboard_overlay_open(&state));
    assert!(
        !state.has_egui_overlay_open(),
        "hidden child must not hide native WebViews"
    );
    assert!(!state.popup_hovered);
    assert!(state.host_popup_hittest.is_empty());
    assert!(state.popup_layers.is_empty());
    assert!(state.popup_escape_owner.is_none());
    assert!(state.popups.take_closed_queue().is_empty());
    let data = state.dialogs.file_picker.as_ref().unwrap();
    assert_eq!(data.selected, ["draft.md"]);
    assert_eq!(data.current_dir, dir);
    assert_eq!(data.requester.as_ref().unwrap().request_id, request);
    assert!(matches!(
        data.load,
        crate::state::FpLoadState::Loading {
            request_id: 123,
            ..
        }
    ));
    assert!(data.result.is_none());
    layout.surface_rects.push((17, rect));
    layout.active_workspace = 0;
    draw(&mut state, &layout, vec![]);
    assert!(state.popups.has_focused());
    assert!(AppState::keyboard_overlay_open(&state));
    assert_eq!(state.host_popup_hittest[0].z_seq, z);
    assert_eq!(
        state.dialogs.file_picker.as_ref().unwrap().selected,
        ["draft.md"]
    );
}

#[test]
fn closing_a_hidden_parent_closes_the_shell_without_waiting_for_draw() {
    let (mut state, _) = trigger(Some(7));
    let inst = parent("surface", Some(17));
    inherit_file_picker_scope(&mut state, [(7, &inst)].into_iter());
    crate::app::dispatch::plugin_popup_events::cancel_child_file_picker(
        &mut state,
        &[(7, tasty_plugin_protocol::PopupCloseReason::PluginRequest)],
    );
    assert!(!state.popups.is_open(FILE_PICKER_POPUP_ID));
    assert!(matches!(
        state.dialogs.file_picker.as_ref().unwrap().result,
        Some(crate::state::FilePickerResult::Cancelled)
    ));
    assert_eq!(state.popups.take_closed_queue(), [FILE_PICKER_POPUP_ID]);
    crate::app::dispatch::plugin_popup_events::cancel_child_file_picker(
        &mut state,
        &[(7, tasty_plugin_protocol::PopupCloseReason::PluginRequest)],
    );
    assert!(state.popups.take_closed_queue().is_empty());
}
