//! Intent 큐를 처리하고 검사된 IPC 요청을 대상 engine에 전달한다.

use winit::window::WindowId;

use crate::app::App;
use crate::ipc;
use crate::view::ui::View as _;

enum IntentClass {
    Domain,
    Appearance,
    Immediate,
}

impl App {
    /// 같은 state의 UI 요청을 먼저 순서대로 처리하고 Domain 요청은 뒤에 모아 처리한다.
    /// AppearanceChanged는 마지막에 한 번만 적용한다. 처리 중 추가한 요청은 다음 호출로 남긴다.
    /// Domain 처리는 App 전체를 빌리므로 창별 상태를 빌린 루프 밖에서 수행한다.
    /// docs/design/flows/action-dispatch.md 참조.
    pub(crate) fn dispatch_pending_intents(&mut self) {
        let (per_state_batches, parked_batches) = self.drain_pending_batches();

        let mut domain_batch: Vec<(
            crate::app::dispatch_domain::DispatchSource,
            crate::intent::DispatchedIntent,
        )> = Vec::new();
        let mut appearance_changed = false;
        self.process_state_batches(
            per_state_batches,
            parked_batches,
            &mut domain_batch,
            &mut appearance_changed,
        );

        self.run_domain_cascade(domain_batch);

        if appearance_changed {
            self.cascade_appearance_changed();
        }
    }

    /// 큐의 소유권을 옮겨 둔 뒤 창·parked 상태의 빌림을 끝내고 처리한다.
    fn drain_pending_batches(
        &mut self,
    ) -> (
        Vec<(WindowId, Vec<crate::intent::DispatchedIntent>)>,
        Vec<(usize, Vec<crate::intent::DispatchedIntent>)>,
    ) {
        let mut per_state_batches = Vec::new();
        let mut parked_batches = Vec::new();
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
        (per_state_batches, parked_batches)
    }

    fn process_state_batches(
        &mut self,
        per_state_batches: Vec<(WindowId, Vec<crate::intent::DispatchedIntent>)>,
        parked_batches: Vec<(usize, Vec<crate::intent::DispatchedIntent>)>,
        domain_batch: &mut Vec<(
            crate::app::dispatch_domain::DispatchSource,
            crate::intent::DispatchedIntent,
        )>,
        appearance_changed: &mut bool,
    ) {
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
                match Self::classify_intent(&intent) {
                    IntentClass::Domain => domain_batch.push((
                        crate::app::dispatch_domain::DispatchSource::Main(window_id),
                        intent,
                    )),
                    IntentClass::Appearance => *appearance_changed = true,
                    IntentClass::Immediate => Self::dispatch_one_intent(
                        core,
                        &mut main.state,
                        &mut main.core_state,
                        &intent,
                    ),
                }
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
                match Self::classify_intent(&intent) {
                    IntentClass::Domain => domain_batch.push((
                        crate::app::dispatch_domain::DispatchSource::Parked(idx),
                        intent,
                    )),
                    IntentClass::Appearance => *appearance_changed = true,
                    IntentClass::Immediate => {
                        Self::dispatch_one_intent(core, state, engine, &intent)
                    }
                }
            }
        }
    }

    fn classify_intent(intent: &crate::intent::DispatchedIntent) -> IntentClass {
        use crate::intent::{Intent, UiIntent};
        if matches!(intent.body, Intent::Domain(_)) {
            IntentClass::Domain
        } else if matches!(intent.body, Intent::Ui(UiIntent::AppearanceChanged)) {
            IntentClass::Appearance
        } else {
            IntentClass::Immediate
        }
    }

    fn run_domain_cascade(
        &mut self,
        domain_batch: Vec<(
            crate::app::dispatch_domain::DispatchSource,
            crate::intent::DispatchedIntent,
        )>,
    ) {
        for (source, dispatched) in domain_batch {
            if let Err(e) = self.dispatch_domain_intent(source, dispatched) {
                tracing::warn!("dispatch_domain_intent failed: {e}");
            }
        }
    }

    /// 테마·배율 변경을 모든 창의 GPU와 터미널 팔레트에 반영한다.
    pub(crate) fn cascade_appearance_changed(&mut self) {
        // 색과 런타임 값이 서로 다른 설정에서 나오지 않게 같은 사본을 읽는다.
        let picked = self
            .focused_window()
            .map(|w| &w.core_state.settings)
            .or_else(|| {
                self.view
                    .views
                    .values()
                    .find_map(|w| w.as_main().map(|m| &m.core_state.settings))
            })
            .or_else(|| self.parked_states.first().map(|(_, e)| &e.settings))
            .map(|s| (s.appearance.clone(), s.theme_runtime()));
        let Some((appearance, runtime)) = picked else {
            return;
        };
        tasty_themes::install_global_with_runtime(&appearance, runtime);

        for w in self.view.views.values_mut() {
            w.base_mut().gpu.refresh_theme();
            w.mark_dirty();
        }

        // 창 없는 engine도 OSC 색상 조회가 새 팔레트를 반환해야 한다.
        for main in self.main_windows_iter_mut() {
            main.core_state.resync_terminal_palettes();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.resync_terminal_palettes();
        }
    }

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
                tracing::error!(
                    "dispatch_one_intent reached Intent::Domain (should be handled in domain_batch)"
                );
            }
        }
    }

    /// 권한·telemetry cap·rate limit을 검사한다. 엔진 선택은 감사 저장소용이며 요청 대상은 바꾸지 않는다.
    /// 통과 객체를 같은 요청의 하위 라우팅에 전달해 검사를 다시 소비하지 않는다.
    pub(crate) fn gates_before_routing<'a>(
        &mut self,
        request: &'a ipc::protocol::JsonRpcRequest,
        caller: &'a ipc::caller::CallerContext,
    ) -> Result<ipc::handler::CheckedRequest<'a>, ipc::protocol::JsonRpcResponse> {
        let core = &mut self.core;
        if let Some(w) = self.view.views.values_mut().find_map(|v| v.as_main_mut()) {
            return ipc::handler::check_request(
                core,
                &mut w.state,
                &mut w.core_state,
                request,
                caller,
            );
        }
        if let Some((state, engine)) = self.parked_states.first_mut() {
            return ipc::handler::check_request(core, state, engine, request, caller);
        }
        ipc::handler::check_without_engine(request, caller)
    }

    /// 대상을 지정했으면 소유 창·parked engine을 찾고 없으면 오류를 반환한다.
    /// 대상이 없을 때만 workspace 이름·포커스 창·첫 parked 상태로 선택한다.
    pub(crate) fn dispatch_checked(
        &mut self,
        checked: &ipc::handler::CheckedRequest<'_>,
    ) -> ipc::protocol::JsonRpcResponse {
        let request = checked.request();
        if let Some(resp) = self.dispatch_list_global(request) {
            return resp;
        }
        let named =
            crate::core::request_target::request_resource_id(&request.method, &request.params);
        let target_id = match self.find_request_owner(&request.method, &request.params) {
            Ok(id) if named.is_some() => id,
            Ok(id) => id.or(self.view.focused_view_id),
            Err(msg) => {
                let id = request.id.clone().unwrap_or(serde_json::Value::Null);
                return ipc::protocol::JsonRpcResponse::invalid_params(id, msg);
            }
        };
        if let Some(id) = target_id {
            let core = &mut self.core;
            let resp_opt = self
                .view
                .views
                .get_mut(&id)
                .and_then(|w| w.as_main_mut())
                .map(|w| {
                    let r = ipc::handler::handle_checked_request(
                        core,
                        &mut w.state,
                        &mut w.core_state,
                        checked,
                    );
                    w.base.dirty = true;
                    r
                });
            if let Some(response) = resp_opt {
                self.dispatch_pending_intents();
                return response;
            }
        }
        let owner_in_parked = named.and_then(|rid| {
            self.parked_states
                .iter_mut()
                .find(|(_, e)| crate::core::request_target::engine_has_resource(e, rid))
        });
        if let Some((state, engine)) = owner_in_parked {
            let response =
                ipc::handler::handle_checked_request(&mut self.core, state, engine, checked);
            self.dispatch_pending_intents();
            return response;
        }
        if let Some(rid) = named {
            let id = request.id.clone().unwrap_or(serde_json::Value::Null);
            return ipc::protocol::JsonRpcResponse::invalid_params(
                id,
                crate::core::request_target::unowned_target_message(rid, &request.method),
            );
        }
        if let Some((state, engine)) = self.parked_states.first_mut() {
            let response =
                ipc::handler::handle_checked_request(&mut self.core, state, engine, checked);
            self.dispatch_pending_intents();
            return response;
        }
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        ipc::protocol::JsonRpcResponse::error(id, -32000, "no application state available")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_partitions_domain_appearance_immediate() {
        use crate::intent::{Intent, UiIntent};
        let dom = || crate::core::intent::DomainIntent::MoveWorkspace {
            from_index: 0,
            to_index: 0,
        };
        let batch = [
            dom().from_agent_ipc(),
            Intent::RestoreClosedItem.from_user_shortcut("t"),
            UiIntent::AppearanceChanged.from_user_menu("t"),
            dom().from_agent_ipc(),
        ];
        let classes: Vec<_> = batch.iter().map(App::classify_intent).collect();
        assert!(matches!(classes[0], IntentClass::Domain));
        assert!(matches!(classes[1], IntentClass::Immediate));
        assert!(matches!(classes[2], IntentClass::Appearance));
        assert!(matches!(classes[3], IntentClass::Domain));
    }

    /// 두 GUI 라우팅 파일에서 사전 검사 호출·반환의 원문 위치 또는 CheckedRequest 인자를 확인한다.
    /// 실행 경로나 실제 검사·기록 횟수는 증명하지 않는다. 주석·문자열도 제거하지 않는 텍스트 검사다.
    /// CheckedRequest의 실제 소비는 handler::checked 시험에서 확인하며 헤드리스는 이 스캔에 포함하지 않는다.
    #[test]
    fn every_routing_entry_gates_before_it_answers() {
        /// 호출 이름 후보. CheckedRequest를 받은 경로는 별도로 인정한다.
        const GATES: &[&str] = &[
            "gates_before_routing",
            "check_permission_gate",
            "ensure_allowed",
        ];
        /// 호출 이름과 첫 return 사이에 허용할 원문 바이트 거리. 실행 의미를 분석하는 값은 아니다.
        const RETURN_WINDOW: usize = 120;
        let sources = [
            ("src/app/dispatch/intents.rs", include_str!("intents.rs")),
            ("src/app/ipc/routing.rs", include_str!("../ipc/routing.rs")),
        ];
        let mut checked = 0usize;
        let mut naked = Vec::new();
        for (file, full) in sources {
            // 시험 문자열을 검사 대상으로 다시 읽지 않도록 test 모듈 앞까지만 사용한다.
            let src = full.split("\n#[cfg(test)]").next().unwrap_or(full);
            let mut rest = src;
            while let Some(at) = rest
                .find("\n    pub(crate) fn ")
                .or_else(|| rest.find("\n    fn "))
            {
                let head = &rest[at + 1..];
                let name: String = head[head.find("fn ").map_or(0, |i| i + 3)..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                let end = head[1..]
                    .find("\n    pub(crate) fn ")
                    .into_iter()
                    .chain(head[1..].find("\n    fn "))
                    .min()
                    .map_or(head.len(), |i| i + 1);
                let body = &head[..end];
                rest = &head[end..];

                let routes = body.contains("dispatch_list_global(")
                    || body.contains("handle_with_caller(")
                    || body.contains("handle_checked_request(")
                    || body.contains("dispatch_checked(");
                if !routes {
                    continue;
                }
                if body.contains("checked: &") && body.contains("handler::CheckedRequest") {
                    checked += 1;
                    continue;
                }
                if !body.contains("caller: &") {
                    continue;
                }
                checked += 1;
                let gate = GATES.iter().filter_map(|g| body.find(g)).min();
                let first_return = body.find("return ");
                match (gate, first_return) {
                    (None, _) => naked.push(format!("  {file}::{name} — 게이트 호출이 없다")),
                    (Some(g), Some(r)) if r < g => naked.push(format!(
                        "  {file}::{name} — 게이트보다 앞서는 조기 응답이 있다"
                    )),
                    (Some(g), Some(r)) if r - g > RETURN_WINDOW => naked.push(format!(
                        "  {file}::{name} — 게이트 결과가 곧바로 반환되지 않는다"
                    )),
                    (Some(_), None) if body.contains("Err(response) => response") => {}
                    (Some(_), None) => {
                        naked.push(format!("  {file}::{name} — 게이트 결과를 반환하지 않는다"))
                    }
                    _ => {}
                }
            }
        }
        assert!(
            checked >= 2,
            "라우팅 진입점이 {checked}개로 하한보다 적다. 함수 탐색 범위와 파서를 확인한다."
        );
        assert!(
            naked.is_empty(),
            "라우팅의 사전 검사 호출 또는 반환 형태가 예상과 다르다 ({}건):\n{}\n사전 검사를 거쳐 응답하는지 실제 분기와 파서 인식을 함께 확인한다.",
            naked.len(),
            naked.join("\n")
        );
    }
}
