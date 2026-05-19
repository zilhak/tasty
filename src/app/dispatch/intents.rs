//! Intent 큐 drain + 도메인별 핸들러 분기 + caller 명시 라우터.

use winit::window::WindowId;

use crate::app::App;
use crate::ipc;
use crate::window::Window as _;

impl App {
    /// 호스트 내부 Intent 큐를 모든 AppState 에서 drain 해 도메인별 핸들러로 분기한다.
    /// 설계: `docs/design/action-dispatch.md`. 처리 순서 = 발화 순서.
    /// drain 중 새로 발화된 Intent 는 다음 프레임에 처리 (재진입 방지).
    pub(crate) fn dispatch_pending_intents(&mut self) {
        // 모든 windows + parked_states 에서 드레인한 뒤 일괄 처리.
        // 각 state 마다 독립적으로 처리해야 — popup mutation 은 그 state.popups 대상이므로.
        let mut per_state_batches: Vec<(WindowId, Vec<crate::intent::DispatchedIntent>)> =
            Vec::new();
        let mut parked_batches: Vec<(usize, Vec<crate::intent::DispatchedIntent>)> = Vec::new();

        for (id, w) in self.windows.iter_mut() {
            if let Some(main) = w.as_main_mut() {
                let batch = main.state.take_pending_intents();
                if !batch.is_empty() {
                    per_state_batches.push((*id, batch));
                }
            }
        }
        for (idx, s) in self.parked_states.iter_mut().enumerate() {
            let batch = s.take_pending_intents();
            if !batch.is_empty() {
                parked_batches.push((idx, batch));
            }
        }

        for (window_id, batch) in per_state_batches {
            let Some(main) = self
                .windows
                .get_mut(&window_id)
                .and_then(|w| w.as_main_mut())
            else {
                continue;
            };
            for intent in batch {
                #[cfg(debug_assertions)]
                crate::intent::watch::observe(&intent);
                Self::dispatch_one_intent(&mut main.state, &intent);
            }
            main.mark_dirty();
        }
        for (idx, batch) in parked_batches {
            let Some(state) = self.parked_states.get_mut(idx) else {
                continue;
            };
            for intent in batch {
                #[cfg(debug_assertions)]
                crate::intent::watch::observe(&intent);
                Self::dispatch_one_intent(state, &intent);
            }
        }
    }

    /// 단일 Intent 를 도메인 핸들러로 분기한다.
    fn dispatch_one_intent(
        state: &mut crate::state::AppState,
        intent: &crate::intent::DispatchedIntent,
    ) {
        use crate::intent::Intent;
        match &intent.body {
            Intent::OpenPopup { .. }
            | Intent::ClosePopup { .. }
            | Intent::TogglePopup { .. } => {
                crate::intent::popup::handle(state, intent);
            }
            Intent::ApplyPreset { .. }
            | Intent::SavePreset { .. }
            | Intent::DeletePreset { .. }
            | Intent::RenamePreset { .. } => {
                crate::intent::preset::handle(state, intent);
            }
            Intent::SplitSurface { .. }
            | Intent::CloseSurface { .. }
            | Intent::ConvertSurface { .. } => {
                crate::intent::surface::handle(state, intent);
            }
            Intent::NewTab { .. } | Intent::CloseTab { .. } => {
                crate::intent::tab::handle(state, intent);
            }
            Intent::SplitPane { .. } => {
                crate::intent::pane::handle(state, intent);
            }
            Intent::NewWorkspace { .. } => {
                crate::intent::workspace::handle(state, intent);
            }
            Intent::Noop => {}
        }
    }

    /// caller를 명시한 라우터 디스패치. Plugin caller도 처리할 수 있도록 핸들러
    /// 진입점에 caller를 주입한다. 호스트 자체 메서드(window.*/plugin.*)는
    /// `process_ipc`가 별도로 처리하므로 여기서는 라우터에만 위임한다.
    pub(crate) fn dispatch_with_caller(
        &mut self,
        request: &ipc::protocol::JsonRpcRequest,
        caller: &ipc::caller::CallerContext,
    ) -> ipc::protocol::JsonRpcResponse {
        let focused_id = self.engine.focused_window_id;
        if let Some(id) = focused_id {
            if let Some(w) = self.windows.get_mut(&id).and_then(|w| w.as_main_mut()) {
                let response = ipc::handler::handle_with_caller(&mut w.state, request, caller);
                w.base.dirty = true;
                return response;
            }
        }
        if let Some(state) = self.parked_states.first_mut() {
            return ipc::handler::handle_with_caller(state, request, caller);
        }
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        ipc::protocol::JsonRpcResponse::error(id, -32000, "no application state available")
    }
}
