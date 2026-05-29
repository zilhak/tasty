//! `tasty-memory` regular 영역의 변경을 `memory.changed` 로 broadcast.

use crate::app::App;

impl App {
    /// `tasty-memory` regular 영역의 누적 변경을 drain 해 `memory.changed` host
    /// event 로 broadcast. secret 영역 변경은 store 가 발화 큐에 넣지 않으므로
    /// 자동으로 누락된다 (다른 plugin 누설 방지).
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
