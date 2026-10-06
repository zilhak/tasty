//! 같은 홈으로 다시 실행한 두 번째 프로세스의 활성화 요청을 처리한다.
//!
//! 증거가 있는 요청만 창 상태를 바꾼다. 숨기거나 최소화한 MainView를 다시 보이고, 사용자가 마지막으로
//! 쓰던 MainView의 활성화를 OS에 요청한다. 포커스 이동 여부는 OS가 정하며 Tasty는 `focus_window()`를
//! 부르지 않는다(원칙 3). MainView가 없으면 토큰을 실어 새 창 하나를 만든다. 만드는 중인 창이 있으면
//! 새로 만들지 않고 그 창이 등록될 때 활성화를 요청한다.

use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::app::App;
use crate::app::event::WindowRequestOrigin;
use crate::boot::single_instance::ExternalActivation;

impl App {
    pub(crate) fn handle_external_activation(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: ExternalActivation,
    ) {
        if !request.has_evidence() {
            tracing::info!("external activation without launch evidence: nothing changed");
            return;
        }
        let mains: Vec<WindowId> = self
            .view
            .views
            .iter()
            .filter(|(_, view)| view.as_main().is_some())
            .map(|(id, _)| *id)
            .collect();
        if mains.is_empty() {
            self.open_activated_window(event_loop, request);
            return;
        }
        for id in &mains {
            if let Some(view) = self.view.views.get(id) {
                let window = &view.base().winit;
                window.set_visible(true);
                window.set_minimized(false);
            }
        }
        let target = self
            .view
            .focused_view_id
            .filter(|id| mains.contains(id))
            .unwrap_or(mains[0]);
        tracing::info!(
            "external activation: restored {} main view(s), requesting OS activation of {target:?}",
            mains.len()
        );
        self.request_os_activation(target, &request);
    }

    fn open_activated_window(&mut self, event_loop: &ActiveEventLoop, request: ExternalActivation) {
        if self.pending_window.is_some() {
            tracing::info!(
                "external activation: a window is being created; activating it when ready"
            );
            self.pending_external_activation = Some(request);
            return;
        }
        self.next_window_activation = Some(request.evidence.clone());
        self.pending_external_activation = Some(request);
        let outcome = self.create_new_window(event_loop, WindowRequestOrigin::User);
        if outcome.is_err() {
            self.next_window_activation = None;
            self.pending_external_activation = None;
        }
        tracing::info!("external activation: opening a new window: {outcome:?}");
    }

    /// 만드는 중이던 창이 등록되면 걸어 둔 외부 활성화를 그 창에 적용한다.
    pub(crate) fn apply_pending_external_activation(&mut self, window_id: WindowId) {
        if let Some(request) = self.pending_external_activation.take() {
            self.request_os_activation(window_id, &request);
        }
    }

    fn request_os_activation(&self, id: WindowId, request: &ExternalActivation) {
        let Some(view) = self.view.views.get(&id) else {
            return;
        };
        let evidence = request.platform_evidence();
        match crate::platform::window_activation::request_activation(&view.base().winit, &evidence)
        {
            Ok(kind) => tracing::info!("external activation: requested {kind:?} for {id:?}"),
            Err(e) => tracing::warn!("external activation: OS activation not requested: {e}"),
        }
    }
}
