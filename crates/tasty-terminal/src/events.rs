use std::sync::Arc;

/// Callback to wake the event loop when PTY data arrives.
pub type Waker = Arc<dyn Fn() + Send + Sync>;

/// Events emitted by the terminal during processing.
pub struct TerminalEvent {
    /// The surface ID that generated this event (0 if not yet assigned).
    pub surface_id: u32,
    pub kind: TerminalEventKind,
}

/// Types of events a terminal can emit.
pub enum TerminalEventKind {
    /// A notification from OSC 9 / OSC 99 / OSC 777.
    Notification { title: String, body: String },
    /// Bell character received.
    BellRing,
    /// Window title changed via OSC 0 / OSC 2.
    TitleChanged(String),
    /// Current working directory changed via OSC 7.
    CwdChanged(String),
    /// The child process has exited.
    ProcessExited,
    /// Terminal requested clipboard set via OSC 52.
    ClipboardSet(String),
}

/// Mouse tracking modes (DECSET 1000/1002/1003).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseTrackingMode {
    None,
    Click,      // 1000
    CellMotion, // 1002
    AllMotion,  // 1003
}
