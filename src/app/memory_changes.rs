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

    /// writer 경로가 옮긴 변경을 먼저, 직접 잠금 소비자가 저장소에 남긴 변경을 뒤에 꺼낸다.
    #[test]
    fn changes_moved_by_the_writer_precede_changes_left_in_the_store() {
        let store: Arc<Mutex<dyn MemoryStorage>> = Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().unwrap(),
        ));
        let core = crate::ipc::handler::cli_entry_tests::test_core_builder()
            .with_memory(store.clone())
            .build()
            .unwrap();
        let mut ticket = core
            .memory_access()
            .submit(MemoryRequest::Write(put("a")))
            .unwrap();
        assert!(ticket.try_take().is_some());
        store
            .lock()
            .unwrap()
            .put(
                HOST_OWNER,
                &Scope::Global,
                "b",
                &MemoryValue::Text("v".into()),
                &PutOpts::default(),
            )
            .unwrap();
        let keys: Vec<String> = core
            .take_memory_changes()
            .into_iter()
            .map(|c| c.key)
            .collect();
        assert_eq!(keys, ["a", "b"]);
        assert!(core.take_memory_changes().is_empty());
    }

    /// 다른 소비자가 저장소 잠금을 잡고 있으면 기다리지 않고 writer가 옮긴 변경만 꺼낸다. 남은
    /// 변경은 잠금이 풀린 뒤 다음 회차에 같은 순서로 나온다.
    #[test]
    fn a_held_store_lock_does_not_block_and_keeps_the_rest_for_later() {
        let store: Arc<Mutex<dyn MemoryStorage>> = Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().unwrap(),
        ));
        let core = crate::ipc::handler::cli_entry_tests::test_core_builder()
            .with_memory(store.clone())
            .build()
            .unwrap();
        let mut ticket = core
            .memory_access()
            .submit(MemoryRequest::Write(put("a")))
            .unwrap();
        assert!(ticket.try_take().is_some());
        let mut guard = store.lock().unwrap();
        guard
            .put(
                HOST_OWNER,
                &Scope::Global,
                "b",
                &MemoryValue::Text("v".into()),
                &PutOpts::default(),
            )
            .unwrap();
        let keys: Vec<String> = core
            .take_memory_changes()
            .into_iter()
            .map(|c| c.key)
            .collect();
        assert_eq!(keys, ["a"]);
        drop(guard);
        let keys: Vec<String> = core
            .take_memory_changes()
            .into_iter()
            .map(|c| c.key)
            .collect();
        assert_eq!(keys, ["b"]);
    }
}
