//! 이미지 파일 저장·디코딩처럼 GUI 이벤트 루프를 멈추는 작업을 App 소유 worker 하나에서 실행한다.
//! 작업 하나는 결과 하나를 돌려준다. 결과 값이 아직 전달하지 않은 파일을 소유하므로, 대상이
//! 사라져 결과를 버리면 그 파일도 함께 회수된다.

pub(crate) mod capture;
pub(crate) mod paste;
#[cfg(test)]
mod tests;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread::JoinHandle;

use crate::adapters::ui::ToastKind;

/// 접수한 뒤 결과를 회수하지 않은 작업 수. 실행 중인 작업을 포함한다.
pub(crate) const MAX_JOBS: usize = 8;
/// 접수한 작업이 메모리에 들고 있는 입력 바이트의 합. 디스크에 있는 입력은 세지 않는다.
/// 비어 있는 worker 는 이 상한을 넘는 한 건도 받는다. 크기만으로 영영 저장하지 못하는 요청을 만들지 않는다.
pub(crate) const MAX_HELD_BYTES: u64 = 256 * 1024 * 1024;

/// 작업을 받지 못한 이유.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ImageJobRejected {
    ShuttingDown,
    /// 대기 건수나 메모리 바이트 상한에 닿았다.
    Busy,
    Spawn(String),
}

impl std::fmt::Display for ImageJobRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShuttingDown => write!(f, "image worker is shutting down"),
            Self::Busy => write!(f, "image worker capacity exhausted"),
            Self::Spawn(reason) => write!(f, "image worker thread failed: {reason}"),
        }
    }
}

type Work<T> = Box<dyn FnOnce() -> T + Send>;

/// App 이 받는 작업 결과.
pub(crate) enum ImageJobDone {
    Paste(paste::SavedPaste),
    Capture(capture::StoredCapture),
}

impl crate::app::App {
    pub(crate) fn poll_image_jobs(&mut self) {
        self.apply_image_job_results();
        self.submit_pending_image_saves();
    }

    fn submit_pending_image_saves(&mut self) {
        let mut requests = Vec::new();
        for session in self.engines.all_sessions_mut() {
            requests.extend(
                std::mem::take(&mut session.remote.pending_image_saves)
                    .into_iter()
                    .map(|request| (session.id, request)),
            );
        }
        for (engine, pending) in requests {
            let paste::PendingImageSave {
                target,
                origin_view,
                bracketed,
                file_name,
                png_bytes,
            } = pending;
            let surface = target.surface_id();
            let request = paste::PasteRequest {
                target,
                origin_view: origin_view.clone(),
                bracketed,
            };
            let submitted = self.image_jobs.submit(png_bytes.len() as u64, move || {
                let file = paste::save(&file_name, &png_bytes);
                ImageJobDone::Paste(paste::SavedPaste {
                    engine,
                    request,
                    file,
                })
            });
            if let Err(error) = submitted {
                self.report_image_save_rejected(&origin_view, surface, error);
            }
        }
    }

    /// 종료 중의 거절은 안내하지 않는다.
    fn report_image_save_rejected(
        &mut self,
        origin: &std::sync::Weak<()>,
        surface: u32,
        error: ImageJobRejected,
    ) {
        tracing::warn!(%error, "clipboard image save not accepted");
        let (message, kind) = match error {
            ImageJobRejected::ShuttingDown => return,
            ImageJobRejected::Busy => (
                crate::i18n::t("toast.image_paste_busy").to_string(),
                ToastKind::Warning,
            ),
            other => (
                crate::i18n::t_fmt_fit("toast.image_paste_save_failed", &other.to_string()),
                ToastKind::Error,
            ),
        };
        self.toast_image_paste_origin(origin, surface, message, kind);
    }

    fn apply_image_job_results(&mut self) {
        for done in self.image_jobs.take_results() {
            match done {
                ImageJobDone::Paste(saved) => self.apply_saved_paste(saved),
                ImageJobDone::Capture(stored) => self.apply_stored_capture(stored),
            }
        }
    }

    fn apply_saved_paste(&mut self, saved: paste::SavedPaste) {
        let paste::SavedPaste {
            engine,
            request,
            file,
        } = saved;
        match file {
            Ok(file) => self.deliver_saved_paste(engine, &request, file),
            Err(reason) => self.report_image_save_failed(&request, &reason),
        }
    }

    /// 경로를 입력하지 못한 파일은 여기서 결과와 함께 버려져 회수된다.
    fn deliver_saved_paste(
        &mut self,
        engine: crate::runtime::engine_session::EngineId,
        request: &paste::PasteRequest,
        file: paste::SavedFile,
    ) {
        let Some(mut engine) = self.engines.get_mut(engine) else {
            tracing::debug!("clipboard image saved after its engine retired; file removed");
            return;
        };
        let delivery = paste::deliver(&mut engine, request, file);
        if delivery != paste::PasteDelivery::Delivered {
            tracing::debug!(
                ?delivery,
                "clipboard image path not delivered; file removed"
            );
        }
    }

    fn report_image_save_failed(&mut self, request: &paste::PasteRequest, reason: &str) {
        tracing::warn!(%reason, "clipboard image save failed");
        let message = crate::i18n::t_fmt_fit("toast.image_paste_save_failed", reason);
        let surface = request.target.surface_id();
        self.toast_image_paste_origin(&request.origin_view, surface, message, ToastKind::Error);
    }

    /// 안내는 요청한 View 에만 표시한다. 그 View 가 닫혔으면 다른 창을 고르지 않는다.
    fn toast_image_paste_origin(
        &mut self,
        origin: &std::sync::Weak<()>,
        surface: u32,
        message: String,
        kind: ToastKind,
    ) {
        use crate::view::ui::View as _;
        let main = self
            .view
            .views
            .values_mut()
            .filter_map(|view| view.as_main_mut())
            .find(|main| main.base.state.matches_identity(origin));
        let Some(main) = main else {
            tracing::debug!("image paste result for a closed View: {message}");
            return;
        };
        main.state.toasts.push(
            message,
            kind,
            crate::adapters::ui::ToastScope::Surface(surface),
        );
        main.mark_dirty();
    }
}

/// 작업은 접수 순서대로 한 번에 하나씩 실행한다. 같은 대상의 붙여넣기 두 건은 접수 순서로 끝난다.
/// Windows 클립보드 소유 창은 스레드마다 하나라 worker 스레드를 작업마다 새로 만들지 않는다.
pub(crate) struct ImageWorker<T: Send + 'static> {
    queue: Option<mpsc::Sender<(u64, Work<T>)>>,
    results_tx: mpsc::Sender<(u64, T)>,
    results_rx: mpsc::Receiver<(u64, T)>,
    worker: Option<JoinHandle<()>>,
    wake: Arc<dyn Fn() + Send + Sync>,
    /// 종료가 시작되면 worker 가 아직 시작하지 않은 작업을 실행하지 않고 버린다.
    cancel: Arc<AtomicBool>,
    jobs: usize,
    held_bytes: u64,
}

impl<T: Send + 'static> ImageWorker<T> {
    pub(crate) fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (results_tx, results_rx) = mpsc::channel();
        Self {
            queue: None,
            results_tx,
            results_rx,
            worker: None,
            wake,
            cancel: Arc::new(AtomicBool::new(false)),
            jobs: 0,
            held_bytes: 0,
        }
    }

    /// 작업을 접수한다. 호출자는 기다리지 않는다. 결과는 [`take_results`](Self::take_results)로 받는다.
    pub(crate) fn submit(
        &mut self,
        held_bytes: u64,
        work: impl FnOnce() -> T + Send + 'static,
    ) -> Result<(), ImageJobRejected> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(ImageJobRejected::ShuttingDown);
        }
        self.reap_dead_worker();
        let over_bytes = self.held_bytes.saturating_add(held_bytes) > MAX_HELD_BYTES;
        if self.jobs >= MAX_JOBS || (self.jobs != 0 && over_bytes) {
            return Err(ImageJobRejected::Busy);
        }
        let queue = match &self.queue {
            Some(queue) => queue,
            None => self.start_worker()?,
        };
        if queue.send((held_bytes, Box::new(work))).is_err() {
            self.queue = None;
            return Err(ImageJobRejected::Spawn("image worker stopped".into()));
        }
        self.jobs += 1;
        self.held_bytes += held_bytes;
        Ok(())
    }

    fn start_worker(&mut self) -> Result<&mpsc::Sender<(u64, Work<T>)>, ImageJobRejected> {
        let (queue, jobs) = mpsc::channel::<(u64, Work<T>)>();
        let results = self.results_tx.clone();
        let wake = self.wake.clone();
        let cancel = self.cancel.clone();
        let worker = std::thread::Builder::new()
            .name("image-jobs".into())
            .spawn(move || {
                while let Ok((bytes, work)) = jobs.recv() {
                    if cancel.load(Ordering::Acquire) {
                        tracing::debug!("image job skipped during shutdown");
                        continue;
                    }
                    let done = work();
                    if results.send((bytes, done)).is_err() {
                        // 결과를 버리면 결과가 소유한 미전달 파일도 함께 지워진다.
                        tracing::debug!("image job result dropped after its owner ended");
                    }
                    wake();
                }
            })
            .map_err(|error| ImageJobRejected::Spawn(error.to_string()))?;
        self.worker = Some(worker);
        Ok(self.queue.insert(queue))
    }

    /// worker 가 패닉으로 끝났으면 회수하지 못할 접수 건수를 비우고 다음 접수 때 새로 시작한다.
    fn reap_dead_worker(&mut self) {
        if !self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            return;
        }
        // 끝나기 전에 보낸 결과는 아직 채널에 있으므로 호출자가 먼저 take_results 로 받는다.
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            tracing::warn!(lost = self.jobs, "image worker panicked");
        }
        self.queue = None;
        self.jobs = 0;
        self.held_bytes = 0;
    }

    /// 끝난 작업의 결과를 접수 순서대로 돌려준다.
    pub(crate) fn take_results(&mut self) -> Vec<T> {
        let mut done = Vec::new();
        while let Ok((bytes, result)) = self.results_rx.try_recv() {
            self.jobs = self.jobs.saturating_sub(1);
            self.held_bytes = self.held_bytes.saturating_sub(bytes);
            done.push(result);
        }
        done
    }

    /// 결과를 회수하지 않은 작업이 있는가.
    #[cfg(test)]
    pub(crate) fn has_pending(&self) -> bool {
        self.jobs != 0
    }

    /// 새 작업을 받지 않고, 아직 시작하지 않은 작업은 실행하지 않는다.
    pub(crate) fn begin_shutdown(&mut self) {
        self.cancel.store(true, Ordering::Release);
        self.queue = None;
    }

    /// 종료 중에는 결과를 전달하지 않고 버린다. 버린 결과의 미전달 파일은 그 자리에서 회수된다.
    /// worker 가 아직 끝나지 않았으면 1 이상을 돌려준다.
    pub(crate) fn poll_shutdown(&mut self) -> usize {
        self.begin_shutdown();
        let discarded = self.take_results().len();
        if discarded != 0 {
            tracing::debug!(discarded, "image job results discarded during shutdown");
        }
        match &self.worker {
            Some(worker) if worker.is_finished() => {
                if self
                    .worker
                    .take()
                    .is_some_and(|worker| worker.join().is_err())
                {
                    tracing::warn!("image worker panicked");
                }
                // join 직전에 보낸 결과도 회수한다.
                drop(self.take_results());
                0
            }
            Some(_) => self.jobs.max(1),
            None => 0,
        }
    }
}

impl<T: Send + 'static> Drop for ImageWorker<T> {
    fn drop(&mut self) {
        let remaining = self.poll_shutdown();
        if remaining != 0 {
            tracing::warn!(remaining, "image worker still unjoined at owner drop");
        }
    }
}
