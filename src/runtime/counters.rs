//! Process-local execution identities. Logical workspace/pane/tab/surface IDs are never issued here.
use std::sync::{Arc,atomic::{AtomicU32,AtomicU64}};
#[derive(Clone)]
pub struct RuntimeCounters {
    pty:Arc<AtomicU32>,observer:Arc<AtomicU64>,hook:Arc<AtomicU64>,global_hook:Arc<AtomicU32>,notification:Arc<AtomicU64>,
}
impl Default for RuntimeCounters {fn default()->Self {Self::new()}}
impl RuntimeCounters {
    pub fn new()->Self {Self {
        pty:Arc::new(AtomicU32::new(crate::runtime::terminal_store::PTY_ID_BASE)),
        observer:Arc::new(AtomicU64::new(1)),hook:Arc::new(AtomicU64::new(1)),global_hook:Arc::new(AtomicU32::new(0)),notification:Arc::new(AtomicU64::new(1)),
    }}
    pub fn pty_counter(&self)->Arc<AtomicU32> {self.pty.clone()}
    pub fn observer_counter(&self)->Arc<AtomicU64> {self.observer.clone()}
    pub fn hook_counter(&self)->Arc<AtomicU64> {self.hook.clone()}
    pub fn global_hook_counter(&self)->Arc<AtomicU32> {self.global_hook.clone()}
    pub fn notification_counter(&self)->Arc<AtomicU64> {self.notification.clone()}
}
