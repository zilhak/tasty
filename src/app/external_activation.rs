//! 같은 홈으로 다시 실행한 두 번째 프로세스의 활성화 요청을 처리한다.
//!
//! 증거가 있는 요청만 창 상태를 바꾼다. 숨기거나 최소화한 MainView를 다시 보이고, 사용자가 마지막으로
//! 쓰던 MainView의 활성화를 OS에 요청한다. 포커스 이동 여부는 OS가 정하며 Tasty는 `focus_window()`를
//! 부르지 않는다(원칙 3). MainView가 없으면 토큰을 실어 새 창 하나를 만든다. 만드는 중인 창이 있으면
//! 새로 만들지 않고 그 창이 등록될 때 활성화를 요청한다.
//!
//! Wayland에서는 이미 떠 있는 창을 외부 토큰으로 활성화할 수 없다(winit에 API가 없다). 그래서 증거가
//! 있어도 기존 창을 건드리지 않고 토큰을 실은 새 창을 연다. 기존 창 활성화 경로는
//! [`can_raise_existing_view`]가 참을 돌려주는 백엔드에서만 쓴다.

use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use crate::app::App;
use crate::app::event::WindowRequestOrigin;
use crate::boot::single_instance::ExternalActivation;

/// 외부 활성화 요청으로 할 일.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ActivationPlan<W> {
    /// 증거가 없다. 아무것도 바꾸지 않는다.
    Ignore,
    /// MainView가 없다. 새 창을 연다.
    OpenWindow,
    /// 트레이 복원처럼 모든 View를 다시 보이고 `target`의 활성화를 OS에 요청한다.
    Restore { views: Vec<W>, target: W },
}

/// `views`는 (창, MainView 여부) 목록이다. 대상은 마지막으로 포커스를 가졌던 View이고, 그 값이 없거나
/// 이미 닫힌 창이면 가장 작은 id의 MainView다. `raise_existing`이 거짓인 백엔드에서는 창이 있어도 새 창을
/// 연다. 기존 창을 다시 보이기만 하고 앞으로 가져오지 못하면 사용자에게는 무반응으로 보이기 때문이다.
pub(crate) fn plan_activation<W: Copy + Ord>(
    has_evidence: bool,
    raise_existing: bool,
    views: &[(W, bool)],
    focused: Option<W>,
) -> ActivationPlan<W> {
    if !has_evidence {
        return ActivationPlan::Ignore;
    }
    if !raise_existing {
        return ActivationPlan::OpenWindow;
    }
    let Some(first_main) = views
        .iter()
        .filter(|(_, main)| *main)
        .map(|(id, _)| *id)
        .min()
    else {
        return ActivationPlan::OpenWindow;
    };
    let target = focused
        .filter(|f| views.iter().any(|(id, _)| id == f))
        .unwrap_or(first_main);
    let mut all: Vec<W> = views.iter().map(|(id, _)| *id).collect();
    all.sort();
    ActivationPlan::Restore { views: all, target }
}

/// 이 백엔드가 외부 토큰으로 이미 떠 있는 창을 활성화할 수 있는지. Wayland는 winit 포크에 그 API가
/// 생길 때까지 거짓이다.
fn can_raise_existing_view(event_loop: &ActiveEventLoop) -> bool {
    !crate::boot::single_instance::is_wayland(event_loop)
}

impl App {
    pub(crate) fn handle_external_activation(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: ExternalActivation,
    ) {
        let views: Vec<(WindowId, bool)> = self
            .view
            .views
            .iter()
            .map(|(id, view)| (*id, view.as_main().is_some()))
            .collect();
        let raise_existing = can_raise_existing_view(event_loop);
        match plan_activation(
            request.has_evidence(),
            raise_existing,
            &views,
            self.view.focused_view_id,
        ) {
            ActivationPlan::Ignore => {
                tracing::info!("external activation without launch evidence: nothing changed");
            }
            ActivationPlan::OpenWindow => {
                self.open_activated_window(event_loop, request, raise_existing);
            }
            ActivationPlan::Restore { views, target } => {
                // 트레이 복원과 같은 방식이되 focus_window()는 부르지 않는다. 포커스는 OS가 정한다.
                for id in &views {
                    if let Some(view) = self.view.views.get(id) {
                        let window = &view.base().winit;
                        window.set_visible(true);
                        window.set_minimized(false);
                    }
                }
                tracing::info!(
                    "external activation: restored {} view(s), requesting OS activation of {target:?}",
                    views.len()
                );
                self.request_os_activation(target, &request);
            }
        }
    }

    /// 토큰을 실은 새 창을 만든다. 등록 뒤의 OS 활성화 요청은 기존 창을 활성화할 수 있는 백엔드에서만
    /// 건다. Wayland에서는 창을 만들 때 실은 토큰이 그 역할을 한다.
    fn open_activated_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        request: ExternalActivation,
        raise_existing: bool,
    ) {
        if self.pending_window.is_some() {
            tracing::info!(
                "external activation: a window is being created; activating it when ready"
            );
            if raise_existing {
                self.pending_external_activation = Some(request);
            }
            return;
        }
        self.next_window_activation = Some(request.evidence.clone());
        if raise_existing {
            self.pending_external_activation = Some(request);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_without_evidence_changes_nothing() {
        assert_eq!(
            plan_activation(false, true, &[(1, true)], Some(1)),
            ActivationPlan::Ignore
        );
        assert_eq!(
            plan_activation::<u32>(false, true, &[], None),
            ActivationPlan::Ignore
        );
    }

    #[test]
    fn no_main_view_opens_a_window() {
        assert_eq!(
            plan_activation::<u32>(true, true, &[], None),
            ActivationPlan::OpenWindow
        );
        // 모달 View만 남은 경우도 MainView가 없는 것으로 본다.
        assert_eq!(
            plan_activation(true, true, &[(5, false)], Some(5)),
            ActivationPlan::OpenWindow
        );
    }

    #[test]
    fn the_last_focused_hidden_view_wins_over_an_agent_window() {
        // 1 = 트레이로 숨긴 마지막 포커스 View, 2 = 그 뒤 에이전트가 만든 보이는 창(내부 포커스 불변).
        assert_eq!(
            plan_activation(true, true, &[(2, true), (1, true)], Some(1)),
            ActivationPlan::Restore {
                views: vec![1, 2],
                target: 1
            }
        );
    }

    #[test]
    fn a_focused_modal_view_is_the_target_and_is_restored_with_the_main_views() {
        assert_eq!(
            plan_activation(true, true, &[(1, true), (9, false)], Some(9)),
            ActivationPlan::Restore {
                views: vec![1, 9],
                target: 9
            }
        );
    }

    #[test]
    fn a_stale_or_missing_focus_falls_back_to_the_lowest_main_view() {
        let expected = ActivationPlan::Restore {
            views: vec![3, 4, 8],
            target: 3,
        };
        assert_eq!(
            plan_activation(true, true, &[(4, true), (8, false), (3, true)], Some(7)),
            expected
        );
        assert_eq!(
            plan_activation(true, true, &[(4, true), (8, false), (3, true)], None),
            expected
        );
    }

    #[test]
    fn a_backend_that_cannot_raise_an_existing_view_always_opens_a_new_one() {
        // Wayland: 증거가 있고 MainView가 있어도 숨김·최소화 창을 건드리지 않고 새 창을 연다.
        assert_eq!(
            plan_activation(true, false, &[(2, true), (1, true)], Some(1)),
            ActivationPlan::OpenWindow
        );
        assert_eq!(
            plan_activation::<u32>(true, false, &[], None),
            ActivationPlan::OpenWindow
        );
        // 증거가 없으면 백엔드와 무관하게 아무것도 바꾸지 않는다.
        assert_eq!(
            plan_activation(false, false, &[(1, true)], Some(1)),
            ActivationPlan::Ignore
        );
    }
}
