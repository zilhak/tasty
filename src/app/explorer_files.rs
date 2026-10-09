//! Bounded local Explorer actions. Views queue fixed inputs; App owns execution and joins.
mod ops;

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
}
impl Operation {
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
            Self::Rename { path, name } => path.as_os_str().len() + name.len(),
            Self::Open(path) => path.as_os_str().len(),
            Self::Create { dir, name, .. } => dir.as_os_str().len() + name.len(),
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
            Self::Rename { path, .. } => Affected {
                changed: parents(std::slice::from_ref(path)),
                removed: vec![path.clone()],
            },
            Self::Open(_) => Affected::default(),
            Self::Create { dir, .. } => Affected {
                changed: vec![dir.clone()],
                removed: Vec::new(),
            },
        }
    }
    fn run(self) -> Result<(), String> {
        match self {
            Self::Paste {
                paths,
                destination,
                cut,
            } => {
                let (ok, error) = ops::paste_all(&paths, &destination, cut);
                error.map_or(Ok(()), |error| {
                    Err(format!("paste ({ok} succeeded): {error}"))
                })
            }
            Self::Trash(paths) => trash::delete_all(paths).map_err(|e| e.to_string()),
            Self::Rename { path, name } => {
                ops::rename_entry(&path, &name).map_err(|e| e.to_string())
            }
            Self::Open(path) => {
                crate::platform::reveal::open_path(&path).map_err(|e| e.to_string())
            }
            Self::Create { dir, name, folder } => {
                ops::create_entry(&dir, &name, folder).map_err(|e| e.to_string())
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
    target: Target,
    operation: Operation,
}
#[derive(Default)]
pub(crate) struct Requests(VecDeque<Request>);
#[cfg(test)]
impl Requests {
    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }
}

impl crate::state::MainViewState {
    /// This entry is only used by native user menus/shortcuts and the user rename popup.
    pub(crate) fn request_explorer_file(
        &mut self,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        surface: u32,
        operation: Operation,
        origin: crate::intent::IntentOrigin,
    ) {
        if !matches!(origin, crate::intent::IntentOrigin::User { .. })
            || engine.is_mirror_surface(surface)
        {
            return;
        }
        let Some(binding) = SurfaceBinding::capture(engine, surface) else {
            return;
        };
        if self.explorer_file_requests.0.len() >= MAX_PENDING_PER_VIEW
            || operation.bytes() > MAX_REQUEST_BYTES
        {
            tracing::warn!(
                "Explorer file request capacity exhausted; no filesystem operation started"
            );
            return;
        }
        let clipboard = match &operation {
            Operation::Paste { cut: true, .. } => self
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
        self.explorer_file_requests
            .0
            .push_back(Request { target, operation });
    }
}

struct Job {
    window: winit::window::WindowId,
    engine: crate::runtime::engine_session::EngineId,
    target: Target,
    affected: Affected,
    worker: JoinHandle<Result<(), String>>,
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
        } = self.job.take().expect("finished job exists");
        Some(Finished {
            window,
            engine,
            target,
            affected,
            result: join(worker),
        })
    }
    pub(crate) fn poll_shutdown(&mut self) -> usize {
        self.begin_shutdown();
        if self.job.as_ref().is_some_and(|j| j.worker.is_finished()) {
            let job = self.job.take().expect("finished job exists");
            report_result(join(job.worker));
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
    result: Result<(), String>,
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
fn join(worker: JoinHandle<Result<(), String>>) -> Result<(), String> {
    worker
        .join()
        .unwrap_or_else(|_| Err("Explorer file worker panicked".into()))
}
fn report_result(result: Result<(), String>) {
    if let Err(error) = result {
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
        if self.explorer_files.stopping || self.explorer_files.job.is_some() {
            return;
        }
        self.start_explorer_file();
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
            ..
        } = finished;
        let success = result.is_ok();
        let failure = result.as_ref().err().cloned();
        if success && let Some(clipboard) = &target.clipboard {
            clipboard.store(true, Ordering::Release);
        }
        report_result(result);
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
        if let Some(error) = failure {
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
            while let Some(Request { target, operation }) =
                view.state.explorer_file_requests.0.pop_front()
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
                let wake = self.view.proxy.clone();
                match std::thread::Builder::new()
                    .name("explorer-files".into())
                    .spawn(move || {
                        let result = operation.run();
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
                            worker,
                        })
                    }
                    Err(error) => tracing::warn!(%error, "Explorer file worker spawn failed"),
                }
                return;
            }
        }
    }
}

#[cfg(test)]
mod tests;
