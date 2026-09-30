use super::*;

fn published_result(worker: &JournalWorker, ticket: u64) -> ResultValue {
    loop {
        match receive(worker) {
            Completion::Publish { batch, .. } => {
                worker.acknowledge(batch.batch_id, Ok(())).unwrap()
            }
            Completion::Finished {
                ticket: got,
                result,
            } => {
                assert_eq!(got, ticket);
                return result.unwrap();
            }
            other => panic!("unexpected retirement result: {other:?}"),
        }
    }
}

#[test]
fn stored_retirement_does_not_delete_a_reused_slots_new_export() {
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
    let open = || Work::OpenEngine {
        selection: EngineSelection::Slot {
            slot: 1,
            resume: true,
        },
        normal_category_name: "Normal".into(),
        surface_floor: 0,
    };
    submit(&worker, 1, open());
    let ResultValue::Bound(first) = published_result(&worker, 1) else {
        panic!("bound");
    };
    std::fs::create_dir(home.path().join("layouts")).unwrap();
    let export = home.path().join("layouts/01.json");
    std::fs::write(&export, "old incarnation").unwrap();
    submit(&worker, 2, Work::RetireEngine(first.binding.clone()));
    let ResultValue::Executed(retired) = published_result(&worker, 2) else {
        panic!("retired");
    };
    assert!(matches!(
        retired.source,
        crate::runtime::command_executor::Source::Committed { .. }
    ));
    assert!(!export.exists());
    submit(&worker, 3, open());
    let ResultValue::Bound(second) = published_result(&worker, 3) else {
        panic!("reused");
    };
    assert_eq!(second.binding.incarnation, first.binding.incarnation + 1);
    std::fs::write(&export, "new incarnation").unwrap();
    submit(&worker, 2, Work::RetireEngine(first.binding));
    let ResultValue::Executed(replayed) = finished(&worker, 2).unwrap() else {
        panic!("stored");
    };
    assert_eq!(
        replayed.source,
        crate::runtime::command_executor::Source::Stored
    );
    assert_eq!(replayed.response, retired.response);
    assert_eq!(std::fs::read_to_string(export).unwrap(), "new incarnation");
    submit(&worker, 4, Work::ReadEngine(second.binding.stream));
    let ResultValue::Engine(model) = finished(&worker, 4).unwrap() else {
        panic!("model");
    };
    assert_eq!(model.engine_incarnation, second.binding.incarnation);
    assert!(!model.engine_retired);
}
