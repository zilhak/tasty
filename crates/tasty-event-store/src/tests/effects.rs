//! effect 의무: 허용 전이·claim·attempt·늦은 결과 거절.

use crate::{
    ActivationClaim, CommitRequest, EffectState, EffectTransition, EventStore, ExpectedRevision,
    StoreError, WriterEpoch,
};

use super::common::{JOURNAL, append, command, db_path, fresh, new_effect, open};

use EffectState::*;

fn claim(generation: u64) -> ActivationClaim {
    ActivationClaim {
        engine_id: "engine-a".to_owned(),
        surface_id: Some("surface-3".to_owned()),
        runtime_epoch: 1,
        activation_generation: generation,
    }
}

fn step(id: &str, from: EffectState, to: EffectState) -> EffectTransition {
    EffectTransition {
        effect_id: id.to_owned(),
        from,
        to,
        resource_generation: 1,
        attempt: None,
        claim: None,
        result: None,
    }
}

fn run(id: &str, from: EffectState, generation: u64) -> EffectTransition {
    EffectTransition {
        claim: Some(claim(generation)),
        ..step(id, from, Running)
    }
}

fn finish(id: &str, to: EffectState, attempt: u32) -> EffectTransition {
    EffectTransition {
        attempt: Some(attempt),
        result: Some(b"result".to_vec()),
        ..step(id, Running, to)
    }
}

fn seed(store: &mut EventStore, epoch: WriterEpoch, id: &str, initial: EffectState) {
    let mut request = CommitRequest::new(epoch);
    request.effects.push(new_effect(id, 1, initial));
    store.commit(&request).expect("seed effect");
}

#[test]
fn effect_is_recorded_with_its_cause_batch() {
    let (_dir, mut store, epoch) = fresh();
    let mut request = CommitRequest::new(epoch);
    request.command = Some(command("cmd-1", None, b"d"));
    request.appends.push(append(
        "engine-a",
        ExpectedRevision::NoStream,
        &["pending-op"],
    ));
    request.effects.push(new_effect("fx-1", 1, Pending));
    store.commit(&request).expect("commit");

    let record = store.effect("fx-1").expect("read").expect("exists");
    assert_eq!(record.cause_batch_id, Some(1));
    assert_eq!(record.command_id.as_deref(), Some("cmd-1"));
    assert_eq!(record.state, Pending);
    assert_eq!(record.attempt, 0);
}

#[test]
fn lifecycle_records_attempts_and_claims() {
    let (_dir, mut store, epoch) = fresh();
    seed(&mut store, epoch, "fx-1", Pending);
    store
        .transition_effect(epoch, &run("fx-1", Pending, 1))
        .expect("run");
    store
        .transition_effect(epoch, &finish("fx-1", Failed, 1))
        .expect("fail");
    // 같은 effect identity의 다음 attempt. 같은 activation claim에 합류한다.
    store
        .transition_effect(epoch, &step("fx-1", Failed, Pending))
        .expect("retry");
    store
        .transition_effect(epoch, &run("fx-1", Pending, 1))
        .expect("rerun");
    store
        .transition_effect(epoch, &finish("fx-1", Succeeded, 2))
        .expect("succeed");

    let record = store.effect("fx-1").expect("read").expect("exists");
    assert_eq!(record.state, Succeeded);
    assert_eq!(record.attempt, 2);
    assert_eq!(record.result.as_deref(), Some(&b"result"[..]));
    let attempts = store.effect_attempts("fx-1").expect("attempts");
    let outcomes: Vec<_> = attempts.iter().map(|a| a.outcome).collect();
    assert_eq!(outcomes, [Some(Failed), Some(Succeeded)]);
    assert_eq!(attempts[0].journal_id, JOURNAL);
    assert_eq!(attempts[0].claim, claim(1));
    assert_eq!(attempts[0].writer_epoch, epoch);
}

#[test]
fn transitions_outside_the_table_are_rejected() {
    let (_dir, mut store, epoch) = fresh();
    let cases = [
        (Pending, Succeeded),
        (Pending, Uncertain),
        (Deferred, Succeeded),
        (Cancelled, Pending),
        (Superseded, Pending),
        (Succeeded, Pending),
        (Uncertain, Pending),
        (Uncertain, Superseded),
        (Running, Superseded),
        (Running, Cancelled),
    ];
    for (from, to) in cases {
        assert!(!from.can_transition_to(to), "{from:?} -> {to:?}");
    }

    seed(&mut store, epoch, "fx-1", Pending);
    let err = store
        .transition_effect(epoch, &step("fx-1", Pending, Succeeded))
        .expect_err("pending cannot succeed without running");
    assert!(
        matches!(err, StoreError::InvalidEffectTransition { .. }),
        "{err:?}"
    );

    store
        .transition_effect(epoch, &step("fx-1", Pending, Cancelled))
        .expect("cancel");
    let err = store
        .transition_effect(epoch, &step("fx-1", Cancelled, Pending))
        .expect_err("cancelled is final");
    assert!(
        matches!(err, StoreError::InvalidEffectTransition { .. }),
        "{err:?}"
    );

    // 저장된 상태와 다른 전제는 CAS 실패다.
    let err = store
        .transition_effect(epoch, &run("fx-1", Pending, 1))
        .expect_err("state mismatch");
    assert!(
        matches!(
            err,
            StoreError::EffectStateMismatch {
                actual: Cancelled,
                ..
            }
        ),
        "{err:?}"
    );

    let err = store
        .transition_effect(epoch, &step("nope", Pending, Cancelled))
        .expect_err("unknown");
    assert!(matches!(err, StoreError::UnknownEffect(_)));

    let mut bad = CommitRequest::new(epoch);
    bad.effects.push(new_effect("fx-2", 1, Running));
    assert!(matches!(
        store.commit(&bad),
        Err(StoreError::InvalidInitialEffectState { .. })
    ));
}

#[test]
fn uncertain_leaves_only_through_reconciliation() {
    let (_dir, mut store, epoch) = fresh();
    seed(&mut store, epoch, "fx-1", Pending);
    store
        .transition_effect(epoch, &run("fx-1", Pending, 1))
        .expect("run");
    store
        .transition_effect(epoch, &finish("fx-1", Uncertain, 1))
        .expect("unknown");
    assert!(
        store
            .transition_effect(epoch, &step("fx-1", Uncertain, Pending))
            .is_err()
    );
    store
        .transition_effect(epoch, &step("fx-1", Uncertain, Succeeded))
        .expect("reconciled");
}

#[test]
fn late_results_from_old_attempts_or_generations_are_rejected() {
    let (_dir, mut store, epoch) = fresh();
    seed(&mut store, epoch, "fx-1", Pending);
    store
        .transition_effect(epoch, &run("fx-1", Pending, 1))
        .expect("run");

    let err = store
        .transition_effect(epoch, &finish("fx-1", Succeeded, 0))
        .expect_err("old attempt");
    assert!(
        matches!(err, StoreError::StaleAttempt { current: 1, .. }),
        "{err:?}"
    );

    let err = store
        .transition_effect(
            epoch,
            &EffectTransition {
                resource_generation: 2,
                ..finish("fx-1", Succeeded, 1)
            },
        )
        .expect_err("other generation");
    assert!(
        matches!(err, StoreError::StaleGeneration { current: 1, .. }),
        "{err:?}"
    );

    let err = store
        .transition_effect(
            epoch,
            &EffectTransition {
                attempt: None,
                ..finish("fx-1", Succeeded, 1)
            },
        )
        .expect_err("missing attempt");
    assert!(
        matches!(
            err,
            StoreError::StaleAttempt {
                presented: None,
                ..
            }
        ),
        "{err:?}"
    );
    assert_eq!(
        store.effect("fx-1").expect("read").expect("exists").state,
        Running
    );
}

#[test]
fn one_activation_generation_has_one_claim_holder() {
    let (_dir, mut store, epoch) = fresh();
    seed(&mut store, epoch, "fx-1", Pending);
    seed(&mut store, epoch, "fx-2", Pending);
    store
        .transition_effect(epoch, &run("fx-1", Pending, 1))
        .expect("first claim");

    let err = store
        .transition_effect(epoch, &run("fx-2", Pending, 1))
        .expect_err("same generation, other effect");
    assert!(
        matches!(err, StoreError::ClaimHeld { ref holder, .. } if holder == "fx-1"),
        "{err:?}"
    );
    assert!(store.effect_attempts("fx-2").expect("attempts").is_empty());

    // 새 activation generation은 별도 실행권이다.
    store
        .transition_effect(epoch, &run("fx-2", Pending, 2))
        .expect("new generation");

    seed(&mut store, epoch, "fx-3", Pending);
    let err = store
        .transition_effect(epoch, &step("fx-3", Pending, Running))
        .expect_err("claim required");
    assert!(matches!(err, StoreError::ClaimRequired(_)));
}

#[test]
fn deferred_effects_wait_for_explicit_activation() {
    let (_dir, mut store, epoch) = fresh();
    seed(&mut store, epoch, "lazy", Deferred);
    seed(&mut store, epoch, "now", Pending);
    let pending: Vec<_> = store
        .effects_in_state(Pending)
        .expect("pending")
        .into_iter()
        .map(|e| e.effect_id)
        .collect();
    assert_eq!(pending, ["now"]);
    store
        .transition_effect(epoch, &run("lazy", Deferred, 1))
        .expect("activate");
}

#[test]
fn running_state_survives_restart_for_recovery() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    seed(&mut store, epoch, "fx-1", Pending);
    store
        .transition_effect(epoch, &run("fx-1", Pending, 1))
        .expect("run");
    drop(store);

    let (mut store, epoch) = open(&path);
    let running = store.effects_in_state(Running).expect("running");
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].attempt, 1);
    store
        .transition_effect(epoch, &finish("fx-1", Uncertain, 1))
        .expect("recovery marks unknown result");
}

#[test]
fn failed_transition_rolls_back_the_whole_commit() {
    let (_dir, mut store, epoch) = fresh();
    seed(&mut store, epoch, "fx-1", Pending);
    let mut request = CommitRequest::new(epoch);
    request
        .appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["e1"]));
    request
        .effect_transitions
        .push(step("fx-1", Pending, Succeeded));
    store.commit(&request).expect_err("invalid transition");
    assert_eq!(store.current_cut().expect("cut").last_batch, None);
    assert_eq!(
        store.effect("fx-1").expect("read").expect("exists").state,
        Pending
    );
}
