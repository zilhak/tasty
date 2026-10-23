//! Main-thread ownership for deferred GIO reads. No native object crosses a worker boundary.
use super::super::EntryResponseSource;
use glib::subclass::prelude::*;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    sync::atomic::{AtomicUsize, Ordering},
};

pub(super) struct StreamLease(std::sync::Arc<AtomicUsize>);
impl Drop for StreamLease {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}
static READY_CALLBACKS: AtomicUsize = AtomicUsize::new(0);
pub(super) fn callbacks_pending() -> bool {
    READY_CALLBACKS.load(Ordering::Acquire) != 0
}

struct Read {
    task: gio::LocalTask<i64>,
    cancel: gio::Cancellable,
    future: Option<glib::JoinHandle<()>>,
}
mod imp {
    use super::*;
    #[derive(Default)]
    pub struct PendingReads {
        pub(super) closed: Cell<bool>,
        pub(super) streams: std::sync::Arc<AtomicUsize>,
        pub(super) native_streams:
            RefCell<HashMap<usize, glib::WeakRef<super::super::deferred_stream::DeferredStream>>>,
        pub(super) sequence: Cell<u64>,
        pub(super) reads: RefCell<HashMap<u64, Read>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for PendingReads {
        const NAME: &'static str = "TastyHtmlPendingReads";
        type Type = super::PendingReads;
    }
    impl ObjectImpl for PendingReads {
        fn dispose(&self) {
            self.obj().close();
        }
    }
}
glib::wrapper! { pub struct PendingReads(ObjectSubclass<imp::PendingReads>); }
impl Default for PendingReads {
    fn default() -> Self {
        glib::Object::new()
    }
}
impl PendingReads {
    pub(super) fn available(&self) -> bool {
        !self.imp().closed.get()
            && self.imp().streams.load(Ordering::Acquire) < 256
            && self.imp().reads.borrow().len() < 256
    }
    pub(super) fn track_stream(&self, stream: &super::deferred_stream::DeferredStream) {
        let mut streams = self.imp().native_streams.borrow_mut();
        streams.retain(|_, weak| weak.upgrade().is_some());
        streams.insert(stream.as_ptr() as usize, stream.downgrade());
    }
    pub(super) fn reserve_stream(&self) -> Option<StreamLease> {
        if self.imp().closed.get() {
            return None;
        }
        self.imp()
            .streams
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| {
                n.checked_add(1).filter(|n| *n <= 256)
            })
            .ok()?;
        Some(StreamLease(std::sync::Arc::clone(&self.imp().streams)))
    }
    pub(super) fn insert(
        &self,
        task: gio::LocalTask<i64>,
        cancel: gio::Cancellable,
    ) -> Option<u64> {
        if self.imp().closed.get() || self.imp().reads.borrow().len() >= 256 {
            return None;
        }
        let next = self.imp().sequence.get().checked_add(1)?;
        self.imp().sequence.set(next);
        self.imp().reads.borrow_mut().insert(
            next,
            Read {
                task,
                cancel,
                future: None,
            },
        );
        Some(next)
    }
    pub(super) fn attach(&self, id: u64, future: glib::JoinHandle<()>) {
        if let Some(read) = self.imp().reads.borrow_mut().get_mut(&id) {
            read.future = Some(future);
        }
    }
    pub(super) fn complete(&self, id: u64, result: Result<i64, glib::Error>) {
        let read = self.imp().reads.borrow_mut().remove(&id);
        if let Some(read) = read {
            return_task(read.task, result);
        }
    }
    pub(super) fn close(&self) {
        self.imp().closed.set(true);
        let streams: Vec<_> = self
            .imp()
            .native_streams
            .borrow_mut()
            .drain()
            .filter_map(|(_, weak)| weak.upgrade())
            .collect();
        for stream in streams {
            stream.cancel_wait();
        }
        let reads: Vec<_> = self
            .imp()
            .reads
            .borrow_mut()
            .drain()
            .map(|(_, v)| v)
            .collect();
        for read in reads {
            read.cancel.cancel();
            return_task(read.task, Err(cancelled()));
            if let Some(future) = read.future {
                future.abort();
            }
        }
    }
}
pub(super) fn cancelled() -> glib::Error {
    glib::Error::new(gio::IOErrorEnum::Cancelled, "remote file read cancelled")
}
pub(super) fn return_task(task: gio::LocalTask<i64>, result: Result<i64, glib::Error>) {
    READY_CALLBACKS.fetch_add(1, Ordering::AcqRel);
    task.return_result(result);
}
pub(super) fn callback_finished() {
    READY_CALLBACKS.fetch_sub(1, Ordering::AcqRel);
}
/// Forward the producer's wake to both GLib and the host. GLib uses a separate X connection.
pub(super) async fn wait_for_body(
    source: EntryResponseSource,
    mut wait: tasty_remote::html_body::ResponseWait,
) -> Result<
    std::sync::Arc<tasty_remote::html_body::NativeBodyResponse>,
    tasty_remote::html_body::ResourceFailure,
> {
    use std::{
        future::{Future, poll_fn},
        pin::Pin,
        sync::Arc,
        task::{Context, Wake, Waker},
    };
    struct Forward {
        source: EntryResponseSource,
        native: Waker,
    }
    impl Wake for Forward {
        fn wake(self: Arc<Self>) {
            self.native.wake_by_ref();
            self.source.wake();
        }
    }
    poll_fn(move |context| {
        let waker = Waker::from(Arc::new(Forward {
            source: source.clone(),
            native: context.waker().clone(),
        }));
        Pin::new(&mut wait).poll(&mut Context::from_waker(&waker))
    })
    .await
}
