//! Owned file requests. The host injects its existing file queue; no synchronous fallback.
use std::{path::PathBuf, sync::Arc};
use tasty_remote_profiles::{Passkeys, RemoteProfile};

pub struct Input {
    pub original: RemoteProfile,
    pub passkeys: Passkeys,
    pub root: PathBuf,
}
pub enum Request {
    Load {
        root: PathBuf,
        name: String,
        expected: Option<RemoteProfile>,
    },
    Save {
        root: PathBuf,
        original: RemoteProfile,
        failed: bool,
        cancel: tasty_ssh::SshCancel,
    },
}
pub enum Value {
    Loaded(Input),
    Saved,
}
pub struct Failure {
    pub message: String,
    pub unknown: bool,
}
pub trait Receipt: Send {
    /// Consume at most one terminal result; never wait or execute file I/O here.
    fn poll(&mut self) -> Option<Result<Value, Failure>>;
}
pub type Submit = Arc<dyn Fn(Request) -> Result<Box<dyn Receipt>, Failure> + Send + Sync>;
