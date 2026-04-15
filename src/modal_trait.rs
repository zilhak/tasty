use std::sync::Arc;

use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

use crate::AppEvent;

/// Action returned by a modal after handling a window event.
pub enum ModalAction {
    /// Modal is still open, no action needed.
    Pending,
    /// Close the modal with no further side effects.
    Close,
    /// Close the modal and fire an AppEvent.
    CloseWithEvent(AppEvent),
}

/// Trait for modal windows.
///
/// A modal is a separate OS window that, while open, is the sole focus target
/// in the application. All other windows have their input blocked.
/// At most one modal can exist at a time.
pub trait Modal {
    /// Get a reference to the modal's window.
    fn window(&self) -> &Arc<Window>;

    /// Mark the modal as needing a redraw.
    fn mark_dirty(&mut self);

    /// Handle a window event. Returns the action to take.
    fn handle_window_event(
        &mut self,
        event: WindowEvent,
        event_loop: &ActiveEventLoop,
    ) -> ModalAction;

    /// Downcast support for extracting modal-specific data on close.
    fn as_any(&self) -> &dyn std::any::Any;

    /// Downcast support (mutable) for extracting modal-specific data on close.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}
