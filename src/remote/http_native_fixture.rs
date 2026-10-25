//! Scoped debug negotiation for exercising an unfinished native App adapter in isolated instances.
pub(crate) fn enabled() -> bool {
    std::env::var_os("TASTY_DEBUG_HTTP_MIRROR_NATIVE").as_deref() == Some(std::ffi::OsStr::new("1"))
}
