//! Supply no prefix before completion, but bind browser Abort to this exact native GIO read.
use super::super::EntryResponseSource;
use super::{
    finish_headers,
    pending_reads::{self, PendingReads},
};
use gio::subclass::prelude::*;
use glib::translate::*;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex, OnceLock},
    thread::ThreadId,
};
use tasty_remote::html_body::{NativeBodyResponse, ResponseWait};
use webkit2gtk::URISchemeResponse;

mod imp {
    use super::*;
    #[derive(Default)]
    pub struct DeferredStream {
        pub(super) thread: OnceLock<ThreadId>,
        pub(super) lease: OnceLock<pending_reads::StreamLease>,
        pub(super) source: OnceLock<EntryResponseSource>,
        pub(super) registry: OnceLock<glib::SendWeakRef<PendingReads>>,
        pub(super) response: RefCell<Option<glib::SendWeakRef<URISchemeResponse>>>,
        pub(super) wait: RefCell<Option<ResponseWait>>,
        pub(super) body: Mutex<Option<(Arc<NativeBodyResponse>, usize)>>,
    }
    #[glib::object_subclass]
    impl ObjectSubclass for DeferredStream {
        const NAME: &'static str = "TastyHtmlDeferredStream";
        type Type = super::DeferredStream;
        type ParentType = gio::InputStream;
        fn class_init(class: &mut Self::Class) {
            // SAFETY: GLib subclass classes begin with the declared parent class. GIO owns the
            // async buffer until our one task callback; read_finish propagates that same task once.
            let class = unsafe { &mut *(class as *mut _ as *mut gio::ffi::GInputStreamClass) };
            class.read_async = Some(super::read_async);
            class.read_finish = Some(super::read_finish);
            class.skip_async = Some(super::skip_async);
            class.skip_finish = Some(super::read_finish);
            class.close_async = Some(super::close_async);
            class.close_finish = Some(super::close_finish);
        }
    }
    impl ObjectImpl for DeferredStream {}
    impl InputStreamImpl for DeferredStream {
        fn close(&self, cancellable: Option<&gio::Cancellable>) -> Result<(), glib::Error> {
            if self.thread.get() != Some(&std::thread::current().id()) {
                return Err(glib::Error::new(
                    gio::IOErrorEnum::NotSupported,
                    "remote close used outside native thread",
                ));
            }
            if cancellable.is_some_and(|c| c.is_cancelled()) {
                return Err(pending_reads::cancelled());
            }
            if self.wait.borrow_mut().take().is_some()
                && let Some(source) = self.source.get()
            {
                source.wake();
            }
            Ok(())
        }

        fn read(
            &self,
            buffer: &mut [u8],
            cancellable: Option<&gio::Cancellable>,
        ) -> Result<usize, glib::Error> {
            if cancellable.is_some_and(|c| c.is_cancelled()) {
                return Err(pending_reads::cancelled());
            }
            if !self.source.get().is_some_and(|s| s.is_current()) {
                return Err(pending_reads::cancelled());
            }
            let mut body = match self.body.lock() {
                Ok(body) => body,
                Err(error) => {
                    tracing::error!(%error, "remote native read state poisoned; denying bytes");
                    return Err(glib::Error::new(
                        gio::IOErrorEnum::Failed,
                        "remote native read state failed",
                    ));
                }
            };
            let Some((body, offset)) = &mut *body else {
                return Err(glib::Error::new(
                    gio::IOErrorEnum::WouldBlock,
                    "remote response is incomplete",
                ));
            };
            let bytes = &body.bytes()[*offset..];
            let count = buffer.len().min(bytes.len());
            buffer[..count].copy_from_slice(&bytes[..count]);
            *offset += count;
            Ok(count)
        }
    }
}
glib::wrapper! { pub struct DeferredStream(ObjectSubclass<imp::DeferredStream>) @extends gio::InputStream; }
impl DeferredStream {
    pub(super) fn new(
        source: EntryResponseSource,
        wait: ResponseWait,
        registry: &PendingReads,
    ) -> Option<Self> {
        let lease = registry.reserve_stream()?;
        let this: Self = glib::Object::new();
        this.imp().lease.get_or_init(|| lease);
        this.imp()
            .thread
            .get_or_init(|| std::thread::current().id());
        this.imp().source.get_or_init(|| source);
        this.imp()
            .registry
            .get_or_init(|| registry.downgrade().into());
        *this.imp().wait.borrow_mut() = Some(wait);
        registry.track_stream(&this);
        Some(this)
    }
    pub(super) fn cancel_wait(&self) {
        if self.imp().thread.get() == Some(&std::thread::current().id()) {
            self.imp().wait.borrow_mut().take();
        }
    }
    pub(super) fn set_response(&self, response: &URISchemeResponse) {
        *self.imp().response.borrow_mut() = Some(response.downgrade().into());
    }
    fn install(&self, body: Arc<NativeBodyResponse>) -> Result<(), glib::Error> {
        let Some(source) = self.imp().source.get().filter(|s| s.is_current()) else {
            return Err(pending_reads::cancelled());
        };
        let Some(response) = self
            .imp()
            .response
            .borrow()
            .as_ref()
            .and_then(|r| r.upgrade())
        else {
            return Err(pending_reads::cancelled());
        };
        // WebKit reads these fields only after the first read completes. Mutate on the native
        // thread before returning any bytes, including empty EOF. CSP cannot be relaxed by HTML.
        finish_headers(&response, source, body.mime(), body.bytes().len() as i64);
        match self.imp().body.lock() {
            Ok(mut state) => {
                *state = Some((body, 0));
                Ok(())
            }
            Err(error) => {
                tracing::error!(%error, "remote native body publication poisoned; denying response");
                Err(glib::Error::new(
                    gio::IOErrorEnum::Failed,
                    "remote native body publication failed",
                ))
            }
        }
    }
}
// SAFETY: called by GIO with a live subclass stream and an output buffer retained until callback.
// The thread check precedes every SendWeakRef upgrade and native response access.
unsafe extern "C" fn read_async(
    stream: *mut gio::ffi::GInputStream,
    buffer: *mut u8,
    count: usize,
    priority: i32,
    cancellable: *mut gio::ffi::GCancellable,
    callback: gio::ffi::GAsyncReadyCallback,
    user_data: glib::ffi::gpointer,
) {
    // SAFETY: the native call owns live stream/cancellable references for the duration of this call.
    let this: glib::translate::Borrowed<DeferredStream> =
        unsafe { from_glib_borrow(stream.cast()) };
    // SAFETY: nullable borrowed GIO cancellable; clone ownership for the asynchronous read.
    let cancellable: Option<gio::Cancellable> = unsafe { from_glib_none(cancellable) };
    // SAFETY: caller owns callback/data until this read's one completion, consumed by read_finish.
    let task =
        unsafe { completion_task(&this, cancellable.as_ref(), priority, callback, user_data) };
    if count == 0 {
        pending_reads::return_task(task, Ok(0));
        return;
    }
    if buffer.is_null() || count > isize::MAX as usize {
        pending_reads::return_task(
            task,
            Err(glib::Error::new(
                gio::IOErrorEnum::InvalidArgument,
                "invalid remote native read buffer",
            )),
        );
        return;
    }
    if this.imp().thread.get() != Some(&std::thread::current().id()) {
        pending_reads::return_task(
            task,
            Err(glib::Error::new(
                gio::IOErrorEnum::NotSupported,
                "remote read used outside native thread",
            )),
        );
        return;
    }
    let Some(registry) = this.imp().registry.get().and_then(|r| r.upgrade()) else {
        pending_reads::return_task(task, Err(pending_reads::cancelled()));
        return;
    };
    let Some(source) = this.imp().source.get().cloned() else {
        pending_reads::return_task(task, Err(pending_reads::cancelled()));
        return;
    };
    let native_cancel = cancellable.unwrap_or_default();
    let Some(id) = registry.insert(task.clone(), native_cancel.clone()) else {
        pending_reads::return_task(
            task,
            Err(glib::Error::new(
                gio::IOErrorEnum::NoSpace,
                "remote native read capacity",
            )),
        );
        return;
    };
    let wait = this.imp().wait.borrow_mut().take();
    let wait_was_pending = wait.is_some();
    let this = this.clone();
    let weak = registry.downgrade();
    let future = glib::MainContext::ref_thread_default().spawn_local(async move {
        let result = async {
            if let Some(wait) = wait {
                let body = pending_reads::wait_for_body(source.clone(), wait)
                    .await
                    .map_err(|_| {
                        glib::Error::new(gio::IOErrorEnum::Failed, "remote file response ended")
                    })?;
                this.install(body)?;
            }
            // SAFETY: GIO keeps this buffer valid until the one LocalTask callback above.
            let bytes = unsafe { std::slice::from_raw_parts_mut(buffer.cast::<u8>(), count) };
            this.imp()
                .read(bytes, Some(&native_cancel))
                .map(|n| n as i64)
        };
        let result = gio::CancellableFuture::new(result, native_cancel.clone())
            .await
            .unwrap_or_else(|_| Err(pending_reads::cancelled()));
        if let Some(registry) = weak.upgrade() {
            registry.complete(id, result);
        }
        if wait_was_pending {
            source.wake();
        }
    });
    registry.attach(id, future);
}
// SAFETY: GIO calls finish once for the LocalTask returned by read_async on this stream.
unsafe extern "C" fn read_finish(
    _stream: *mut gio::ffi::GInputStream,
    result: *mut gio::ffi::GAsyncResult,
    error: *mut *mut glib::ffi::GError,
) -> isize {
    // SAFETY: this class's read_async always creates a LocalTask<i64>; GIO returns it unchanged.
    let task: gio::LocalTask<i64> = unsafe { from_glib_none(result.cast::<gio::ffi::GTask>()) };
    match task.propagate() {
        Ok(count) => count as isize,
        Err(reason) => {
            if !error.is_null() {
                // SAFETY: GIO provides writable error storage; transfer this owned error to it.
                let owned = unsafe { reason.into_glib_ptr() };
                // SAFETY: GIO supplies writable error storage for this finish invocation.
                unsafe {
                    *error = owned;
                }
            }
            -1
        }
    }
}

// SAFETY: caller supplies one native async operation and its live callback/data pair. Every class
// async override uses this same creator-thread GTask and the matching finish implementation.
unsafe fn completion_task(
    this: &DeferredStream,
    cancellable: Option<&gio::Cancellable>,
    priority: i32,
    callback: gio::ffi::GAsyncReadyCallback,
    user_data: glib::ffi::gpointer,
) -> gio::LocalTask<i64> {
    let completion = move |task: gio::LocalTask<i64>, source: Option<&DeferredStream>| {
        if let Some(callback) = callback {
            // SAFETY: native async caller retains its data until this exact completion callback.
            unsafe {
                callback(
                    source.map_or(std::ptr::null_mut(), |s| s.as_ptr().cast()),
                    task.as_ptr().cast(),
                    user_data,
                );
            }
        }
        pending_reads::callback_finished();
    };
    // SAFETY: task is never cast to a Send task; its result is returned/propagated once.
    let task = unsafe { gio::LocalTask::<i64>::new(Some(this), cancellable, completion) };
    task.set_priority(priority.into());
    task
}
// SAFETY: GIO calls with live stream, cancellable and callback storage for this operation.
unsafe extern "C" fn skip_async(
    stream: *mut gio::ffi::GInputStream,
    _count: usize,
    priority: i32,
    cancellable: *mut gio::ffi::GCancellable,
    callback: gio::ffi::GAsyncReadyCallback,
    user_data: glib::ffi::gpointer,
) {
    // SAFETY: objects are borrowed from this live GIO invocation.
    let this: glib::translate::Borrowed<DeferredStream> =
        unsafe { from_glib_borrow(stream.cast()) };
    // SAFETY: nullable cancellable from the same invocation, cloned into the task.
    let cancel: Option<gio::Cancellable> = unsafe { from_glib_none(cancellable) };
    // SAFETY: this completion is paired with skip_finish (the same i64 propagator as read_finish).
    let task = unsafe { completion_task(&this, cancel.as_ref(), priority, callback, user_data) };
    pending_reads::return_task(
        task,
        Err(glib::Error::new(
            gio::IOErrorEnum::NotSupported,
            "remote response stream cannot skip",
        )),
    );
}
// SAFETY: same GIO ownership contract as read_async, without a borrowed output buffer.
unsafe extern "C" fn close_async(
    stream: *mut gio::ffi::GInputStream,
    priority: i32,
    cancellable: *mut gio::ffi::GCancellable,
    callback: gio::ffi::GAsyncReadyCallback,
    user_data: glib::ffi::gpointer,
) {
    // SAFETY: live subclass instance owned by GIO throughout this invocation.
    let this: glib::translate::Borrowed<DeferredStream> =
        unsafe { from_glib_borrow(stream.cast()) };
    // SAFETY: nullable cancellable borrowed from the GIO caller, retained by task.
    let cancel: Option<gio::Cancellable> = unsafe { from_glib_none(cancellable) };
    // SAFETY: matching close_finish consumes this task's i64 result exactly once.
    let task = unsafe { completion_task(&this, cancel.as_ref(), priority, callback, user_data) };
    let result = this.imp().close(cancel.as_ref()).map(|()| 0);
    pending_reads::return_task(task, result);
}
// SAFETY: consumes only the LocalTask created above for this one close call.
unsafe extern "C" fn close_finish(
    stream: *mut gio::ffi::GInputStream,
    result: *mut gio::ffi::GAsyncResult,
    error: *mut *mut glib::ffi::GError,
) -> glib::ffi::gboolean {
    // SAFETY: matching local task and error storage passed through by GIO.
    unsafe { (read_finish(stream, result, error) >= 0).into_glib() }
}

#[cfg(test)]
mod tests;
