use super::*;
#[test]
fn backend_business_errors_partial_cleanup_and_unknown_are_not_transport_success() {
    assert!(matches!(
        classify_result(&Ok::<_, String>(())).outcome,
        command::CommandOutcome::KnownSuccess
    ));
    let mut error = Failure::before("refused");
    let disposition = classify_failure(&Err::<(), _>(error.clone()));
    assert!(matches!(
        disposition.outcome,
        command::CommandOutcome::KnownFailure
    ));
    assert!(!disposition.partial);
    error.applied = true;
    assert!(classify_failure(&Err::<(), _>(error.clone())).partial);
    error.applied = false;
    error.saved = None;
    error.details = serde_json::json!({"rollback_failed":true});
    let disposition = classify_failure(&Err::<(), _>(error));
    assert!(matches!(
        disposition.outcome,
        command::CommandOutcome::Unknown
    ));
    assert!(disposition.partial);
}
