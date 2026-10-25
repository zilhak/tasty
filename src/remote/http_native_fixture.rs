//! Explicit host-only controls for isolated native integration tests; not public activation.
fn enabled() -> bool {
    std::env::var_os("TASTY_DEBUG_HTTP_MIRROR_NATIVE").as_deref() == Some(std::ffi::OsStr::new("1"))
}
pub(crate) fn hold_retained_resume() -> bool {
    enabled()
        && std::env::var_os("TASTY_DEBUG_HTTP_RELOAD_HOLD").as_deref()
            == Some(std::ffi::OsStr::new("1"))
}
