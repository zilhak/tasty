//! Application frame sequencing: View geometry first, engine effects second, readonly drawing last.
use super::App;
impl App {
    pub(crate) fn redraw_main_window(&mut self, id: winit::window::WindowId) {
        self.dispatch_pending_intents();
        if self.journal.is_halted() || self.journal.pauses_observation() {
            return;
        }
        self.poll_port_scans();
        self.refresh_approval_presentations();
        let Some(engine) = self.engines.of_window(id) else {
            return;
        };
        let Some(session) = self.engines.session_mut(engine) else {
            return;
        };
        let Some(view) = self
            .view
            .views
            .get_mut(&id)
            .and_then(|view| view.as_main_mut())
        else {
            return;
        };
        // 다른 윈도우가 바꾼 공용 목록(Explorer·포트 즐겨찾기)을 그리기 전에 받는다.
        if session.runtime.sync_shared_lists() {
            tracing::debug!(window = ?id, "shared lists: copy refreshed before redraw");
            crate::view::ui::View::mark_dirty(view);
        }
        view.prepare_redraw(&session.read());
        apply_frame_engine_actions(&mut view.state, session, self.plugin_manager.as_ref());
        // TerminalStore owns grid → tap → throttled OS resize. Input contexts see that cut.
        view.prepare_render_inputs(
            &session.read(),
            self.plugin_manager
                .as_ref()
                .map(super::plugin_display::PluginDisplay::new),
        );
        apply_frame_engine_actions(&mut view.state, session, self.plugin_manager.as_ref());
        if let Some(manager) = self.plugin_manager.as_ref() {
            for update in super::view_mesh::relay_subscribed_mesh(
                &mut session.borrow_mut(),
                manager,
                &self.stream_hub,
            ) {
                match update {
                    super::view_mesh::MeshViewUpdate::Bootstrap(boot) => {
                        view.note_mesh_bootstrap(*boot)
                    }
                    super::view_mesh::MeshViewUpdate::Full { surface } => {
                        view.request_mesh_full(surface)
                    }
                }
            }
        }
        view.render_if_dirty(
            &session.read(),
            self.plugin_manager
                .as_ref()
                .map(super::plugin_display::PluginDisplay::new),
        );
        // A banner decision from this frame must update its gate before native reload/sync.
        apply_frame_engine_actions(&mut view.state, session, self.plugin_manager.as_ref());
        view.finish_redraw(
            &session.read(),
            self.plugin_manager
                .as_ref()
                .map(super::plugin_display::PluginDisplay::new),
        );
        super::webview_sync::synchronize(
            view,
            &mut session.borrow_mut(),
            self.plugin_manager.as_ref(),
            &self.services.navigation_proofs,
        );
        self.process_remote_tool_requests(id);
        self.poll_port_scans();
        self.redraw_windows_for_shared_lists();
    }

    /// 공용 목록(Explorer·포트 즐겨찾기) 원본이 마지막으로 알린 리비전 뒤에 바뀌었으면 모든 메인 윈도우를
    /// 다시 그리게 한다. 변경은 프레임 안의 intent 루프에서도, 프레임 밖의 `dispatch_pending_intents` 에서도
    /// 적용되므로 두 곳 모두 이것을 부른다. 각 윈도우는 그리기 전에 사본을 맞춘다.
    pub(crate) fn redraw_windows_for_shared_lists(&mut self) {
        let registries = &self.services.registries;
        redraw_other_windows(
            [
                registries.explorer_favorites.revision(),
                registries.port_favorites.revision(),
            ],
            &mut self.shared_lists_announced,
            self.view
                .views
                .values_mut()
                .filter_map(|view| view.as_main_mut()),
            crate::view::ui::View::mark_dirty,
        );
    }
}

/// 프레임 단계 사이에 View가 낸 요청 중 Engine 효과만 지금 적용한다. 다른 요청은 출처 봉투를 그대로 둔 채
/// 같은 순서로 큐에 되돌려 다음 일반 dispatch가 처리한다. 호출 위치가 단계 순서를 정한다.
fn apply_frame_engine_actions(
    state: &mut crate::state::MainViewState,
    session: &mut crate::runtime::engine_session::EngineSession,
    plugins: Option<&crate::plugin::PluginManager>,
) {
    for intent in state.take_pending_intents() {
        match intent.body {
            crate::intent::Intent::Engine(action) => {
                action.apply(&mut session.borrow_mut(), plugins)
            }
            _ => state.dispatch_intent(intent),
        }
    }
}

/// `announced` 뒤에 원본 리비전이 바뀌었으면 기준을 올리고 모든 윈도우에 `redraw` 를 부른다.
fn redraw_other_windows<K: PartialEq + Copy, W>(
    current: K,
    announced: &mut K,
    windows: impl IntoIterator<Item = W>,
    mut redraw: impl FnMut(W),
) {
    if current == *announced {
        return;
    }
    *announced = current;
    for window in windows {
        redraw(window);
    }
}

#[cfg(test)]
mod tests {
    /// Engine 효과만 그 자리에서 적용되고, 나머지 요청은 출처와 함께 원래 순서로 큐에 남는다.
    #[test]
    fn a_frame_drain_applies_engine_effects_and_requeues_the_rest_in_order() {
        use crate::app::engine_action::{EngineAction, SettingsPatch};
        use crate::core::state::attention::AttentionKind;
        use crate::intent::Intent;

        let (mut state, mut session) = crate::state::tests::test_state();
        let sid = session
            .borrow_mut()
            .workspace_at(0)
            .expect("workspace index is valid")
            .all_surface_ids()[0];
        let target = crate::runtime::surface_binding::SurfaceBinding::capture(&session.read(), sid)
            .expect("binding");
        session
            .borrow_mut()
            .raise_attention(sid, AttentionKind::NeedsInput);
        let patch = |mode: &str| Intent::PatchSettings(SettingsPatch::ExplorerMode(mode.into()));
        state.dispatch_intent(patch("first").from_user_menu("first"));
        state.dispatch_intent(
            Intent::Engine(EngineAction::FocusObserved {
                target,
                window_focused: true,
            })
            .from_user_context_menu(),
        );
        state.dispatch_intent(patch("second").from_agent_ipc());

        super::apply_frame_engine_actions(&mut state, &mut session, None);

        assert_eq!(
            session.borrow_mut().attention_kind(sid),
            None,
            "engine effect ran"
        );
        let rest: Vec<String> = state
            .take_pending_intents()
            .into_iter()
            .map(|intent| format!("{:?} {:?}", intent.body, intent.origin))
            .collect();
        assert_eq!(
            rest,
            [
                format!(
                    "{:?} {:?}",
                    patch("first"),
                    patch("first").from_user_menu("first").origin
                ),
                format!(
                    "{:?} {:?}",
                    patch("second"),
                    patch("second").from_agent_ipc().origin
                ),
            ]
        );
    }

    /// prepare_redraw가 낸 기본 grid는 첫 drain에서 바로 적용돼 입력 준비가 새 값을 본다.
    #[test]
    fn the_geometry_drain_applies_the_default_grid_before_inputs() {
        use crate::app::engine_action::EngineAction;
        use crate::intent::Intent;

        let (mut state, mut session) = crate::state::tests::test_state();
        state.dispatch_intent(
            Intent::Engine(EngineAction::DefaultGrid {
                cols: 133,
                rows: 41,
            })
            .from_user_shortcut("grid-resize"),
        );

        super::apply_frame_engine_actions(&mut state, &mut session, None);

        let engine = session.borrow_mut();
        assert_eq!(
            (engine.runtime.default_cols, engine.runtime.default_rows),
            (133, 41)
        );
        assert!(state.take_pending_intents().is_empty());
    }

    /// html 탭 하나와 스크립트가 있는 확정 문서를 만든다. 배너가 허용을 기다린다.
    fn state_with_html_document() -> (
        crate::state::RequestContext,
        crate::runtime::engine_session::EngineSession,
        u32,
    ) {
        use tasty_core::{DomainEvent as E, SurfaceSpec};
        use tasty_model::html_script::{Fingerprint, ScriptDetection, ScriptScan};
        let model = crate::state::tests::test_model(vec![
            E::CategoryCreated {
                id: 0,
                name: "normal".into(),
                index: 0,
            },
            E::WorkspaceCreated {
                id: 1,
                name: "workspace".into(),
                category: 0,
                index: 0,
                pane: 1,
            },
            E::TabCreated {
                id: 1,
                pane: 1,
                index: 0,
                name: "html".into(),
                surface: SurfaceSpec {
                    id: 1,
                    kind: "html".into(),
                    data: None,
                },
            },
        ]);
        let (state, mut session) = crate::state::tests::test_state_from_model(model);
        let remote = crate::plugin_bridge::remote_surface::RemoteSurface::new(
            1,
            "html",
            "com.tasty.html".into(),
            "html".into(),
        );
        remote.with_html_script(|st| {
            st.on_host_load_requested();
            st.on_load_started();
            st.on_main_response(
                "file:///docs/a.html",
                Some(ScriptScan {
                    fingerprint: Fingerprint([0xab; 32]),
                    detection: ScriptDetection::Scripts,
                }),
            );
            st.on_committed(Some("file:///docs/a.html"));
            st.on_load_finished();
            st.update_banner();
        });
        session.runtime.surfaces.insert(1, Box::new(remote));
        (state, session, 1)
    }

    fn take_reload_request(
        session: &mut crate::runtime::engine_session::EngineSession,
        sid: u32,
    ) -> bool {
        session
            .runtime
            .surfaces
            .get(&sid)
            .and_then(|s| {
                s.as_any()
                    .downcast_ref::<crate::plugin_bridge::remote_surface::RemoteSurface>()
            })
            .expect("html remote surface")
            .with_html_script(|st| st.take_reload_request())
    }

    /// draw에서 누른 허용은 그 프레임의 마지막 drain에서 gate에 반영돼 native 동기화가 reload를 본다.
    #[test]
    fn an_allow_drawn_this_frame_reaches_the_gate_before_native_sync() {
        use crate::app::engine_action::EngineAction;
        use crate::intent::Intent;
        use crate::runtime::html_script::HtmlActionKind;

        let (mut state, mut session, sid) = state_with_html_document();
        let allow = session
            .read()
            .html_script(sid)
            .expect("html snapshot")
            .request(HtmlActionKind::Allow);
        state.dispatch_intent(
            Intent::Engine(EngineAction::Html(allow)).from_user_menu("html-script-banner"),
        );
        assert!(!take_reload_request(&mut session, sid));

        super::apply_frame_engine_actions(&mut state, &mut session, None);

        assert!(
            take_reload_request(&mut session, sid),
            "the allowance must request the reload before webview sync"
        );
    }

    use crate::core::explorer_favorites::ExplorerFavorites;
    use crate::core::port_favorites::PortFavorites;
    use crate::core::shared_list::SharedList;

    /// 프레임 밖에서 적용된 제거도, 다른 목록의 변경도 다음 확인 때 모든 윈도우를 한 번 깨운다.
    #[test]
    fn a_shared_list_change_redraws_every_window_once() {
        let _home = crate::test_support::IsolatedHome::new();
        let explorer = SharedList::<ExplorerFavorites>::default();
        let port = SharedList::<PortFavorites>::default();
        let current = || [explorer.revision(), port.revision()];
        let mut announced = current();
        let mut redrawn = [0, 0];
        let check = |announced: &mut [u64; 2], redrawn: &mut [i32; 2]| {
            super::redraw_other_windows(current(), announced, redrawn.iter_mut(), |n| *n += 1)
        };
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [0, 0], "nothing changed");

        let path = crate::test_support::abs_path("w/alpha");
        explorer.update(|f| f.add(path.clone(), String::new()));
        explorer.update(|f| f.remove(&path));
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [1, 1]);
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [1, 1], "already announced");

        port.update(|f| f.add("127.0.0.1".parse().unwrap(), 3000, String::new()));
        check(&mut announced, &mut redrawn);
        assert_eq!(redrawn, [2, 2], "a port favorite change also redraws");
    }
}
