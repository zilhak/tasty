use super::*;
use tasty_core::{
    CreationDestination, CreationPlan, DomainEvent, OperationId, PreparationResult,
    StructuralResult, SurfaceSpec,
};

#[test]
fn a_multi_stream_creation_completes_only_after_every_operation_and_preserves_result_order() {
    exercise(false, false);
}

#[test]
fn public_creation_progress_survives_restart_and_keeps_each_completed_wire_part() {
    exercise(true, true);
}

#[test]
fn public_creation_keeps_completed_wire_parts_in_original_order() {
    exercise(true, false);
}

fn exercise(public: bool, restart: bool) {
    let home = tempfile::tempdir().unwrap();
    let mut worker = start(home.path());
    seed_category(&worker);
    submit(&worker, 101, Work::Admit(header("second-category")));
    finished(&worker, 101).unwrap();
    submit(&worker, 101, Work::Reserve(vec![(IdKind::Category, 1)]));
    finished(&worker, 101).unwrap();
    submit(
        &worker,
        101,
        Work::Resolve {
            changes: vec![category("slot-2", 2)],
            response: None,
        },
    );
    publish(&worker);
    finished(&worker, 101).unwrap();
    submit(&worker, 1, Work::Admit(header("two-workspaces")));
    finished(&worker, 1).unwrap();
    submit(
        &worker,
        1,
        Work::Reserve(vec![
            (IdKind::Workspace, 2),
            (IdKind::Pane, 2),
            (IdKind::Tab, 2),
            (IdKind::Surface, 2),
        ]),
    );
    let ResultValue::Reserved(ids) = finished(&worker, 1).unwrap() else {
        panic!("ids")
    };
    let id = |kind: IdKind, index: u32| {
        ids.iter()
            .find(|range| range.kind == kind.label())
            .unwrap()
            .start as u32
            + index
    };
    submit(
        &worker,
        1,
        Work::PutPreparation(PreparationInput {
            adopt: None,
            child: None,
            kind: "empty".into(),
            cwd: None,
            params: serde_json::json!({}),
            shell: None,
            restore: None,
        }),
    );
    let ResultValue::InputStored(input) = finished(&worker, 1).unwrap() else {
        panic!("input")
    };
    submit(
        &worker,
        1,
        Work::Resolve {
            changes: (0..2)
                .map(|index| StreamCommand {
                    stream: format!("structure:slot-{}", index + 1),
                    command: StructuralCommand::PrepareCreation {
                        operation: OperationId(String::new()),
                        command_id: String::new(),
                        input,
                        plan: CreationPlan {
                            destination: CreationDestination::Workspace {
                                workspace: id(IdKind::Workspace, index),
                                pane: id(IdKind::Pane, index),
                                tab: id(IdKind::Tab, index),
                                name: format!("workspace-{index}"),
                                category: index + 1,
                                subtitle: String::new(),
                                description: String::new(),
                                attach_mapping: None,
                            },
                            surface: SurfaceSpec {
                                id: id(IdKind::Surface, index),
                                kind: "empty".into(),
                                data: None,
                            },
                            tab_name: "empty".into(),
                            explicit_name: None,
                        },
                    },
                })
                .collect(),
            response: public.then(|| {
                ResponsePlan::Multiple(
                    (0..2)
                        .map(|index| ResponsePlan::WorkspaceCreated {
                            stream: format!("structure:slot-{}", index + 1),
                            id: id(IdKind::Workspace, index),
                            surface_id: id(IdKind::Surface, index),
                        })
                        .collect(),
                )
            }),
        },
    );
    let prepared = publish(&worker);
    finished(&worker, 1).unwrap();
    let operations: Vec<_> = prepared
        .streams
        .iter()
        .map(|(stream, events)| {
            let operation = events
                .iter()
                .find_map(|event| match &event.event {
                    DomainEvent::OperationPrepared { operation } => Some(operation.id.clone()),
                    _ => None,
                })
                .unwrap();
            (stream.clone(), operation)
        })
        .collect();
    // Finish the second stream first to catch response ordering based on arrival order.
    for index in [1, 0] {
        let (stream, operation) = &operations[index];
        submit(
            &worker,
            10,
            Work::ClaimPreparation {
                stream: stream.clone(),
                operation: operation.clone(),
            },
        );
        let ResultValue::Claimed(claimed) = finished(&worker, 10).unwrap() else {
            panic!("claim")
        };
        submit(
            &worker,
            11,
            Work::Prepared {
                lease: claimed.lease.clone(),
                result: PreparationResult::Ready { data: None },
            },
        );
        publish(&worker);
        finished(&worker, 11).unwrap();
        submit(
            &worker,
            12,
            Work::CleanupFinished {
                view: crate::runtime::journal_product::CompletionView::default(),
                lease: claimed.lease,
            },
        );
        publish(&worker);
        finished(&worker, 12).unwrap();
        submit(&worker, 13, Work::Admit(header("two-workspaces")));
        let ResultValue::Stored(record) = finished(&worker, 13).unwrap() else {
            panic!("record")
        };
        if public {
            if index == 1 {
                assert_eq!(record.status, tasty_event_store::CommandStatus::InProgress);
                let progress: super::super::response::ResponseProgress =
                    serde_json::from_slice(record.response.as_ref().unwrap()).unwrap();
                assert!(progress.replies[0].is_none());
                assert_eq!(
                    progress.replies[1]
                        .as_ref()
                        .unwrap()
                        .result
                        .as_ref()
                        .unwrap()["name"],
                    "workspace-1"
                );
                // Later live metadata must not rewrite an already frozen wire reply.
                submit(&worker, 20, Work::Admit(header("rename-completed")));
                finished(&worker, 20).unwrap();
                submit(
                    &worker,
                    20,
                    Work::Resolve {
                        changes: vec![StreamCommand {
                            stream: "structure:slot-2".into(),
                            command: StructuralCommand::UpdateWorkspaceMeta {
                                workspace_id: id(IdKind::Workspace, 1),
                                name: Some("later-name".into()),
                                subtitle: None,
                                description: None,
                            },
                        }],
                        response: None,
                    },
                );
                publish(&worker);
                finished(&worker, 20).unwrap();
                if restart {
                    // A claim without a completion receipt remains unknown after a new writer.
                    // Recovery never reruns its factory merely to complete the aggregate reply.
                    let (pending_stream, pending_operation) = &operations[0];
                    submit(
                        &worker,
                        21,
                        Work::ClaimPreparation {
                            stream: pending_stream.clone(),
                            operation: pending_operation.clone(),
                        },
                    );
                    assert!(matches!(
                        finished(&worker, 21).unwrap(),
                        ResultValue::Claimed(_)
                    ));
                    let frozen = serde_json::to_vec(progress.replies[1].as_ref().unwrap()).unwrap();
                    drop(worker);
                    worker = start(home.path());
                    submit(&worker, 22, Work::Admit(header("two-workspaces")));
                    assert!(matches!(
                        finished(&worker, 22).unwrap(),
                        ResultValue::RecoveryRequired { replay: true, .. }
                    ));
                    submit(&worker, 23, Work::ReadEngine(pending_stream.clone()));
                    let ResultValue::Engine(model) = finished(&worker, 23).unwrap() else {
                        panic!("pending model")
                    };
                    assert!(matches!(
                        model.operations[pending_operation].outcome,
                        Some(tasty_core::OperationOutcome::Uncertain { .. })
                    ));
                    assert!(model.workspaces.is_empty());
                    submit(
                        &worker,
                        24,
                        Work::ClaimPreparation {
                            stream: pending_stream.clone(),
                            operation: pending_operation.clone(),
                        },
                    );
                    assert!(
                        finished(&worker, 24).is_err(),
                        "old unknown work cannot be reexecuted"
                    );
                    let identity: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(home.path().join("structure/journal.json")).unwrap(),
                    )
                    .unwrap();
                    let store = tasty_event_store::EventStore::open(
                        &home.path().join("structure/journal.db"),
                        identity["journal_id"].as_str().unwrap(),
                    )
                    .unwrap();
                    let admission = header("two-workspaces");
                    let tasty_event_store::CommandLookup::Hit(record) = store
                        .lookup_command(admission.key.as_ref().unwrap(), &admission.original_digest)
                        .unwrap()
                    else {
                        panic!("original command")
                    };
                    assert_eq!(record.status, tasty_event_store::CommandStatus::InProgress);
                    let preserved: super::super::response::ResponseProgress =
                        serde_json::from_slice(record.response.as_ref().unwrap()).unwrap();
                    assert!(preserved.replies[0].is_none());
                    assert_eq!(
                        serde_json::to_vec(preserved.replies[1].as_ref().unwrap()).unwrap(),
                        frozen
                    );
                    submit(&worker, 25, Work::ReadEngine("structure:slot-2".into()));
                    let ResultValue::Engine(model) = finished(&worker, 25).unwrap() else {
                        panic!("completed model")
                    };
                    assert_eq!(
                        model.workspaces[&id(IdKind::Workspace, 1)].name,
                        "later-name"
                    );
                    return;
                }
            } else {
                assert_eq!(record.status, tasty_event_store::CommandStatus::Completed);
                let response: tasty_ipc::protocol::JsonRpcResponse =
                    serde_json::from_slice(record.response.as_ref().unwrap()).unwrap();
                let results = response.result.unwrap();
                assert_eq!(results[0]["name"], "workspace-0");
                assert_eq!(results[1]["name"], "workspace-1");
                assert_eq!(results[0]["surface_id"], id(IdKind::Surface, 0));
                assert_eq!(results[1]["surface_id"], id(IdKind::Surface, 1));
                submit(&worker, 21, Work::ReadEngine("structure:slot-2".into()));
                let ResultValue::Engine(model) = finished(&worker, 21).unwrap() else {
                    panic!("model")
                };
                assert_eq!(
                    model.workspaces[&id(IdKind::Workspace, 1)].name,
                    "later-name"
                );
            }
            continue;
        }
        let results: Vec<StructuralResult> =
            serde_json::from_slice(record.response.as_ref().unwrap()).unwrap();
        assert_eq!(results.len(), 2);
        if index == 1 {
            assert_eq!(record.status, tasty_event_store::CommandStatus::InProgress);
            assert!(matches!(&results[0], StructuralResult::Pending { .. }));
        } else {
            assert_eq!(record.status, tasty_event_store::CommandStatus::Completed);
            assert!(
                matches!(&results[0],StructuralResult::Created { surface,.. } if *surface==id(IdKind::Surface,0))
            );
        }
        assert!(
            matches!(&results[1],StructuralResult::Created { workspace:Some(workspace), pane:Some(pane), tab:Some(tab), surface } if *workspace==id(IdKind::Workspace,1) && *pane==id(IdKind::Pane,1) && *tab==id(IdKind::Tab,1) && *surface==id(IdKind::Surface,1))
        );
    }
}
