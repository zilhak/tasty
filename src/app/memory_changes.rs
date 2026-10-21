//! regular 메모리 변경을 memory.changed 이벤트로 전달한다. GUI 이벤트 루프와 headless 루프가
//! 회차마다 부른다. plugin 매니저가 없으면 꺼낸 변경을 버려 버퍼가 쌓이지 않게 한다.

use crate::app::App;

impl App {
    /// secret 변경은 저장소가 이벤트 큐에 넣지 않아 다른 플러그인에 전달되지 않는다.
    pub(crate) fn dispatch_pending_memory_changes(&mut self) {
        use tasty_plugin_protocol::EventScope;
        use tasty_plugin_protocol::events::payloads::{
            MemoryChangeKind as ProtoKind, MemoryChanged,
        };
        let changes = self.services.take_memory_changes();
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

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use tasty_memory::writer::MemoryRequest;
    use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryValue, PutOpts, Scope, WriteOp};

    fn put(key: &str) -> WriteOp {
        WriteOp::Put {
            owner: HOST_OWNER.into(),
            scope: Scope::Global,
            key: key.into(),
            value: MemoryValue::Text("v".into()),
            opts: PutOpts::default(),
        }
    }

    /// 변경은 commit 순서대로 버퍼에서 꺼내며 저장소 잠금을 잡지 않는다. 잠금이 잡혀 있어도 기다리지
    /// 않는다.
    #[test]
    fn committed_changes_come_out_in_order_without_the_store_lock() {
        let store: Arc<Mutex<dyn MemoryStorage>> = Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().unwrap(),
        ));
        let core = crate::ipc::handler::cli_entry_tests::test_core_builder()
            .with_memory(store.clone())
            .build()
            .unwrap();
        for key in ["a", "b"] {
            let mut ticket = core
                .memory_access()
                .submit(MemoryRequest::Write(put(key)))
                .unwrap();
            assert!(ticket.try_take().is_some());
        }
        let guard = store.lock().unwrap();
        let keys: Vec<String> = core
            .take_memory_changes()
            .into_iter()
            .map(|c| c.key)
            .collect();
        assert_eq!(keys, ["a", "b"]);
        assert!(core.take_memory_changes().is_empty());
        drop(guard);
    }
}
