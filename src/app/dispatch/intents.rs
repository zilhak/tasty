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
    /// 1. 전역 `Theme` 인스턴스를 `install_global_with_runtime` 으로 재빌드.
    /// 2. main + modal (settings / plugins / preset / quit) 의 GpuState 모두
    ///    `refresh_theme()` 호출 + mark_dirty.
    pub(crate) fn cascade_appearance_changed(&mut self) {
        // appearance 의 single source — focused main 의 core_state.settings.
        // focused 가 없으면 어떤 main 이든 (clone 으로 settings 동기화돼 있음).
        // 색 레이어(appearance)와 런타임 값(배율·모션 감소)을 **같은 settings 에서**
        // 함께 꺼낸다 — 따로 꺼내면 한쪽만 갱신된 조합이 설치될 수 있다.
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

    /// 라우팅 이전에 도는 게이트 3종(권한 / telemetry cap / rate limit).
    ///
    /// `handle_with_caller` 안에도 같은 게이트가 있다. 중복이 아니라 **다른 경계**다 —
    /// 그쪽은 그 함수를 직접 부르는 진입점(headless · attach · routing)을 지키고,
    /// 이쪽은 그 함수에 **도달하지 않는** 조기 응답을 지킨다. 거부는 여기서
    /// 단락되므로 안쪽 게이트가 다시 돌지 않고, 통과는 부수효과가 없다.
    ///
    /// audit 기록에 쓰는 engine 은 아무 것이나 된다 — 기록 대상은 프로세스 하나가
    /// 공유하는 memory store 이고 engine 은 workspace id 와 telemetry 순번을 줄 뿐이다.
    pub(crate) fn gates_before_routing(
        &mut self,
        request: &ipc::protocol::JsonRpcRequest,
        caller: &ipc::caller::CallerContext,
    ) -> Option<ipc::protocol::JsonRpcResponse> {
        let canonical = ipc::alias::canonicalize(&request.method);
        let id = request.id.clone().unwrap_or(serde_json::Value::Null);
        let core = &mut self.core;

        let run = |core: &mut crate::core::Core,
                   engine: &mut crate::core::CoreState,
                   ws: Option<u32>| {
            ipc::handler::check_permission_gate(core, engine, caller, canonical, ws, &id)
                .or_else(|| ipc::handler::check_cap_gate(core, engine, caller, canonical, ws, &id))
                .or_else(|| {
                    ipc::handler::check_rate_limit_gate(core, engine, caller, canonical, ws, &id)
                })
        };

        if let Some(w) = self.view.views.values_mut().find_map(|v| v.as_main_mut()) {
            let ws = w
                .core_state
                .workspaces
                .get(w.state.active_workspace)
                .map(|x| x.id);
            return run(core, &mut w.core_state, ws);
        }
        if let Some((state, engine)) = self.parked_states.first_mut() {
            let ws = engine.workspaces.get(state.active_workspace).map(|x| x.id);
            return run(core, engine, ws);
        }
        // engine 이 하나도 없으면 audit 은 못 남기지만 **거부는 남는다** — 기록할 수
        // 없다는 이유로 통과시키면 부팅 직후 창이 없는 순간이 구멍이 된다.
        caller.ensure_allowed(canonical).err().map(|e| {
            tracing::warn!("ipc permission denied (no engine to audit): {e}");
            ipc::protocol::JsonRpcResponse::error(id, -32001, format!("permission_denied: {e}"))
        })
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
        // 게이트가 **라우팅보다 먼저** 돈다. 아래에는 `handle_with_caller` 에
        // 도달하지 않고 끝나는 경로가 여럿이다(list 합산 응답 · owner 해석 실패 ·
        // 지목한 대상이 없음 · app state 없음). 게이트가 그 안에만 있으면 그런
        // 조기 응답 하나마다 권한·cap·rate 세 검사가 통째로 건너뛰어진다 —
        // 실제로 `surface.list`/`workspace.list`/`pane.list` 가 그렇게 새고 있었다.
        if let Some(resp) = self.gates_before_routing(request, caller) {
            return resp;
        }
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

    /// **계약: 요청에 답하기 전에 게이트가 돈다.**
    ///
    /// 이 가드가 그 계약을 소유한다 — 문서는 여기를 가리키기만 한다. 지키는 성질은
    /// 하나다: *요청을 라우팅하는 함수 안에서, 그 함수가 응답을 돌려주는 **어떤**
    /// 자리보다 게이트 호출이 먼저 나온다.*
    ///
    /// ## 무엇을 세는가 — 구문이지 등재 목록이 아니다
    ///
    /// 함수 본문을 읽어 `return` 이 나오는 **첫 위치**와 게이트 호출 위치를 비교한다.
    /// 순서만 보면 "게이트를 부르되 결과를 버리는" 형태를 못 가르므로, 게이트 호출
    /// **바로 뒤**에 반환이 붙어 있는지도 함께 본다 — 거부를 실제로 돌려주지 않으면
    /// 부른 것이 아니다.
    /// 새 조기 응답을 어떤 형태로 추가하든(list 합산 · 캐시 · 지름길) 그 `return` 이
    /// 게이트보다 앞서면 그 자리에서 걸린다. 등재 목록으로 만들면 새로 넣은 사람이
    /// 등재를 안 할 때 조용히 통과하므로 — 그건 가드가 아니라 체크리스트다.
    ///
    /// ## 왜 이 계약인가
    ///
    /// 게이트가 라우터 **안쪽**(`handle_with_caller`)에만 있던 동안,
    /// `dispatch_with_caller` 는 `dispatch_list_global` 로 `surface.list` ·
    /// `workspace.list` · `pane.list` 를 먼저 답해 버렸다. 그 셋은 `surface.read` 를
    /// 요구한다고 선언돼 있는데 권한 없는 plugin 에게 그대로 나갔고(실측), 권한만이
    /// 아니라 telemetry cap · rate limit · audit 기록까지 함께 건너뛰었다 — 감사 로그에
    /// 흔적조차 남지 않았다. 같은 `surface.read` 를 요구하는 `tab.list` 는 라우터
    /// 안쪽까지 가서 정상 거부됐다. 즉 결함은 검사 로직이 아니라 **검사 위치**였다.
    ///
    /// ## 대상을 어떻게 고르는가
    ///
    /// caller 를 인자로 받아 라우팅하는 함수, 즉 `caller: &` 를 시그니처에 갖고
    /// 본문에서 `dispatch_list_global` 이나 `handle_with_caller` 를 부르는 것.
    /// 이름을 나열하지 않으므로 그런 함수가 새로 생기면 자동으로 대상이 된다.
    #[test]
    fn every_routing_entry_gates_before_it_answers() {
        /// 게이트로 인정하는 호출. `gates_before_routing` 은 이 파일의 진입 게이트,
        /// 나머지 둘은 라우터 안쪽과 app 단 caller 게이트가 쓰는 이름이다.
        const GATES: &[&str] = &[
            "gates_before_routing",
            "check_permission_gate",
            "ensure_allowed",
        ];
        /// 답하기 전에 게이트가 **없어도 되는** 함수와 그 근거. 근거는 이 테스트가
        /// 다시 검사한다 — 전제가 사라지면 면제도 같이 깨져야 하기 때문이다.
        /// 게이트 호출과 그 반환 사이에 허용하는 거리(바이트). rustfmt 가 만드는
        /// `if let Some(resp) = ... {\n    return resp;\n}` 는 60 바이트 안쪽이다.
        /// 넘으면 사이에 무언가 끼어든 것이므로 사람이 한 번 본다.
        const RETURN_WINDOW: usize = 120;
        const EXEMPT: &[(&str, &str, &str)] = &[(
            "ipc_step_routing",
            "caller_gate.rs 의 step 1 이 Local 이 아닌 caller 를 이미 ensure_allowed 로 거른다",
            "src/app/ipc/caller_gate.rs",
        )];
        /// 면제 전제를 확인할 때 찾는 문자열. 이름만(`ensure_allowed`) 찾으면 그
        /// 파일의 **모듈 doc** 에도 같은 이름이 있어, 실제 호출을 지워도 주석이 남아
        /// 전제가 살아 있는 것처럼 보인다(변이로 확인했다). 수신자까지 붙여 호출
        /// 형태로 찾는다.
        const PRECONDITION_CALL: &str = "caller.ensure_allowed(";

        let sources = [
            ("src/app/dispatch/intents.rs", include_str!("intents.rs")),
            ("src/app/ipc/routing.rs", include_str!("../ipc/routing.rs")),
        ];
        let mut checked = 0usize;
        let mut naked = Vec::new();
        for (file, full) in sources {
            // 프로덕션 본문만 본다. 테스트 모듈에는 이 가드 자신이 있고, 그 본문의
            // 문자열 리터럴에 마커가 그대로 들어 있어 스스로를 대상으로 오인한다.
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

                let routes =
                    body.contains("dispatch_list_global(") || body.contains("handle_with_caller(");
                if !routes || !body.contains("caller: &") {
                    continue;
                }
                checked += 1;
                if let Some((_, why, precondition)) =
                    EXEMPT.iter().find(|(n, _, _)| *n == name.as_str())
                {
                    let pre = std::fs::read_to_string(
                        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(precondition),
                    )
                    .unwrap_or_else(|e| panic!("면제 전제 `{precondition}` 를 못 읽었다: {e}"));
                    assert!(
                        pre.contains(PRECONDITION_CALL),
                        "`{name}` 의 면제 근거가 사라졌다 — {why}. `{precondition}` 에 \
                         ensure_allowed 가 더는 없다."
                    );
                    continue;
                }
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
                    (Some(_), None) => {
                        naked.push(format!("  {file}::{name} — 게이트 결과를 반환하지 않는다"))
                    }
                    _ => {}
                }
            }
        }
        assert!(
            checked >= 2,
            "라우팅 진입점을 {checked}개밖에 못 찾았다 — 파서가 낡았다. 0 은 통과가 \
             아니라 측정 실패다."
        );
        assert!(
            naked.is_empty(),
            "요청에 답하기 전에 게이트가 돌지 않는다 ({}건):\n{}\n\n\
             게이트 안쪽까지 못 가고 끝나는 응답은 권한·telemetry cap·rate limit 을 \
             통째로 건너뛰고 audit 에도 남지 않는다. 조기 응답을 추가하려면 게이트 \
             호출 **뒤로** 놓아라. 앞서야 할 이유가 있으면 EXEMPT 에 이름과 근거, \
             그리고 이 테스트가 다시 검사할 수 있는 전제 파일을 적는다.",
            naked.len(),
            naked.join("\n")
        );
    }
}
