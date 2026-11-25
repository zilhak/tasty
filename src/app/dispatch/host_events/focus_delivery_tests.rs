//! 포커스 이벤트의 발행 끝단. 키 포커스 감지가 낸 `SurfaceUnfocused`·`SurfaceFocused`가 `surface::emit_unfocused`·
//! `surface::emit_focused`를 지나 `surface.*`를 구독한 플러그인에 어떤 키와 payload로 가는지 본다.

use crate::adapters::ui::popup::PopupScope;
use crate::runtime::engine_session::EngineSession;
use crate::state::{PendingHostEvent, RequestContext};

const PLUGIN: &str = "com.example.focus";

/// `surface.*`를 구독한 stub 플러그인이 붙은 매니저.
fn subscribed_manager() -> (
    crate::plugin::PluginManager,
    tasty_host_plugin::manager::CallerStub,
) {
    let mut mgr = crate::plugin::PluginManager::with_registries(
        std::sync::Arc::new(tasty_terminal::waker_factory::NoopWakerFactory),
        std::sync::Arc::new(crate::file::format::FileFormatRegistry::new()),
        std::sync::Arc::new(crate::file::handler::FileHandlerRegistry::new()),
    );
    let stub = mgr.attach_caller_stub_for_test(PLUGIN);
    mgr.event_bus
        .set_plugin_declarations(PLUGIN, vec!["surface.*".into()], vec![]);
    mgr.event_bus
        .subscribe_plugin(PLUGIN, 1, "surface.*".into())
        .expect("subscribes");
    (mgr, stub)
}

/// 창이 프레임 하나를 화면에 낸 것처럼 마친다(`render_if_dirty`의 두 호출).
fn show(state: &mut RequestContext, session: &mut EngineSession) {
    let engine = session.borrow_mut();
    let read = engine.read();
    state.show_key_focus_frame(&read);
    state.note_shown_key_focus(&read, true);
}

/// 창마다 도는 감지 단계 뒤 대기 이벤트의 포커스 이벤트를 발행 함수로 보내고, 구독자가 받은 (키, payload)를 꺼낸다.
fn deliver(
    state: &mut RequestContext,
    session: &mut EngineSession,
    mgr: &mut crate::plugin::PluginManager,
    stub: &tasty_host_plugin::manager::CallerStub,
) -> Vec<(String, serde_json::Value)> {
    let mut engine = session.borrow_mut();
    super::observe_window_focus(state, &mut engine);
    for ev in super::take_current_host_events(&mut engine) {
        match ev {
            PendingHostEvent::SurfaceFocused {
                surface_id,
                prev_surface_id,
            } => super::surface::emit_focused(mgr, surface_id, prev_surface_id),
            PendingHostEvent::SurfaceUnfocused {
                surface_id,
                next_surface_id,
            } => super::surface::emit_unfocused(mgr, surface_id, next_surface_id),
            _ => {}
        }
    }
    stub.drain_requests()
        .into_iter()
        .filter(|(method, _)| method == tasty_plugin_protocol::METHOD_EVENT_DISPATCH)
        .map(|(_, params)| {
            (
                params["envelope"]["key"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
                params["envelope"]["payload"].clone(),
            )
        })
        .collect()
}

/// 키를 가져가는 팝업이 열리면 구독자는 `surface.unfocused {surface_id, next_surface_id: null}`을, 닫혀 돌아오면
/// `surface.focused {surface_id, prev_surface_id: null}`을 받는다.
#[test]
fn subscribers_receive_unfocused_and_focused_around_a_popup() {
    crate::i18n::init("en");
    let (mut state, mut session) = crate::state::tests::two_pane_fixture();
    session.runtime.surfaces.insert(
        201,
        Box::new(crate::runtime::egui_mesh_surface::EguiMeshSurface::new(
            201,
            "mesh-demo",
            "com.example.demo".into(),
            "Mesh".into(),
            None,
        )),
    );
    let (mut mgr, stub) = subscribed_manager();
    show(&mut state, &mut session);
    deliver(&mut state, &mut session, &mut mgr, &stub);

    state
        .popups
        .open_centered_focused("notifications", PopupScope::Window)
        .expect("opens");
    show(&mut state, &mut session);
    assert_eq!(
        deliver(&mut state, &mut session, &mut mgr, &stub),
        vec![(
            "surface.unfocused".to_string(),
            serde_json::json!({ "surface_id": 201, "next_surface_id": null })
        )]
    );

    state.popups.close("notifications");
    show(&mut state, &mut session);
    assert_eq!(
        deliver(&mut state, &mut session, &mut mgr, &stub),
        vec![(
            "surface.focused".to_string(),
            serde_json::json!({ "surface_id": 201, "prev_surface_id": null })
        )]
    );
}
