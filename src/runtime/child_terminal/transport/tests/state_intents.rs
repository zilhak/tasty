use super::*;
use crate::runtime::child_terminal::ParentKey;

#[test]
fn confirmed_deletions_release_transient_intent_bytes() {
    let mut f = Fixture::new();
    let mut client = f.client();
    for child in 100..116 {
        client.register_child(ParentKey::External("owned".into()), entry(child, 0));
        client.set_idle(child, true);
        wait(&client.submit().unwrap()).unwrap();
        assert!(client.unregister_child_by_surface(child));
        wait(&client.submit().unwrap()).unwrap();
        let state = client.client.lock().unwrap();
        for view in [&state.confirmed, &state.accepted, &state.visible] {
            assert!(view.idle_intents.is_empty());
            assert!(view.needs_input_intents.is_empty());
            assert!(view.parent_of_child(child).is_none());
            let mut without_intents = view.clone();
            without_intents.idle_intents.clear();
            without_intents.needs_input_intents.clear();
            assert_eq!(registry_bytes(view), registry_bytes(&without_intents));
        }
        drop(state);
        assert_eq!(f.hub.state.lock().unwrap().bytes, 0);
    }
    assert!(f.disk().idle.is_empty());
    assert!(f.disk().needs_input.is_empty());
    f.drain();
}

#[test]
fn deletion_ack_keeps_later_admitted_and_unsent_recycled_child_reports() {
    let mut f = Fixture::new();
    let mut client = f.client();
    client.register_child(10, entry(100, 0));
    client.set_idle(100, true);
    wait(&client.submit().unwrap()).unwrap();
    let (release, gate) = f.gate();
    assert!(client.unregister_child_by_surface(100));
    let deleted = client.submit().unwrap();
    client.register_child(20, entry(100, 0));
    client.set_idle(100, true);
    client.set_needs_input(100, true);
    let admitted = client.submit().unwrap();
    client.set_idle(100, false);
    assert!(deleted.poll().is_none());
    assert!(admitted.poll().is_none());
    gate_done(release, gate);
    wait(&deleted).unwrap();
    wait(&admitted).unwrap();
    assert_eq!(f.disk().parent_of_child(100), Some(20.into()));
    assert!(f.disk().list_children(&10.into()).is_empty());
    assert_eq!(f.disk().state_of(100), "needs_input");
    assert_eq!(client.state_of(100), "active");
    {
        let state = client.client.lock().unwrap();
        assert_eq!(state.accepted.needs_input.get(&100), Some(&true));
        assert!(!state.visible.idle_intents.get(&100).unwrap().1);
        assert!(!state.visible.needs_input_intents.get(&100).unwrap().1);
    }
    wait(&client.submit().unwrap()).unwrap();
    assert_eq!(f.disk().state_of(100), "active");
    assert_eq!(f.hub.state.lock().unwrap().bytes, 0);
    f.drain();
}

#[test]
fn earlier_ack_does_not_restore_unsent_deleted_state_intents() {
    let mut f = Fixture::new();
    let mut client = f.client();
    client.register_child(10, entry(100, 0));
    client.set_idle(100, true);
    wait(&client.submit().unwrap()).unwrap();
    let (release, gate) = f.gate();
    client.update_child(&10.into(), 0, |entry| {
        entry.nickname = Some("latest".into())
    });
    let earlier = client.submit().unwrap();
    assert!(client.unregister_child_by_surface(100));
    gate_done(release, gate);
    wait(&earlier).unwrap();
    {
        let state = client.client.lock().unwrap();
        assert!(state.visible.idle_intents.is_empty());
        assert!(state.visible.needs_input_intents.is_empty());
        assert!(state.visible.parent_of_child(100).is_none());
        assert_eq!(state.confirmed.idle_intents.len(), 1);
    }
    wait(&client.submit().unwrap()).unwrap();
    assert!(f.disk().parent_of_child(100).is_none());
    f.drain();
}
