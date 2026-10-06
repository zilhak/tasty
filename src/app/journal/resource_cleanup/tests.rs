//! Which retirement phases pause observation, and what an in-flight retirement still holds.
use super::*;

fn journal_with_cleanup() -> (JournalApplication, EngineId, OperationId) {
    let mut journal = JournalApplication::new(std::sync::Arc::new(|| {})).unwrap();
    let engine = EngineId::for_test();
    let operation = OperationId("cmd-1-1/prepare/0".into());
    journal
        .queue_resource_retirement(engine, "structure:slot-1".into(), operation.clone())
        .unwrap();
    (journal, engine, operation)
}

fn set_phase(journal: &mut JournalApplication, phase: Phase) {
    for entry in journal.resource_cleanups.values_mut() {
        entry.phase = match &phase {
            Phase::Claim => Phase::Claim,
            Phase::Running => Phase::Running,
            Phase::Reconcile => Phase::Reconcile,
            Phase::Reconciled => Phase::Reconciled,
            Phase::Finish { uncertain } => Phase::Finish {
                uncertain: *uncertain,
            },
        };
    }
}

#[test]
fn only_claim_and_finish_pause_observation() {
    let (mut journal, _, operation) = journal_with_cleanup();
    assert!(journal.cleanup_pauses_observation(), "Claim");
    assert!(!journal.cleanup_awaits_receipts(&operation));
    set_phase(&mut journal, Phase::Running);
    assert!(!journal.cleanup_pauses_observation(), "Running");
    assert!(journal.cleanup_awaits_receipts(&operation));
    set_phase(&mut journal, Phase::Finish { uncertain: false });
    assert!(journal.cleanup_pauses_observation(), "Finish");
    assert!(!journal.cleanup_awaits_receipts(&operation));
    set_phase(&mut journal, Phase::Reconcile);
    assert!(!journal.cleanup_pauses_observation(), "Reconcile");
    assert!(!journal.cleanup_awaits_receipts(&operation));
}

/// Engine retirement waits for receipts independently of the observation pause.
#[test]
fn a_running_retirement_still_holds_its_engine() {
    let (mut journal, engine, _) = journal_with_cleanup();
    set_phase(&mut journal, Phase::Running);
    assert!(journal.has_resource_cleanup(engine));
    #[cfg(feature = "gui")]
    assert!(journal.has_pending_engine_effects(engine));
    assert!(!journal.is_ready(engine));
}

#[test]
fn another_operation_is_not_mistaken_for_one_awaiting_receipts() {
    let (mut journal, _, _) = journal_with_cleanup();
    set_phase(&mut journal, Phase::Running);
    assert!(!journal.cleanup_awaits_receipts(&OperationId("cmd-1-2/prepare/0".into())));
}
