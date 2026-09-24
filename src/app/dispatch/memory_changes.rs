//! regular 메모리 변경을 memory.changed 이벤트로 전달한다.

use crate::app::App;

impl App {
    /// secret 변경은 저장소가 이벤트 큐에 넣지 않아 다른 플러그인에 전달되지 않는다.
    pub(crate) fn dispatch_pending_memory_changes(&mut self) {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::events::payloads::{
            MemoryChangeKind as ProtoKind, MemoryChanged,
        };
        let changes = self.core.with_memory(|s| s.take_pending_changes());
        if changes.is_empty() {
            return;
        }
        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };
        for ch in changes {
            let kind = match ch.kind {
                tasty_memory::MemoryChangeKind::Created => ProtoKind::Created,
                tasty_memory::MemoryChangeKind::Updated => ProtoKind::Updated,
                tasty_memory::MemoryChangeKind::Deleted => ProtoKind::Deleted,
                tasty_memory::MemoryChangeKind::Expired => ProtoKind::Expired,
            };
            let payload = MemoryChanged {
                scope: ch.scope,
                key: ch.key,
                kind,
                version: ch.version,
            };
            mgr.emit_host_event("memory.changed", &payload, EventScope::System);
        }
    }
}
