use std::sync::Arc;

use tasty_terminal::Terminal;

use super::EngineSession;
use crate::core::agent::event_feed::AgentEvent;
use crate::core::agent::task_waker::TerminalSnapshot;
use crate::core::output_observer::{ObserverSpec, SinkSpec};

fn session() -> EngineSession {
    EngineSession::new(80, 24, Arc::new(|| {})).expect("isolated session")
}

#[test]
fn execution_borrows_keep_one_terminal_and_one_task_scope() {
    let mut owner = session();
    let sid = owner.core_state.workspaces[0].all_surface_ids()[0];
    owner
        .runtime
        .terminals
        .insert(sid, Terminal::new_detached(80, 24), None);
    let hub = Arc::clone(owner.task_scope.waker_hub());
    let feed = Arc::clone(owner.task_scope.event_queue());
    let runners = Arc::clone(owner.task_scope.runner_registry());
    {
        let engine = owner.borrow_mut();
        assert!(Arc::ptr_eq(&hub, engine.task_scope.waker_hub()));
        assert!(Arc::ptr_eq(&feed, engine.task_scope.event_queue()));
        assert!(Arc::ptr_eq(&runners, engine.task_scope.runner_registry()));
        engine
            .runtime
            .terminals
            .get_mut(sid)
            .unwrap()
            .feed_bytes(b"one-owner");
        assert!(
            engine
                .as_ref()
                .find_terminal_by_id(sid)
                .unwrap()
                .screen_text(false)
                .contains("one-owner")
        );
    }
    assert!(
        owner
            .runtime
            .terminals
            .get(sid)
            .unwrap()
            .screen_text(false)
            .contains("one-owner")
    );
}

#[test]
fn session_drop_joins_observer_and_preserves_external_task_handles() {
    let mut owner = session();
    let sid = owner.core_state.workspaces[0].all_surface_ids()[0];
    let memory = Arc::clone(&owner.core_state.memory);
    let hub = Arc::clone(owner.task_scope.waker_hub());
    let feed = Arc::clone(owner.task_scope.event_queue());
    let observer = owner
        .observer_router
        .register(
            ObserverSpec {
                surface_id: Some(sid),
                parsers: vec!["path".into()],
                kinds: None,
                sink: SinkSpec::Memory { max_records: 0 },
            },
            Arc::clone(&memory),
        )
        .expect("observer worker");
    owner
        .observer_router
        .dispatch_text(sid, "/session-owner/accepted.rs\n");
    // No sleep or explicit join: the owning Session must finish the observer's accepted work.
    drop(owner);
    let records = memory
        .lock()
        .unwrap()
        .list(
            &tasty_memory::Scope::Global,
            &tasty_memory::ListOpts {
                prefix: Some(format!("tasty.observer.{observer}.")),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(records.len(), 1);
    assert!(
        feed.take_pending().0.is_empty(),
        "dropping a scope must not invent task cancellation"
    );
    hub.fire(
        1,
        &"held-task".to_string(),
        TerminalSnapshot {
            state: tasty_agent::TaskState::Succeeded,
            result: None,
        },
    );
    assert_eq!(
        feed.take_pending(),
        (
            vec![AgentEvent::TaskFinished {
                workspace_id: 1,
                task_id: "held-task".into(),
                state: "succeeded"
            }],
            0
        )
    );
}

#[test]
fn retired_terminal_content_cannot_mutate_the_replacement_binding() {
    let mut owner = session();
    let sid = owner.core_state.workspaces[0].all_surface_ids()[0];
    owner
        .runtime
        .terminals
        .insert(sid, Terminal::new_detached(80, 24), None);
    let old_epoch = owner
        .runtime
        .terminals
        .get(sid)
        .unwrap()
        .with_content(|v| v.cut().epoch);
    let (mut old, _old_pty) = owner
        .runtime
        .terminals
        .replace(sid, Terminal::new_detached(80, 24), None)
        .unwrap();
    owner
        .runtime
        .terminals
        .get_mut(sid)
        .unwrap()
        .feed_bytes(b"replacement");
    old.feed_bytes(b"late-old-content");
    let current = owner.runtime.terminals.get(sid).unwrap();
    assert_ne!(current.with_content(|v| v.cut().epoch), old_epoch);
    assert!(current.screen_text(false).contains("replacement"));
    assert!(!current.screen_text(false).contains("late-old-content"));
}

#[cfg(feature = "gui")]
#[test]
fn parked_session_keeps_terminal_hook_and_task_identity() {
    use crate::app::engine_registry::EngineRegistry;
    use crate::hook_runtime::HookExecutor;
    use tasty_hooks::{HookBinding, HookEvent};
    let (state, mut owner) = crate::state::tests::test_state();
    let sid = owner.core_state.workspaces[0].all_surface_ids()[0];
    owner
        .runtime
        .terminals
        .insert(sid, Terminal::new_detached(80, 24), None);
    let epoch = owner
        .runtime
        .terminals
        .get(sid)
        .unwrap()
        .with_content(|v| v.cut().epoch);
    let hub = Arc::clone(owner.task_scope.waker_hub());
    owner.hooks.add_surface_hook(
        sid,
        HookEvent::Bell,
        HookBinding::Handler("test.missing".into()),
        true,
    );
    let mut registry = EngineRegistry::default();
    let id = registry
        .insert_pending(owner)
        .unwrap_or_else(|_| panic!("empty registry"));
    let window = winit::window::WindowId::from(7);
    registry.attach_window(window, id);
    assert_eq!(registry.park(window, state), Some(id));
    {
        let engine = registry.get_mut(id).unwrap();
        engine
            .runtime
            .terminals
            .get_mut(sid)
            .unwrap()
            .feed_bytes(b"output-while-parked");
    }
    let (restored_id, _state) = registry.unpark_first().unwrap();
    assert_eq!(restored_id, id);
    let engine = registry.get_mut(id).unwrap();
    assert!(Arc::ptr_eq(&hub, engine.task_scope.waker_hub()));
    let terminal = engine.runtime.terminals.get(sid).unwrap();
    assert_eq!(terminal.with_content(|v| v.cut().epoch), epoch);
    assert!(terminal.screen_text(false).contains("output-while-parked"));
    let executor = HookExecutor::new(None);
    assert_eq!(engine.hooks.fire(&executor, sid, HookEvent::Bell).len(), 1);
    assert!(
        engine
            .hooks
            .fire(&executor, sid, HookEvent::Bell)
            .is_empty()
    );
}
