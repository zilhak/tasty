//! Bounded local Explorer actions. Views queue fixed inputs; App owns execution and joins.
pub(crate) mod job;
mod ops;
mod ui_sync;
pub(crate) use ui_sync::open_conflict;
use ui_sync::{push_result, show_running};

use job::leftover::Leftover;
use job::{OpKind, Report, Shared, UndoStep};

use crate::runtime::surface_binding::SurfaceBinding;
use crate::view::ui::View;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};
use std::thread::JoinHandle;

const MAX_PENDING_PER_VIEW: usize = 8;
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Debug)]
pub(crate) enum Operation {
    Paste {
        paths: Vec<PathBuf>,
        destination: PathBuf,
        cut: bool,
    },
    Trash(Vec<PathBuf>),
    Rename {
        path: PathBuf,
        name: String,
    },
    Open(PathBuf),
    /// 새 폴더나 빈 파일. 같은 이름이 있으면 덮어쓰지 않고 실패한다.
    Create {
        dir: PathBuf,
        name: String,
        folder: bool,
    },
    /// 끝난 복사·이동을 되돌린다.
    Undo(Vec<UndoStep>),
    /// 원본이 남은 이동 항목의 원본만 다시 지운다. `dest` 는 처음 이동의 목적지 폴더다.
    RemoveLeftovers {
        dest: Option<PathBuf>,
        leftovers: Vec<Leftover>,
    },
}

/// worker 가 돌려주는 결과. 복사·이동·휴지통·되돌리기는 항목별 결과를 남긴다.
#[derive(Debug)]
pub(crate) enum Done {
    Report(Report),
    Simple(Result<(), String>),
}
impl Done {
    fn success(&self) -> bool {
        match self {
            Self::Report(r) => r.failed.is_empty() && r.skipped.is_empty() && !r.cancelled,
            Self::Simple(r) => r.is_ok(),
        }
    }
    /// 실패를 한 줄로 요약한다. 실패가 없으면 None.
    fn error_summary(&self) -> Option<String> {
        match self {
            Self::Report(r) => r.failed.first().map(|f| {
                format!(
                    "{} succeeded, {} failed: {}: {:?}",
                    r.done,
                    r.failed.len(),
                    f.path.display(),
                    f.reason
                )
            }),
            Self::Simple(r) => r.as_ref().err().cloned(),
        }
    }
}
impl Operation {
    /// 진행·결과를 항목 단위로 보이는 작업의 종류. 이름 변경·열기·새 항목은 None.
    pub(crate) fn kind(&self) -> Option<OpKind> {
        match self {
            Self::Paste { cut: false, .. } => Some(OpKind::Copy),
            Self::Paste { cut: true, .. } | Self::RemoveLeftovers { .. } => Some(OpKind::Move),
            Self::Trash(_) => Some(OpKind::Trash),
            Self::Undo(_) => Some(OpKind::Undo),
            Self::Rename { .. } | Self::Open(_) | Self::Create { .. } => None,
        }
    }
    /// 진행 표시에 쓰는 목적지 폴더와 항목 수.
    fn summary(&self) -> (Option<PathBuf>, usize) {
        match self {
            Self::Paste {
                paths, destination, ..
            } => (Some(destination.clone()), paths.len()),
            Self::Trash(paths) => (None, paths.len()),
            Self::Undo(steps) => (None, steps.len()),
            Self::RemoveLeftovers { dest, leftovers } => (dest.clone(), leftovers.len()),
            Self::Rename { .. } | Self::Open(_) | Self::Create { .. } => (None, 1),
        }
    }
    /// 되돌리기라면 되돌리는 작업의 종류. 옮긴 항목이 하나라도 있으면 이동이다.
    fn undo_of(&self) -> Option<OpKind> {
        let Self::Undo(steps) = self else {
            return None;
        };
        Some(
            if steps.iter().any(|s| matches!(s, UndoStep::Moved { .. })) {
                OpKind::Move
            } else {
                OpKind::Copy
            },
        )
    }
    fn bytes(&self) -> usize {
        let paths = |paths: &[PathBuf]| {
            paths
                .iter()
                .map(|p| p.as_os_str().len() + std::mem::size_of::<PathBuf>())
                .sum::<usize>()
        };
        match self {
            Self::Paste {
                paths: items,
                destination,
                ..
            } => paths(items) + destination.as_os_str().len(),
            Self::Trash(items) => paths(items),
            Self::RemoveLeftovers { dest, leftovers } => {
                dest.as_ref().map_or(0, |d| d.as_os_str().len())
                    + leftovers
                        .iter()
                        .map(|l| {
                            l.source.as_os_str().len()
                                + l.copy.as_os_str().len()
                                + std::mem::size_of::<Leftover>()
                        })
                        .sum::<usize>()
            }
            Self::Rename { path, name } => path.as_os_str().len() + name.len(),
            Self::Open(path) => path.as_os_str().len(),
            Self::Create { dir, name, .. } => dir.as_os_str().len() + name.len(),
            Self::Undo(steps) => steps
                .iter()
                .map(|s| match s {
                    UndoStep::Created(p, _) | UndoStep::Replaced(p) => p.as_os_str().len(),
                    UndoStep::Moved { from, to } => from.as_os_str().len() + to.as_os_str().len(),
                } + std::mem::size_of::<UndoStep>())
                .sum(),
        }
    }
    /// 작업이 바꿀 수 있는 경로. 결과와 관계없이 이 경로를 보는 목록을 다시 읽는다.
    /// 앞은 항목이 생기거나 사라질 수 있는 폴더, 뒤는 원래 자리에서 사라질 수 있는 경로다.
    fn affected(&self) -> Affected {
        let parents = |paths: &[PathBuf]| -> Vec<PathBuf> {
            paths
                .iter()
                .filter_map(|p| p.parent().map(PathBuf::from))
                .collect()
        };
        match self {
            Self::Paste {
                paths,
                destination,
                cut,
            } => {
                let mut changed = vec![destination.clone()];
                if *cut {
                    changed.extend(parents(paths));
                }
                let removed = if *cut { paths.clone() } else { Vec::new() };
                Affected { changed, removed }
            }
            Self::Trash(paths) => Affected {
                changed: parents(paths),
                removed: paths.clone(),
            },
            Self::RemoveLeftovers { leftovers, .. } => {
                let sources: Vec<PathBuf> = leftovers.iter().map(|l| l.source.clone()).collect();
                Affected {
                    changed: parents(&sources),
                    removed: sources,
                }
            }
            Self::Rename { path, .. } => Affected {
                changed: parents(std::slice::from_ref(path)),
                removed: vec![path.clone()],
            },
            Self::Open(_) => Affected::default(),
            Self::Create { dir, .. } => Affected {
                changed: vec![dir.clone()],
                removed: Vec::new(),
            },
            Self::Undo(steps) => {
                let mut changed = Vec::new();
                let mut removed = Vec::new();
                for step in steps {
                    match step {
                        UndoStep::Created(p, _) | UndoStep::Replaced(p) => {
                            changed.extend(parents(std::slice::from_ref(p)));
                            removed.push(p.clone());
                        }
                        UndoStep::Moved { from, to } => {
                            changed.extend(parents(&[from.clone(), to.clone()]));
                            removed.push(to.clone());
                        }
                    }
                }
                Affected { changed, removed }
            }
        }
    }
    fn run(self, shared: &Shared) -> Done {
        match self {
            Self::Paste {
                paths,
                destination,
                cut,
            } => Done::Report(job::run_transfer(shared, &paths, &destination, cut)),
            Self::Trash(paths) => Done::Report(job::run_trash(shared, &paths)),
            Self::Undo(steps) => Done::Report(job::run_undo(shared, &steps)),
            Self::RemoveLeftovers { dest, leftovers } => Done::Report(
                job::leftover::run_remove_leftovers(shared, dest, &leftovers),
            ),
            Self::Rename { path, name } => {
                Done::Simple(ops::rename_entry(&path, &name).map_err(|e| e.to_string()))
            }
            Self::Open(path) => {
                Done::Simple(crate::platform::reveal::open_path(&path).map_err(|e| e.to_string()))
            }
            Self::Create { dir, name, folder } => {
                Done::Simple(ops::create_entry(&dir, &name, folder).map_err(|e| e.to_string()))
            }
        }
    }
}

#[derive(Debug, Default, PartialEq)]
struct Affected {
    changed: Vec<PathBuf>,
    removed: Vec<PathBuf>,
}

struct Target {
    origin: crate::intent::IntentOrigin,
    surface: u32,
    binding: SurfaceBinding,
    view: Weak<()>,
    selection: Option<Weak<()>>,
    clipboard: Option<Arc<AtomicBool>>,
    reload: bool,
    clear_selection: bool,
    /// 만든 항목. 성공했고 요청한 explorer 가 아직 그 폴더를 보고 있으면 이 항목을 고른다.
    created: Option<PathBuf>,
}
struct Request {
    /// 대기열에서 이 요청을 가리키는 번호.
    id: u64,
    target: Target,
    operation: Operation,
}
#[derive(Default)]
pub(crate) struct Requests(VecDeque<Request>, u64);
impl Requests {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
    /// 아직 시작하지 않은 요청을 뺀다.
    pub(crate) fn remove(&mut self, id: u64) {
        self.0.retain(|r| r.id != id);
    }
    /// `surface` 가 요청해 기다리는 작업들. 진행 표시가 없는 이름 변경·열기는 뺀다.
    fn queued_for(&self, surface: u32) -> Vec<crate::explorer_ui::view::ops::Queued> {
        self.0
            .iter()
            .filter(|r| r.target.surface == surface)
            .filter_map(|r| {
                let kind = r.operation.kind()?;
                let (dest, count) = r.operation.summary();
                Some(crate::explorer_ui::view::ops::Queued {
                    id: r.id,
                    kind,
                    count,
                    dest,
                })
            })
            .collect()
    }
}

impl crate::state::MainViewState {
    /// This entry is only used by native user menus/shortcuts and the user rename popup.
    /// 잘라내기 붙여넣기는 explorer 클립보드에서 온 것으로 보고 성공하면 클립보드를 비운다.
    pub(crate) fn request_explorer_file(
        &mut self,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        surface: u32,
        operation: Operation,
        origin: crate::intent::IntentOrigin,
    ) {
        self.enqueue_explorer_file(engine, surface, operation, origin, true);
    }
    /// 클립보드와 무관한 사용자 작업(드래그·다시 시도·되돌리기)을 요청한다.
    pub(crate) fn request_explorer_file_direct(
        &mut self,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        surface: u32,
        operation: Operation,
        origin: crate::intent::IntentOrigin,
    ) {
        self.enqueue_explorer_file(engine, surface, operation, origin, false);
    }
    fn enqueue_explorer_file(
        &mut self,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        surface: u32,
        operation: Operation,
        origin: crate::intent::IntentOrigin,
        from_clipboard: bool,
    ) {
        if !matches!(origin, crate::intent::IntentOrigin::User { .. })
            || engine.is_mirror_surface(surface)
        {
            return;
        }
        let Some(binding) = SurfaceBinding::capture(engine, surface) else {
            return;
        };
        // 사용자 조작으로 들어온 요청이므로 받지 않을 때도 그 칸에 이유를 보인다.
        let refused = if self.explorer_file_requests.0.len() >= MAX_PENDING_PER_VIEW {
            Some(crate::i18n::t("explorer.state.queue_full").to_string())
        } else if operation.bytes() > MAX_REQUEST_BYTES {
            Some(crate::i18n::t("explorer.state.request_too_large").to_string())
        } else {
            None
        };
        if let Some(message) = refused {
            tracing::warn!(
                "Explorer file request capacity exhausted; no filesystem operation started"
            );
            self.toasts.push(
                message,
                crate::adapters::ui::ToastKind::Info,
                crate::adapters::ui::ToastScope::Surface(surface),
            );
            return;
        }
        let clipboard = match &operation {
            Operation::Paste { cut: true, .. } if from_clipboard => self
                .explorer_clipboard
                .as_ref()
                .map(|c| Arc::clone(&c.identity)),
            _ => None,
        };
        let target = Target {
            surface,
            binding,
            origin,
            view: self.webview_identity.clone(),
            clipboard,
            selection: self
                .explorer_views
                .get(surface)
                .map(|v| v.selection_identity()),
            clear_selection: matches!(operation, Operation::Trash(_) | Operation::Rename { .. }),
            reload: !matches!(operation, Operation::Open(_)),
            created: match &operation {
                Operation::Create { dir, name, .. } => Some(dir.join(name)),
                _ => None,
            },
        };
        let requests = &mut self.explorer_file_requests;
        requests.1 += 1;
        let id = requests.1;
        requests.0.push_back(Request {
            id,
            target,
            operation,
        });
    }
}

struct Job {
    window: winit::window::WindowId,
    engine: crate::runtime::engine_session::EngineId,
    target: Target,
    affected: Affected,
    shared: Arc<Shared>,
    worker: JoinHandle<Done>,
    undo_of: Option<OpKind>,
    /// 마지막으로 다시 그린 진행 상태(끝난 항목, 바이트, 질문 여부). 바뀔 때만 다시 그린다.
    seen: (usize, u64, bool),
}
/// One running file operation serializes collision checks/moves. Pending requests remain bounded
/// in their originating Views and are discarded if that View closes before admission.
#[derive(Default)]
pub(crate) struct ExplorerFiles {
    job: Option<Job>,
    stopping: bool,
}
impl ExplorerFiles {
    pub(crate) fn has_pending(&self) -> bool {
        self.job.is_some()
    }
    pub(crate) fn begin_shutdown(&mut self) {
        self.stopping = true;
    }
    /// 끝난 작업을 거둔다. 아직 실행 중이거나 작업이 없으면 None 이다.
    fn take_finished(&mut self) -> Option<Finished> {
        if !self.job.as_ref().is_some_and(|j| j.worker.is_finished()) {
            return None;
        }
        let Job {
            window,
            engine,
            target,
            affected,
            worker,
            shared,
            undo_of,
            ..
        } = self.job.take().expect("finished job exists");
        Some(Finished {
            window,
            engine,
            target,
            affected,
            shared,
            undo_of,
            result: join(worker),
        })
    }
    pub(crate) fn poll_shutdown(&mut self) -> usize {
        self.begin_shutdown();
        // 종료 중에는 남은 항목을 멈춘다. 답을 기다리던 작업도 풀리고, 만들던 사본은 임시 폴더째 지워진다.
        if let Some(job) = &self.job {
            job.shared.cancel();
        }
        if self.job.as_ref().is_some_and(|j| j.worker.is_finished()) {
            let job = self.job.take().expect("finished job exists");
            report_result(&join(job.worker));
        }
        usize::from(self.job.is_some())
    }
}
impl Drop for ExplorerFiles {
    fn drop(&mut self) {
        if self.poll_shutdown() != 0 {
            tracing::warn!(
                "Explorer file worker remains unjoined at App drop; OS file work was not cancelled"
            );
        }
    }
}
struct Finished {
    window: winit::window::WindowId,
    engine: crate::runtime::engine_session::EngineId,
    target: Target,
    affected: Affected,
    shared: Arc<Shared>,
    undo_of: Option<OpKind>,
    result: Done,
}
impl Finished {
    /// 이 작업이 바꿀 수 있는 경로를 보는 로컬 explorer 를 다시 읽게 한다. 다시 읽은 View 가 있으면 true.
    fn reload_views(
        &self,
        store: &mut crate::adapters::ui::surface::explorer::view::ExplorerViewStore,
    ) -> bool {
        let Affected { changed, removed } = &self.affected;
        (!changed.is_empty() || !removed.is_empty()) && store.invalidate_local(changed, removed)
    }
}
fn join(worker: JoinHandle<Done>) -> Done {
    worker
        .join()
        .unwrap_or_else(|_| Done::Simple(Err("Explorer file worker panicked".into())))
}
fn report_result(result: &Done) {
    if let Some(error) = result.error_summary() {
        tracing::warn!(%error, "Explorer file operation failed");
    }
}
impl Target {
    fn matches_view(&self, view: &crate::view::state::ViewState) -> bool {
        view.matches_identity(&self.view)
    }
    fn view_is_current(&self, view: &crate::view::main::MainView) -> bool {
        self.matches_view(&view.base.state)
    }
    fn clipboard_is_current(&self, state: &crate::state::MainViewState) -> bool {
        self.clipboard.as_ref().is_none_or(|old| {
            state
                .explorer_clipboard
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(old, &current.identity))
        })
    }
    fn apply(self, state: &mut crate::state::MainViewState, success: bool) {
        if success && self.clipboard.is_some() && self.clipboard_is_current(state) {
            state.explorer_clipboard = None;
        }
        if self.reload
            && let Some(view) = state.explorer_views.get_mut(self.surface)
        {
            if success
                && self.clear_selection
                && self
                    .selection
                    .as_ref()
                    .is_some_and(|old| view.matches_selection(old))
            {
                view.clear_selection();
            }
            if success && let Some(path) = &self.created {
                view.reveal_created(path);
            }
            view.request_reload();
        }
    }
}
impl super::App {
    pub(crate) fn poll_explorer_files(&mut self) {
        self.finish_explorer_file();
        if !self.explorer_files.stopping && self.explorer_files.job.is_none() {
            self.start_explorer_file();
        }
        self.sync_explorer_ops();
    }
    fn finish_explorer_file(&mut self) {
        let Some(finished) = self.explorer_files.take_finished() else {
            return;
        };
        // 같은 폴더를 보는 다른 explorer 도 다시 읽는다. 실패·부분 성공도 실제 상태를 다시 읽어야 하고,
        // 요청한 surface 가 그 사이 닫혔어도 다른 윈도우의 목록은 낡은 채로 남으면 안 된다.
        // mirror explorer 는 원격 파일을 보므로 `invalidate_local` 이 건너뛴다.
        for view in self.view.views.values_mut() {
            if let Some(view) = view.as_main_mut()
                && finished.reload_views(&mut view.state.explorer_views)
            {
                view.mark_dirty();
            }
        }
        let Finished {
            window,
            engine,
            target,
            result,
            shared,
            undo_of,
            ..
        } = finished;
        self.clear_explorer_running(&shared);
        let success = result.success();
        let failure = result.error_summary();
        if success && let Some(clipboard) = &target.clipboard {
            clipboard.store(true, Ordering::Release);
        }
        report_result(&result);
        if self.engines.of_window(window) != Some(engine) {
            return;
        }
        let Some(session) = self.engines.get(engine) else {
            return;
        };
        if !target.binding.current(&session) {
            return;
        }
        let Some(view) = self
            .view
            .views
            .get_mut(&window)
            .and_then(|v| v.as_main_mut())
        else {
            return;
        };
        if !target.view_is_current(view) {
            return;
        }
        if let Done::Report(report) = result {
            let lifetime =
                std::time::Duration::from_millis(session.read().settings.overlay.toast_duration_ms);
            push_result(&mut view.state, target.surface, report, undo_of, lifetime);
        } else if let Some(error) = failure {
            view.state.toasts.push(
                crate::i18n::t_fmt("explorer.state.operation_failed", &error),
                crate::adapters::ui::ToastKind::Error,
                crate::adapters::ui::ToastScope::Surface(target.surface),
            );
        }
        target.apply(&mut view.state, success);
        view.mark_dirty();
    }
    fn start_explorer_file(&mut self) {
        for (&window, view) in &mut self.view.views {
            let Some(view) = view.as_main_mut() else {
                continue;
            };
            let Some(engine) = self.engines.of_window(window) else {
                continue;
            };
            let Some(session) = self.engines.get(engine) else {
                continue;
            };
            while let Some(Request {
                target, operation, ..
            }) = view.state.explorer_file_requests.0.pop_front()
            {
                if !matches!(target.origin, crate::intent::IntentOrigin::User { .. })
                    || !target.view_is_current(view)
                    || !target.binding.current(&session)
                    || session.read().is_mirror_surface(target.surface)
                    || target
                        .clipboard
                        .as_ref()
                        .is_some_and(|c| c.load(Ordering::Acquire))
                {
                    continue;
                }
                let affected = operation.affected();
                let undo_of = operation.undo_of();
                let wake = self.view.proxy.clone();
                let shared = Arc::new(ui_sync::interactive_shared(wake.clone()));
                show_running(&mut view.state, target.surface, &operation, &shared);
                let worker_shared = Arc::clone(&shared);
                match std::thread::Builder::new()
                    .name("explorer-files".into())
                    .spawn(move || {
                        let result = operation.run(&worker_shared);
                        if wake.send_event(crate::AppEvent::TimerTick).is_err() {
                            tracing::debug!("Explorer file completion after event loop closed");
                        }
                        result
                    }) {
                    Ok(worker) => {
                        self.explorer_files.job = Some(Job {
                            window,
                            engine,
                            target,
                            affected,
                            shared,
                            worker,
                            undo_of,
                            seen: (0, 0, false),
                        })
                    }
                    Err(error) => {
                        tracing::warn!(%error, "Explorer file worker spawn failed");
                        if let Some(v) = view.state.explorer_views.get_mut(target.surface) {
                            v.ops.running = None;
                        }
                    }
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
