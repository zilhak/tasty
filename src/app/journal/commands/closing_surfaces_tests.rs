//! The plugin host must know which surfaces an admitted close may retire, from admission until
//! the close finishes, so a plugin restart in between does not republish them.
use super::*;

#[test]
fn an_admitted_close_holds_its_targets_until_it_finishes() {
    let (mut session, mut journal) = boot();
    let rx = send(
        &mut journal,
        request(
            "workspace.create",
            serde_json::json!({"name":"held"}),
            None,
            911,
        ),
    );
    assert!(finish(&mut journal, &mut session, &rx).error.is_none());
    let surface = session
        .core_state
        .workspaces()
        .iter()
        .last()
        .unwrap()
        .all_surface_ids()[0];
    assert!(journal.closing_surfaces(&[&session]).is_empty());

    let rx = send(
        &mut journal,
        request(
            "surface.close",
            serde_json::json!({"surface_id":surface}),
            None,
            912,
        ),
    );
    assert_eq!(
        journal.closing_surfaces(&[&session]),
        [surface].into(),
        "held from admission, before its targets are resolved"
    );
    let mut resolved = false;
    let mut stall = StallBudget::new(&journal);
    let response = loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        for (ticket, _) in journal.requests_needing_resolution() {
            journal.resolve_ipc_for_engine(ticket, &session);
            resolved = true;
        }
        if let Ok(response) = rx.try_recv() {
            break response;
        }
        if resolved {
            assert_eq!(
                journal.closing_surfaces(&[&session]),
                [surface].into(),
                "held while the resolved close commits and retires"
            );
        }
        stall.nap("held close");
    };
    assert!(resolved);
    assert_eq!(response.result.unwrap()["closed"], true);
    assert!(
        journal.closing_surfaces(&[&session]).is_empty(),
        "released once the close finished"
    );
}

#[test]
fn a_close_of_a_missing_surface_holds_nothing() {
    let (session, mut journal) = boot();
    let _rx = send(
        &mut journal,
        request(
            "surface.close",
            serde_json::json!({"surface_id":u32::MAX - 1}),
            None,
            913,
        ),
    );
    assert!(journal.closing_surfaces(&[&session]).is_empty());
}
