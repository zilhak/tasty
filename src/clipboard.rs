#[cfg(feature = "gui")]
pub(crate) mod context;

#[cfg(feature = "gui")]
pub(crate) use context::ClipboardContext;
