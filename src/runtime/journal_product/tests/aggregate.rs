use super::*;
use tasty_domain::{
    CreationDestination, CreationPlan, DomainEvent, OperationId, PreparationResult,
    StructuralResult, SurfaceSpec,
};

#[test]
fn a_multi_stream_creation_completes_only_after_every_operation_and_preserves_result_order() {
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
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
            response: None,
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
                lease: claimed.lease,
            },
        );
        publish(&worker);
        finished(&worker, 12).unwrap();
        submit(&worker, 13, Work::Admit(header("two-workspaces")));
        let ResultValue::Stored(record) = finished(&worker, 13).unwrap() else {
            panic!("record")
        };
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
