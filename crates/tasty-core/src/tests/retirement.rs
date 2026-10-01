use super::common::{batch, scenario_batches};
use crate::*;

fn initial() -> JournalModel {
    let mut model = JournalModel::default();
    evolve(&mut model, &scenario_batches()[0]).unwrap();
    model
}
fn close(local: bool, remote: bool) -> StructuralCommand {
    StructuralCommand::Close {
        operation: OperationId("close/1".into()),
        command_id: "request/1".into(),
        input: DataRef(20),
        target: CloseTarget::Surface(1),
        expected: vec![RetiredSurface {
            id: 1,
            kind: "terminal".into(),
            activation_generation: None,
        }],
        undo: Some(UndoCapture {
            snapshot: DataRef(21),
            retained: vec![],
        }),
        is_user_close: local,
        remote_user_close: remote,
    }
}

#[test]
fn remote_user_close_keeps_undo_without_a_local_user_lifecycle() {
    let mut model = initial();
    let decision = decide_structure(&model, &close(false, true)).unwrap();
    let revision = model.applied.revision.unwrap();
    evolve(&mut model, &batch(2, revision, &decision.events)).unwrap();
    let plan = model.operations[&OperationId("close/1".into())]
        .retirement
        .as_ref()
        .unwrap();
    assert!(!plan.is_user_close, "plugin lifecycle keeps Remote origin");
    assert!(plan.remote_user_close);
    assert_eq!(model.undo_records.len(), 1);
    assert!(plan.undo.is_some());
    let event = &decision.events[0];
    let encoded = encode_event(event).unwrap();
    assert_eq!(
        decode_event(&encoded.type_tag, encoded.schema_version, &encoded.bytes).unwrap(),
        *event
    );
}

#[test]
fn agent_cannot_append_undo_and_user_origins_cannot_overlap() {
    assert!(decide_structure(&initial(), &close(false, false)).is_err());
    assert!(decide_structure(&initial(), &close(true, true)).is_err());
}

#[test]
fn old_close_and_retirement_data_keep_their_local_user_meaning() {
    let command = close(true, false);
    let mut command_json = serde_json::to_value(&command).unwrap();
    command_json["Close"]
        .as_object_mut()
        .unwrap()
        .remove("remote_user_close");
    let restored: StructuralCommand = serde_json::from_value(command_json).unwrap();
    assert_eq!(restored, command);
    let decision = decide_structure(&initial(), &restored).unwrap();
    let DomainEvent::OperationPrepared { operation } = &decision.events[0] else {
        panic!("preparation fact")
    };
    let plan = operation.retirement.as_ref().unwrap();
    let mut plan_json = serde_json::to_value(plan).unwrap();
    plan_json
        .as_object_mut()
        .unwrap()
        .remove("remote_user_close");
    let restored_plan: RetirementPlan = serde_json::from_value(plan_json).unwrap();
    assert!(restored_plan.is_user_close);
    assert!(!restored_plan.remote_user_close);
}
