//! 실행 중인 파일 작업과 explorer 칸 표시를 잇는다. 진행을 다시 그리고, 충돌 질문을 띄우고,
//! 결과를 카드로 넘긴다.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::job::{Report, Shared};
use super::{OpKind, Operation};
use crate::adapters::ui::popup::PopupScope;
use crate::adapters::ui::popup::explorer_conflict::EXPLORER_CONFLICT_POPUP_ID;
use crate::explorer_ui::view::ops::{Running, result_is_timed};
use crate::state::MainViewState;
use crate::view::ui::View;

/// 충돌을 물을 수 있는 작업 상태. 진행이 바뀌면 이벤트 루프를 깨운다.
pub(super) fn interactive_shared(
    proxy: winit::event_loop::EventLoopProxy<crate::AppEvent>,
) -> Shared {
    // proxy 를 여러 스레드가 함께 쓰도록 잠금 안에 둔다.
    let proxy = Mutex::new(proxy);
    Shared::interactive(move || {
        let sent = proxy
            .lock()
            .map(|p| p.send_event(crate::AppEvent::TimerTick).is_ok());
        if !matches!(sent, Ok(true)) {
            tracing::debug!("Explorer file progress after event loop closed");
        }
    })
}

/// 작업을 요청한 칸에 진행 표시를 붙인다.
pub(super) fn show_running(
    state: &mut MainViewState,
    surface: u32,
    operation: &Operation,
    shared: &Arc<Shared>,
    in_history: bool,
) {
    let Some(kind) = operation.kind() else {
        return;
    };
    let Some(view) = state.explorer_views.get_mut(surface) else {
        return;
    };
    let (dest, total) = operation.summary();
    view.ops.running = Some(Running {
        shared: Arc::clone(shared),
        kind,
        dest,
        total,
        in_history,
    });
}

/// 끝난 작업의 결과 카드를 요청한 칸에 둔다. 모두 끝났거나 취소한 결과만 `lifetime` 뒤 사라진다.
/// 한 누름으로 묶인 작업이면 묶음의 작업이 모두 끝났을 때 합친 카드 하나를 둔다.
pub(super) fn push_result(
    state: &mut MainViewState,
    surface: u32,
    (report, press): (Report, Option<(u64, u64)>),
    undo_of: Option<OpKind>,
    lifetime: Duration,
) {
    let Some(view) = state.explorer_views.get_mut(surface) else {
        return;
    };
    let report = match press {
        Some((press, request)) => view.ops.arrive(press, request, report),
        None => Some(report),
    };
    if let Some(report) = report {
        let expires = result_is_timed(&report).then(|| Instant::now() + lifetime);
        view.ops.push_result(report, undo_of, expires);
    }
}

/// 묶음의 요청이 시작하지 않고 빠졌다. 먼저 끝난 결과만 남았으면 그 카드를 둔다.
pub(crate) fn forget_press_request(state: &mut MainViewState, surface: u32, request: u64) {
    if let Some(view) = state.explorer_views.get_mut(surface) {
        view.ops.forget_request(request);
    }
}

/// 결과 없이 끝난 작업(worker panic 등)을 알린다. 한 누름으로 묶인 요청이면 묶음에서 빼서
/// 먼저 끝난 결과의 카드가 남지 않고 나오게 한다.
pub(super) fn push_failure(
    state: &mut MainViewState,
    surface: u32,
    press: Option<(u64, u64)>,
    failure: Option<String>,
) {
    if let Some((_, request)) = press {
        forget_press_request(state, surface, request);
    }
    if let Some(error) = failure {
        state.toasts.push(
            crate::i18n::t_fmt("explorer.state.operation_failed", &error),
            crate::adapters::ui::ToastKind::Error,
            crate::adapters::ui::ToastScope::Surface(surface),
        );
    }
}

impl crate::app::App {
    /// 끝난 작업의 진행 표시를 모든 칸에서 거둔다. 요청한 칸이 다른 윈도우로 옮겨 갔어도 남지 않는다.
    pub(super) fn clear_explorer_running(&mut self, shared: &Arc<Shared>) {
        for view in self.view.views.values_mut() {
            let Some(view) = view.as_main_mut() else {
                continue;
            };
            let mut changed = false;
            for sid in view
                .state
                .explorer_views
                .cached_surfaces()
                .collect::<Vec<_>>()
            {
                let Some(ev) = view.state.explorer_views.get_mut(sid) else {
                    continue;
                };
                if ev
                    .ops
                    .running
                    .as_ref()
                    .is_some_and(|r| Arc::ptr_eq(&r.shared, shared))
                {
                    ev.ops.running = None;
                    changed = true;
                }
            }
            if changed {
                view.mark_dirty();
            }
        }
    }

    /// 대기열 표시를 맞추고, 진행이 바뀐 칸을 다시 그리고, 포커스된 칸의 충돌 질문을 띄운다.
    pub(super) fn sync_explorer_ops(&mut self) {
        for view in self.view.views.values_mut() {
            let Some(view) = view.as_main_mut() else {
                continue;
            };
            if sync_queues(&mut view.state) {
                view.mark_dirty();
            }
        }
        let Some(job) = self.explorer_files.job.as_mut() else {
            return;
        };
        let snap = job.shared.snapshot();
        let asking = job.shared.pending_ask().is_some();
        let seen = (snap.items_done, snap.bytes_done, asking);
        let changed = seen != job.seen;
        job.seen = seen;
        let (window, surface, engine) = (job.window, job.target.surface, job.engine);
        let view = self
            .view
            .views
            .get_mut(&window)
            .and_then(|v| v.as_main_mut());
        // 질문을 띄울 칸이 사라졌으면 답할 곳이 없으므로 남은 항목을 멈춘다.
        let Some(view) = view.filter(|v| v.state.explorer_views.get(surface).is_some()) else {
            if asking {
                job.shared.cancel();
            }
            return;
        };
        if changed {
            view.mark_dirty();
        }
        if !asking || view.state.popups.is_open(EXPLORER_CONFLICT_POPUP_ID) {
            return;
        }
        let Some(session) = self.engines.get(engine) else {
            return;
        };
        if view.state.focused_surface_id(&session.read()) == Some(surface) {
            open_conflict(&mut view.state, surface, "explorer_conflict_focus");
            view.mark_dirty();
        }
    }
}

/// 칸마다 기다리는 작업 목록을 요청 대기열과 맞춘다. 바뀐 칸이 있으면 true.
fn sync_queues(state: &mut MainViewState) -> bool {
    let mut changed = false;
    for sid in state.explorer_views.cached_surfaces().collect::<Vec<_>>() {
        let queued = state.explorer_file_requests.queued_for(sid);
        let Some(view) = state.explorer_views.get_mut(sid) else {
            continue;
        };
        let same = view.ops.queued.len() == queued.len()
            && view
                .ops
                .queued
                .iter()
                .zip(&queued)
                .all(|(a, b)| a.id == b.id);
        if !same {
            view.ops.queued = queued;
            changed = true;
        }
    }
    changed
}

/// 충돌 질문을 그 칸에 띄운다.
pub(crate) fn open_conflict(state: &mut MainViewState, surface: u32, source: &'static str) {
    state.dispatch_intent(
        crate::intent::UiIntent::OpenPopup {
            id: EXPLORER_CONFLICT_POPUP_ID,
            mode: crate::intent::OpenPopupMode::WithScope(PopupScope::Surface(surface)),
        }
        .from_user_menu(source),
    );
}
