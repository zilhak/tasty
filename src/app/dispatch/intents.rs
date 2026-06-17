//! Intent 큐 drain + 도메인별 핸들러 분기 + caller 명시 라우터.

use winit::window::WindowId;

use crate::app::App;
use crate::ipc;
use crate::view::ui::View as _;

impl App {
    /// 호스트 내부 Intent 큐를 모든 AppState 에서 drain 해 도메인별 핸들러로 분기한다.
    /// 설계: `docs/design/flows/action-dispatch.md`. 처리 순서 = 발화 순서.
    /// drain 중 새로 발화된 Intent 는 다음 프레임에 처리 (재진입 방지).
    ///
    /// D.3.I.3 두 큐 통합 — `Intent::Domain(DomainIntent)` 도 본 메서드에서
    /// 처리한다. per-state batch 안의 `Intent::Domain` 항목은 따로 모아 본 loop
    /// 끝난 후 `dispatch_domain_intent` (App-level cascade) 로 일괄 처리. 두
    /// 단계 분리 이유: `dispatch_domain_intent` 가 `&mut self` 필요하지만 per-state
    /// loop 는 `&mut self.view.views[id]` 를 잡고 있어 동시 borrow 불가.
    pub(crate) fn dispatch_pending_intents(&mut self) {
        use crate::intent::{Intent, UiIntent};
        // 모든 windows + parked_states 에서 드레인한 뒤 일괄 처리.
        // 각 state 마다 독립적으로 처리해야 — popup mutation 은 그 state.popups 대상이므로.
        let mut per_state_batches: Vec<(WindowId, Vec<crate::intent::DispatchedIntent>)> =
            Vec::new();
        let mut parked_batches: Vec<(usize, Vec<crate::intent::DispatchedIntent>)> = Vec::new();
        let mut appearance_changed = false;

        for (id, w) in self.view.views.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                let batch = main.state.take_pending_intents();
                if !batch.is_empty() {
                    per_state_batches.push((*id, batch));
                }
            }
        }
        for (idx, (s, _)) in self.parked_states.iter_mut().enumerate() {
            let batch = s.take_pending_intents();
            if !batch.is_empty() {
                parked_batches.push((idx, batch));
            }
        }

        // Domain intents 는 separate batch — main loop 가 끝난 후 처리
        // (dispatch_domain_intent 가 &mut self 필요). 발화 source (Main(wid) /
        // Parked(idx)) 와 origin 을 보존해 cascade 가 정확한 engine/state 에
        // 접근하고 User/Agent/System 분기를 결정한다.
        let mut domain_batch: Vec<(
            crate::app::dispatch_domain::DispatchSource,
            crate::intent::DispatchedIntent,
        )> = Vec::new();

        for (window_id, batch) in per_state_batches {
            let core = &mut self.core;
            let Some(main) = self
                .view
                .views
                .get_mut(&window_id)
                .and_then(|w| w.as_main_mut())
            else {
                continue;
            };
            for intent in batch {
                #[cfg(debug_assertions)]
                crate::intent::watch::observe(&intent);
                if matches!(intent.body, Intent::Domain(_)) {
                    domain_batch.push((
                        crate::app::dispatch_domain::DispatchSource::Main(window_id),
                        intent,
                    ));
                    continue;
                }
                if matches!(intent.body, Intent::Ui(UiIntent::AppearanceChanged)) {
                    appearance_changed = true;
                    continue;
                }
                Self::dispatch_one_intent(core, &mut main.state, &mut main.core_state, &intent);
            }
            main.mark_dirty();
        }
        for (idx, batch) in parked_batches {
            let core = &mut self.core;
            let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                continue;
            };
            for intent in batch {
                #[cfg(debug_assertions)]
                crate::intent::watch::observe(&intent);
                if matches!(intent.body, Intent::Domain(_)) {
                    domain_batch.push((
                        crate::app::dispatch_domain::DispatchSource::Parked(idx),
                        intent,
                    ));
                    continue;
                }
                if matches!(intent.body, Intent::Ui(UiIntent::AppearanceChanged)) {
                    appearance_changed = true;
                    continue;
                }
                Self::dispatch_one_intent(core, state, engine, &intent);
            }
        }

        // Domain cascade — handle_core_event 가 App 메서드라 &mut self 필요.
        for (source, dispatched) in domain_batch {
            if let Err(e) = self.dispatch_domain_intent(source, dispatched) {
                tracing::warn!("dispatch_domain_intent failed: {e}");
            }
        }

        // Appearance fan-out — 모든 windows 의 GpuState 에 새 Theme 인스턴스를
        // 박고 egui ctx 에 reapply. dispatcher 가 single entry point 라 main 과
        // modal (settings / plugins / preset / quit) 모두 같은 프레임에 갱신된다.
        if appearance_changed {
            self.cascade_appearance_changed();
        }
    }

    /// Appearance (theme 색상 또는 host UI zoom) 가 바뀌었다는 단일 통지를 모든
    /// 윈도우로 fan-out 한다. settings 변경 cascade 와 단축키 발화 (Z-7) 가 같은
    /// entry point 로 모인다.
    ///
    /// 1. 전역 `Theme` 인스턴스를 `install_global_with_zoom` 으로 재빌드.
    /// 2. main + modal (settings / plugins / preset / quit) 의 GpuState 모두
    ///    `refresh_theme()` 호출 + mark_dirty.
    pub(crate) fn cascade_appearance_changed(&mut self) {
        // appearance 의 single source — focused main 의 core_state.settings.appearance.
        // focused 가 없으면 어떤 main 이든 (clone 으로 settings 동기화돼 있음).
        let appearance = self
            .focused_window()
            .map(|w| w.core_state.settings.appearance.clone())
            .or_else(|| {
                self.view.views.values().find_map(|w| {
                    w.as_main()
                        .map(|m| m.core_state.settings.appearance.clone())
                })
            })
            .or_else(|| {
                self.parked_states
                    .first()
                    .map(|(_, e)| e.settings.appearance.clone())
            });
        let Some(appearance) = appearance else {
            return;
        };
        let ui_zoom = appearance.ui_scale_factor();
        tasty_themes::install_global_with_zoom(&appearance, ui_zoom);

        // Broadcast: 모든 윈도우의 GpuState 가 새 Theme 을 egui ctx 에 reapply.
        for w in self.view.views.values_mut() {
            w.base_mut().gpu.refresh_theme();
            w.mark_dirty();
        }

        // Re-plumb the new theme palette into every terminal so OSC 10/11/12/4
        // color queries report the new colors (decision H3 (가)). Covers main +
        // parked engines, mirroring `cascade_internal_clipboard_copy`.
        for main in self.main_windows_iter_mut() {
            main.core_state.resync_terminal_palettes();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.resync_terminal_palettes();
        }
    }

    /// 단일 Intent 를 도메인 핸들러로 분기한다.
    fn dispatch_one_intent(
        core: &mut crate::core::Core,
        state: &mut crate::state::AppState,
        engine: &mut crate::core::CoreState,
        intent: &crate::intent::DispatchedIntent,
    ) {
        use crate::intent::Intent;
        match &intent.body {
            Intent::Ui(_) => {
                crate::intent::popup::handle(state, intent);
            }
            Intent::ApplyPreset { .. } | Intent::SavePreset { .. } => {
                crate::intent::preset::handle(core, state, engine, intent);
            }
            Intent::SplitSurface { .. } | Intent::ConvertSurface { .. } => {
                crate::intent::surface::handle(core, state, engine, intent);
            }
            Intent::NewTab { .. } => {
                crate::intent::tab::handle(core, state, engine, intent);
            }
            Intent::SplitPane { .. } => {
                crate::intent::pane::handle(core, state, engine, intent);
            }
            Intent::NewWorkspace { .. } => {
                crate::intent::workspace::handle(core, state, engine, intent);
            }
            Intent::RestoreClosedItem => {
                crate::intent::closed_item::handle(core, state, engine, intent);
            }
            Intent::Domain(_) => {
                // unreachable — dispatch_pending_intents 가 본 variant 를
                // domain_batch 로 분리해 본 함수를 우회하므로 들어올 수 없다.
                // defensive — 로그만 남기고 무시.
                tracing::error!(
                    "dispatch_one_intent reached Intent::Domain (should be handled in domain_batch)"
                );
            }
        }
    }

    /// caller를 명시한 라우터 디스패치. Plugin caller도 처리할 수 있도록 핸들러
    /// 진입점에 caller를 주입한다. 호스트 자체 메서드(window.*/plugin.*)는
    /// `process_ipc`가 별도로 처리하므로 여기서는 라우터에만 위임한다.
    ///
    /// Routing 우선순위 (CLAUDE.md "포커스 독립" 원칙):
    /// 1. request 가 surface_id/workspace_id/pane_id 를 명시했고 owner main 이
    ///    있으면 그 main 으로 — focus 와 무관하게 ID 로 직접 라우팅.
    /// 2. owner 못 찾으면 focused main 으로 (대상 미지정 / list 외 폴백).
    /// 3. 그것도 없으면 parked[0].
    pub(crate) fn dispatch_with_caller(
        &mut self,
        request: &ipc::protocol::JsonRpcRequest,
        caller: &ipc::caller::CallerContext,
    ) -> ipc::protocol::JsonRpcResponse {
        // list 류는 모든 engine 결과를 합쳐 반환 (포커스 독립 원칙).
        if let Some(resp) = self.dispatch_list_global(request) {
            return resp;
        }
        // clipboard clear/remove 는 모든 engine 에 broadcast (다른 윈도우의
        // history 도 함께 비워야 사용자 일관성).
        if let Some(resp) = self.dispatch_clipboard_global(request) {
            return resp;
        }
        let target_id = self
            .find_request_owner(&request.params)
            .or(self.view.focused_view_id);
        if let Some(id) = target_id {
            let core = &mut self.core;
            let resp_opt = self
                .view
                .views
                .get_mut(&id)
                .and_then(|w| w.as_main_mut())
                .map(|w| {
                    let r = ipc::handler::handle_with_caller(
                        core,
                        &mut w.state,
                        &mut w.core_state,
                        request,
                        caller,
                    );
                    w.base.dirty = true;
                    r
                });
            if let Some(response) = resp_opt {
                self.dispatch_pending_intents();
                return response;
            }
        }
        // parked 도 owner 검사 후 fallback.
        let owner_in_parked = crate::app::request_owner::params_resource_id(&request.params)
            .and_then(|(_, rid)| {
                self.parked_states.iter_mut().find(|(_, e)| match rid.kind {
                    crate::app::request_owner::Kind::Surface => e.has_surface(rid.id),
                    crate::app::request_owner::Kind::Workspace => e.has_workspace(rid.id),
                    crate::app::request_owner::Kind::Pane => e.has_pane(rid.id),
                })
            });
        if let Some((state, engine)) = owner_in_parked {
            let response =
                ipc::handler::handle_with_caller(&mut self.core, state, engine, request, caller);
            self.dispatch_pending_intents();
            return response;
        }
        if let Some((state, engine)) = self.parked_states.first_mut() {
            let response =
                ipc::handler::handle_with_caller(&mut self.core, state, engine, request, caller);
            self.dispatch_pending_intents();
            return response;
        }
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        ipc::protocol::JsonRpcResponse::error(id, -32000, "no application state available")
    }
}
