//! Intent 큐 drain + 도메인별 핸들러 분기 + caller 명시 라우터.

use winit::window::WindowId;

use crate::app::App;
use crate::ipc;
use crate::view::ui::View as _;

/// per-state batch 안 한 intent 의 처리 클래스. `classify_intent` 이 산출.
enum IntentClass {
    /// `Intent::Domain` — 단계 C(`run_domain_cascade`)로 지연.
    Domain,
    /// `UiIntent::AppearanceChanged` — bool 로 축약해 프레임 끝 1회 fan-out.
    Appearance,
    /// 그 외 — `dispatch_one_intent` 로 즉시 처리.
    Immediate,
}

impl App {
    /// 호스트 내부 Intent 큐를 모든 AppState 에서 drain 해 도메인별 핸들러로 분기한다.
    /// 설계: `docs/design/flows/action-dispatch.md`.
    ///
    /// 처리 순서 = **클래스별 부분순서**: 같은 state 내 non-Domain 은 FIFO 즉시,
    /// `Intent::Domain` 은 전부 뒤로 밀려 단계 C 에서 FIFO, `AppearanceChanged` 는
    /// 프레임 끝 1회로 축약. drain 중 새로 발화된 Intent 는 다음 프레임에 처리
    /// (재진입 방지 — `mem::take`).
    ///
    /// D.3.I.3: `Intent::Domain` 을 per-state loop 안에서 처리하지 못하는 이유는
    /// `dispatch_domain_intent` 가 `&mut self` 를 요구하는데 loop 는
    /// `&mut self.view.views[id]` 를 잡고 있어 동시 borrow 불가하기 때문. 그래서
    /// 단계 A(드레인)/B(per-state 처리+Domain 분리)/C(Domain cascade) 로 분리한다.
    pub(crate) fn dispatch_pending_intents(&mut self) {
        // 단계 A — 모든 windows + parked_states 에서 큐를 소유째 드레인.
        let (per_state_batches, parked_batches) = self.drain_pending_batches();

        // 단계 B — per-state 처리 + Domain 분리 + appearance 축약.
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

        // 단계 C — Domain cascade (handle_core_event 가 App 메서드라 &mut self 필요).
        self.run_domain_cascade(domain_batch);

        // 부수 fan-out — appearance (모든 윈도우 GpuState theme reapply + palette resync).
        if appearance_changed {
            self.cascade_appearance_changed();
        }
    }

    /// 단계 A — 모든 main window + parked state 의 pending intent 큐를 `mem::take`
    /// 로 비우고 소유 batch 로 이동. 처리는 하지 않는다(드레인만). 반환 시점에
    /// `self.view` / `self.parked_states` 빌림이 종료되므로 단계 B 가 필요한 필드만
    /// 짧게 재빌림할 수 있다.
    fn drain_pending_batches(
        &mut self,
    ) -> (
        Vec<(WindowId, Vec<crate::intent::DispatchedIntent>)>,
        Vec<(usize, Vec<crate::intent::DispatchedIntent>)>,
    ) {
        // 각 state 마다 독립 처리 — popup mutation 은 그 state.popups 대상이므로.
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

    /// 단계 B — 드레인된 소유 batch 를 state 단위로 처리. `Immediate` 는 즉시
    /// `dispatch_one_intent`, `Domain` 은 source 부착해 `domain_batch` 로 지연,
    /// `Appearance` 는 bool 로 축약. 입력이 소유 batch 라 매 iteration 의
    /// `&mut self.core` + `get_mut()` 재빌림이 충돌하지 않는다.
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

    /// per-state / parked 두 루프가 공유하는 분류 규칙. `Intent::Domain` 은 단계 C
    /// 로 지연, `AppearanceChanged` 는 축약, 나머지는 즉시 처리. 이 3분류가
    /// "클래스별 부분순서" semantics 의 단일 정의점이다.
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

    /// 단계 C — Domain cascade. `handle_core_event` 가 App 메서드라 `&mut self`
    /// 필요. 소유 batch 를 순회하므로 view/parked 빌림과 충돌 없음.
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
        // parked engines.
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
    /// Routing 우선순위 (핵심 원칙 3 "포커스 독립"):
    /// 요청이 대상을 **지목했는가**로 갈린다.
    /// - 지목함: owner main → parked owner → **에러**. 포커스로 안 샌다.
    /// - 안 함: `"workspace"` 문자열 대상 → focused main → parked[0].
    pub(crate) fn dispatch_with_caller(
        &mut self,
        request: &ipc::protocol::JsonRpcRequest,
        caller: &ipc::caller::CallerContext,
    ) -> ipc::protocol::JsonRpcResponse {
        // list 류는 모든 engine 결과를 합쳐 반환 (포커스 독립 원칙).
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
        let owner_in_parked = named.and_then(|rid| {
            self.parked_states
                .iter_mut()
                .find(|(_, e)| crate::core::request_target::engine_has_resource(e, rid))
        });
        if let Some((state, engine)) = owner_in_parked {
            let response =
                ipc::handler::handle_with_caller(&mut self.core, state, engine, request, caller);
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
                ipc::handler::handle_with_caller(&mut self.core, state, engine, request, caller);
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

    /// 단계 B 의 분류 규칙 고정 — Domain 지연 / appearance 축약 / 즉시처리.
    /// research §2 "클래스별 부분순서" semantics 의 회귀 방지. 이 테스트가 깨지면
    /// 누군가 Domain 을 인라인 처리로 되돌렸거나 appearance 축약을 없앤 것.
    #[test]
    fn classify_partitions_domain_appearance_immediate() {
        use crate::intent::{Intent, UiIntent};
        let dom = || crate::core::intent::DomainIntent::MoveWorkspace {
            from_index: 0,
            to_index: 0,
        };
        // 발화 순서: Domain, Immediate, Appearance, Domain.
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
}
