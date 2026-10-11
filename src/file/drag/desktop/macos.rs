//! AppKit retains native drag state until its end callback; cancellation never invents a drop.
use super::{Completion, Outcome, Request};
use objc2::{
    AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
    rc::Retained, runtime::ProtocolObject,
};
use objc2_app_kit::{
    NSApplication, NSDragOperation, NSDraggingContext, NSDraggingItem, NSDraggingSession,
    NSDraggingSource, NSEvent, NSEventType, NSView, NSWorkspace,
};
use objc2_foundation::{NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSURL};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ffi::CString,
    os::unix::ffi::OsStrExt,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use winit::{
    raw_window_handle::{HasWindowHandle, RawWindowHandle},
    window::Window,
};

pub(crate) struct GestureEvent(Retained<NSEvent>);
impl GestureEvent {
    pub(crate) fn capture(window: &Window) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let RawWindowHandle::AppKit(handle) = window.window_handle().ok()?.as_raw() else {
            return None;
        };
        // SAFETY: winit supplies a live NSView; this borrow cannot outlive the borrowed Window.
        let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
        let event = NSApplication::sharedApplication(mtm).currentEvent()?;
        let native_window = view.window()?;
        if matches!(
            event.r#type(),
            NSEventType::LeftMouseDown | NSEventType::LeftMouseDragged
        ) && event.windowNumber() == native_window.windowNumber()
        {
            Some(Self(event))
        } else {
            None
        }
    }
}
struct SourceState {
    completion: Completion,
    cancelled: Arc<AtomicBool>,
    ended: Cell<bool>,
}
define_class!(
    // SAFETY: NSObject allows subclassing; AppKit invokes NSDraggingSource only on the main thread.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "TastyOutboundFileSource"]
    #[ivars = SourceState]
    struct Source;
    unsafe impl NSObjectProtocol for Source {}
    unsafe impl NSDraggingSource for Source {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation(
            &self,
            _session: &NSDraggingSession,
            context: NSDraggingContext,
        ) -> NSDragOperation {
            if self.ivars().cancelled.load(Ordering::Acquire)
                || context == NSDraggingContext::WithinApplication
            {
                NSDragOperation::None
            } else {
                NSDragOperation::Copy
            }
        }
        #[unsafe(method(ignoreModifierKeysForDraggingSession:))]
        fn ignore_modifiers(&self, _session: &NSDraggingSession) -> bool {
            true
        }
        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, _point: NSPoint, operation: NSDragOperation) {
            if !self.ivars().ended.replace(true) {
                let outcome = if operation == NSDragOperation::None {
                    Outcome::Cancelled
                } else if operation == NSDragOperation::Copy {
                    Outcome::Copied
                } else {
                    Outcome::Unknown
                };
                // Never reenter the App/View here. The queued event releases retained state later.
                self.ivars().completion.send(Ok(outcome));
            }
        }
    }
);
struct Active {
    _source: Retained<Source>,
    _session: Retained<NSDraggingSession>,
    _window: Arc<Window>,
}
thread_local! {
    static ACTIVE: RefCell<HashMap<u64, Active>> = RefCell::new(HashMap::new());
}
pub(super) fn release_completed(id: u64) {
    // Called from the queued App event, including when the originating View was closed.
    ACTIVE.with(|active| {
        active.borrow_mut().remove(&id);
    });
}
pub(super) fn start(request: &Request, completion: Completion) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("AppKit drag must start on the main thread")?;
    if ACTIVE.with(|active| !active.borrow().is_empty()) {
        return Err("AppKit drag is still awaiting native completion".into());
    }
    let event = &request
        .event
        .as_ref()
        .ok_or("no native mouse event for the held gesture")?
        .0;
    if NSEvent::pressedMouseButtons() & 1 == 0 {
        return Err("drag button is no longer held".into());
    }
    let RawWindowHandle::AppKit(handle) = request
        .window
        .window_handle()
        .map_err(|e| e.to_string())?
        .as_raw()
    else {
        return Err("AppKit drag requires an NSView".into());
    };
    // SAFETY: The request's Arc and Active retain the winit window through actual native completion.
    let view = unsafe { &*handle.ns_view.as_ptr().cast::<NSView>() };
    let window = view.window().ok_or("drag source view has no window")?;
    if event.windowNumber() != window.windowNumber() {
        return Err("drag event belongs to a different window".into());
    }
    let point = view.convertPoint_fromView(event.locationInWindow(), None);
    let workspace = NSWorkspace::sharedWorkspace();
    let mut items = Vec::with_capacity(request.paths.len());
    for path in &request.paths {
        let path_bytes = CString::new(path.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        // SAFETY: CString lives through the call, which copies the filesystem bytes.
        // File-system representation preserves non-UTF8 Unix path bytes; no canonicalization.
        let url = unsafe {
            NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(
                std::ptr::NonNull::new(path_bytes.as_ptr().cast_mut()).expect("CString pointer"),
                path.is_dir(),
                None,
            )
        };
        let native_path = url.path().ok_or("file URL has no path")?;
        let icon = workspace.iconForFile(&native_path);
        let item = NSDraggingItem::initWithPasteboardWriter(
            NSDraggingItem::alloc(),
            ProtocolObject::from_ref(&*url),
        );
        // SAFETY: AppKit accepts an NSImage for contents. Its system-provided size supplies the frame.
        unsafe {
            item.setDraggingFrame_contents(NSRect::new(point, icon.size()), Some(&icon));
        }
        items.push(item);
    }
    let source = Source::alloc(mtm).set_ivars(SourceState {
        completion,
        cancelled: request.cancelled.clone(),
        ended: Cell::new(false),
    });
    // SAFETY: NSObject's init is valid after objc2 has installed the Rust ivars.
    let source: Retained<Source> = unsafe { msg_send![super(source), init] };
    let items = NSArray::from_retained_slice(&items);
    let session = view.beginDraggingSessionWithItems_event_source(
        &items,
        event,
        ProtocolObject::from_ref(&*source),
    );
    ACTIVE.with(|active| {
        active.borrow_mut().insert(
            request.id,
            Active {
                _source: source,
                _session: session,
                _window: request.window.clone(),
            },
        );
    });
    Ok(())
}
