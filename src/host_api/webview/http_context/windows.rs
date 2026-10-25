//! Refuse remote HTTP contexts whose mandatory native transport is unavailable.
//!
//! Chromium's SOCKS5 transport does not implement RFC1929 authentication. The
//! document endpoint requires it, so constructing a normal WebView here would
//! either fail authentication or risk a direct-network fallback. See
//! docs/dev-guide/windows-http-context.md for the native boundary and probe.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ContextError {
    AuthenticatedSocksUnavailable,
}

/// No successful context can escape this factory until the native contract is met.
/// In particular, a caller cannot substitute a local/default WebView after refusal.
#[derive(Debug)]
pub(crate) struct DataStore(std::convert::Infallible);

impl DataStore {
    /// Check before preparing a profile, runtime, endpoint or native widget.
    /// This performs no filesystem, COM, network or protocol-ID work.
    pub(crate) fn new() -> Result<Self, ContextError> {
        Err(ContextError::AuthenticatedSocksUnavailable)
    }
}

#[cfg(all(test, windows))]
#[path = "windows/tests.rs"]
mod tests;
