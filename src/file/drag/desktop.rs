//! Native UI-thread handoff; callbacks only enqueue completion and never borrow a View.
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "macos")]
pub(crate) use macos::GestureEvent;

use super::Outcome;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
use winit::{
    event_loop::EventLoopProxy,
    window::{Window, WindowId},
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct Handle {
    pub(crate) id: u64,
    cancelled: Arc<AtomicBool>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
    }
}
pub(crate) struct Request {
    window: Arc<Window>,
    paths: Vec<PathBuf>,
    id: u64,
    cancelled: Arc<AtomicBool>,
    #[cfg(target_os = "macos")]
    pub(crate) event: Option<GestureEvent>,
}
impl Request {
    pub(crate) fn new(window: Arc<Window>, paths: Vec<PathBuf>) -> (Self, Handle) {
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        (
            Self {
                window,
                paths,
                id,
                cancelled: cancelled.clone(),
                #[cfg(target_os = "macos")]
                event: None,
            },
            Handle { id, cancelled },
        )
    }
    pub(crate) fn start(self, proxy: EventLoopProxy<crate::AppEvent>) {
        let completion = Completion {
            window: self.window.id(),
            id: self.id,
            proxy,
        };
        let result = super::payload::validate(&self.paths).and_then(|()| {
            if self.cancelled.load(Ordering::Acquire) {
                return Err("file drag source was cancelled".into());
            }
            #[cfg(target_os = "windows")]
            {
                windows::start(&self).map(Some)
            }
            #[cfg(target_os = "macos")]
            {
                macos::start(&self, completion.clone()).map(|()| None)
            }
        });
        match result {
            Ok(Some(outcome)) => completion.send(Ok(outcome)),
            Ok(None) => {}
            Err(error) => completion.send(Err(error)),
        }
    }
}
#[derive(Clone)]
struct Completion {
    window: WindowId,
    id: u64,
    proxy: EventLoopProxy<crate::AppEvent>,
}
impl Completion {
    fn send(&self, result: Result<Outcome, String>) {
        if self
            .proxy
            .send_event(crate::AppEvent::OutboundDragEnded {
                window: self.window,
                id: self.id,
                result,
            })
            .is_err()
        {
            tracing::debug!("outbound completion after event loop shutdown");
        }
    }
}
pub(crate) fn release_completed(_id: u64) {
    #[cfg(target_os = "macos")]
    macos::release_completed(_id);
}
