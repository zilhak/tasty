//! 실행 중인 파일 작업과 explorer 칸 표시를 잇는다. 진행을 다시 그리고, 충돌 질문을 띄우고,
//! 결과를 카드로 넘긴다.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::Operation;
use super::history::UndoOf;
use super::job::{Report, Shared, Snapshot};
use crate::adapters::ui::popup::PopupScope;
use crate::adapters::ui::popup::explorer_conflict::EXPLORER_CONFLICT_POPUP_ID;
use crate::explorer_ui::view::ops::{Running, result_is_timed};
use crate::state::MainViewState;
use crate::view::ui::View;

/// Redraw whenever any displayed progress value or conflict state changes.
#[derive(Default)]
pub(super) struct ProgressSeen(Option<(Snapshot, bool)>);

impl ProgressSeen {
    fn update(&mut self, snapshot: Snapshot, asking: bool) -> bool {
        let now = Some((snapshot, asking));
        let changed = self.0 != now;
        self.0 = now;
        changed
    }
}

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
    let Some(view) = state.explorer_views.get_mut(surface) else {
        return;
    };
    let Some(kind) = operation.kind() else {
        // 진행 표시는 없지만 끝나기 전 되돌리기가 그 앞 단계를 꺼내지 않게 이력을 바쁜 것으로 둔다.
        if in_history {
            view.ops.quiet_step = Some(Arc::clone(shared));
        }
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
    undo_of: Option<UndoOf>,
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

/// 연결이 끊겨 보내지 못하고 버린 원격 요청 수(칸별). 서버에 닿지 않았으므로 아무것도 바뀌지 않았다.
#[derive(Default)]
pub(super) struct LostRequests(Vec<(u32, usize)>);

impl LostRequests {
    pub(super) fn add(&mut self, surface: u32) {
        match self.0.iter_mut().find(|(s, _)| *s == surface) {
            Some((_, n)) => *n += 1,
            None => self.0.push((surface, 1)),
        }
    }
    /// 칸마다 원격 실패와 같은 토스트로 알린다.
    pub(super) fn notify(self, state: &mut MainViewState) {
        for (surface, n) in self.0 {
            let text = crate::i18n::t_count(
                "explorer.state.remote_queue_dropped",
                n as u64,
                &[&n.to_string()],
            );
            push_failure(state, surface, None, Some(text));
        }
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
                if ev
                    .ops
                    .quiet_step
                    .as_ref()
                    .is_some_and(|s| Arc::ptr_eq(s, shared))
                {
                    ev.ops.quiet_step = None;
                }
            }
            if changed {
                view.mark_dirty();
            }
        }
    }

    /// 대기열 표시를 맞추고, 진행이 바뀐 칸을 다시 그리고, 포커스된 칸의 충돌 질문을 띄운다.
    pub(super) fn sync_explorer_ops(&mut self) {
        let files = &mut self.explorer_files;
        let targets: Vec<&super::Target> = files
            .job
            .iter()
            .map(|j| &j.target)
            .chain(files.remote.iter().map(|j| &j.target))
            .collect();
        let running: Vec<(u32, &std::path::Path)> = targets
            .iter()
            .filter_map(|t| Some((t.surface, t.created.as_deref()?)))
            .collect();
        let running_requests: Vec<u64> = targets.iter().map(|t| t.request).collect();
        for view in self.view.views.values_mut() {
            let Some(view) = view.as_main_mut() else {
                continue;
            };
            let resumed = resume_unanswered_creates(&mut view.state, &running)
                | resume_unanswered_rename(&mut view.state, &running_requests)
                | resume_unanswered_permissions(&mut view.state, &running_requests);
            if sync_queues(&mut view.state) || resumed {
                view.mark_dirty();
            }
        }
        let local = files.job.iter_mut().map(|j| {
            (
                (j.window, j.target.surface, j.engine),
                &j.shared,
                &mut j.seen,
            )
        });
        let remote = files.remote.iter_mut().map(|j| {
            (
                (j.window, j.target.surface, j.engine),
                &j.shared,
                &mut j.seen,
            )
        });
        for (place, shared, seen) in local.chain(remote) {
            sync_job(&mut self.view, &self.engines, place, shared, seen);
        }
    }
}

/// 실행 중인 작업 하나의 진행을 다시 그리고, 포커스된 칸이면 충돌 질문을 띄운다.
fn sync_job(
    registry: &mut crate::view::ViewRegistry,
    engines: &crate::app::engine_registry::EngineRegistry,
    (window, surface, engine): (
        winit::window::WindowId,
        u32,
        crate::runtime::engine_session::EngineId,
    ),
    shared: &Arc<Shared>,
    seen: &mut ProgressSeen,
) {
    let snap = shared.snapshot();
    let asking = shared.pending_ask().is_some();
    let changed = seen.update(snap, asking);
    let view = registry
        .views
        .get_mut(&window)
        .and_then(|v| v.as_main_mut());
    // 질문을 띄울 칸이 사라졌으면 답할 곳이 없으므로 남은 항목을 멈춘다.
    let Some(view) = view.filter(|v| v.state.explorer_views.get(surface).is_some()) else {
        if asking {
            shared.cancel();
        }
        return;
    };
    if changed {
        view.mark_dirty();
    }
    if !asking || view.state.popups.is_open(EXPLORER_CONFLICT_POPUP_ID) {
        return;
    }
    let Some(session) = engines.get(engine) else {
        return;
    };
    if view.state.focused_surface_id(&session.read()) == Some(surface) {
        open_conflict(&mut view.state, surface, "explorer_conflict_focus");
        view.mark_dirty();
    }
}

/// 칸마다 기다리는 작업 목록을 요청 대기열과 맞춘다. 바뀐 칸이 있으면 true.
/// 원격 만들기를 기다리는 편집 줄 가운데 대기열에도 `running` 에도 없는 것은 성공으로 닫히지 않고
/// 끝났다(실패·거절·연결 끊김·버려진 요청). 글자를 둔 채 다시 편집하게 한다. 실패 이유는 그 경로가
/// 따로 알린다. `running` 은 실행 중인 작업의 (칸, 만들 경로)다.
pub(super) fn resume_unanswered_creates(
    state: &mut MainViewState,
    running: &[(u32, &std::path::Path)],
) -> bool {
    let mut changed = false;
    for sid in state.explorer_views.cached_surfaces().collect::<Vec<_>>() {
        let Some(path) = state
            .explorer_views
            .get(sid)
            .and_then(|v| v.create.as_ref()?.waiting_path())
        else {
            continue;
        };
        let alive = |surface: u32, created: Option<&std::path::Path>| {
            surface == sid && created == Some(path.as_path())
        };
        let queued = state
            .explorer_file_requests
            .0
            .iter()
            .any(|r| alive(r.target.surface, r.target.created.as_deref()));
        if queued || running.iter().any(|&(s, p)| alive(s, Some(p))) {
            continue;
        }
        if let Some(create) = state
            .explorer_views
            .get_mut(sid)
            .and_then(|v| v.create.as_mut())
        {
            create.resume_editing();
            changed = true;
        }
    }
    changed
}

/// 이름 변경 팝업이 기다리는 요청이 대기열에도 `running` 에도 없고 성공으로 끝나지 않았으면
/// 기다림을 풀어 다시 편집하게 한다. 실패 이유는 그 칸이 따로 알린다.
pub(super) fn resume_unanswered_rename(state: &mut MainViewState, running: &[u64]) -> bool {
    let Some(waiting) = state.dialogs.rename_waiting.as_ref().filter(|w| !w.done) else {
        return false;
    };
    let id = waiting.request;
    if running.contains(&id) || state.explorer_file_requests.0.iter().any(|r| r.id == id) {
        return false;
    }
    state.dialogs.rename_waiting = None;
    true
}

/// Properties 권한 편집이 기다리는 요청이 대기열에도 `running` 에도 없으면 시작되지 않고 버려졌다.
/// 편집 상태로 돌려 다시 Apply 할 수 있게 한다. 끝난 요청은 [`settle_permissions`] 가 먼저 풀어 둔다.
pub(super) fn resume_unanswered_permissions(state: &mut MainViewState, running: &[u64]) -> bool {
    let Some(props) = state.dialogs.explorer_properties.as_mut() else {
        return false;
    };
    let Some(id) = props.perm_request() else {
        return false;
    };
    if running.contains(&id) || state.explorer_file_requests.0.iter().any(|r| r.id == id) {
        return false;
    }
    props.settle_perm(id, None);
    true
}

/// 권한 변경이 끝났으면 그 요청을 기다리던 Properties 편집 상자에 결과를 알린다. 받은 상자가 있으면 true.
pub(super) fn settle_permissions(
    registry: &mut crate::view::ViewRegistry,
    request: u64,
    result: Result<(), String>,
) -> bool {
    let mut settled = false;
    for view in registry.views.values_mut() {
        let Some(view) = view.as_main_mut() else {
            continue;
        };
        if let Some(props) = view.state.dialogs.explorer_properties.as_mut()
            && props.perm_request() == Some(request)
        {
            props.settle_perm(request, Some(result.clone()));
            view.mark_dirty();
            settled = true;
        }
    }
    settled
}

/// 원격이 이름 변경에 성공했으면 그 요청을 기다리던 팝업을 닫게 한다.
pub(super) fn close_renamed(registry: &mut crate::view::ViewRegistry, request: u64) {
    for view in registry.views.values_mut() {
        let Some(view) = view.as_main_mut() else {
            continue;
        };
        if let Some(waiting) = view.state.dialogs.rename_waiting.as_mut()
            && waiting.request == request
        {
            waiting.done = true;
            view.mark_dirty();
        }
    }
}

/// 원격이 만들기에 성공했으면 그 경로를 기다리던 편집 줄을 닫는다. 항목은 다시 읽은 목록에서 고른다.
pub(super) fn close_created(
    registry: &mut crate::view::ViewRegistry,
    surface: u32,
    path: &std::path::Path,
) {
    for view in registry.views.values_mut() {
        let Some(view) = view.as_main_mut() else {
            continue;
        };
        if let Some(v) = view.state.explorer_views.get_mut(surface)
            && v.create.as_ref().and_then(|c| c.waiting_path()).as_deref() == Some(path)
        {
            v.create = None;
            view.mark_dirty();
        }
    }
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::file_jobs::job::{Choice, FileProgress};

    #[test]
    fn counting_and_zero_byte_files_redraw_without_top_level_or_byte_changes() {
        let mut seen = ProgressSeen::default();
        let mut snap = Shared::fixed(Choice::KeepBoth).snapshot();
        assert!(seen.update(snap.clone(), false));
        assert!(!seen.update(snap.clone(), false));
        snap.files = Some(FileProgress {
            done: 0,
            total: 0,
            counting: true,
        });
        assert!(seen.update(snap.clone(), false));
        snap.files.as_mut().unwrap().total = 42;
        assert!(seen.update(snap.clone(), false));
        snap.files.as_mut().unwrap().counting = false;
        assert!(seen.update(snap.clone(), false));
        snap.files.as_mut().unwrap().done = 1;
        snap.current = "empty.txt".into();
        assert!(seen.update(snap.clone(), false));
        assert!(!seen.update(snap.clone(), false));
        assert!(seen.update(snap, true));
    }
}
