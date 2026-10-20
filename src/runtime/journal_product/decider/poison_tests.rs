use super::*;
use std::sync::atomic::Ordering;

fn poison<T>(lock: &std::sync::Mutex<T>) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _held = lock.lock().expect("fresh lock");
        panic!("poison the journal metadata lock");
    }));
    assert!(result.is_err());
}

#[test]
fn poisoned_preservation_metadata_keeps_ids_and_accepts_retry_updates() {
    let decider = StructureDecider::default();
    decider.remember_unpreserved(7);
    poison(&decider.unpreserved);
    assert_eq!(decider.unpreserved(), [7].into());
    decider.remember_unpreserved(9);
    assert_eq!(decider.unpreserved(), [7, 9].into());
    decider.forget_unpreserved(7);
    assert_eq!(decider.unpreserved(), [9].into());
    assert!(decider.unpreserved_poisoned.load(Ordering::Relaxed));
}

#[test]
fn poisoned_load_report_keeps_damage_until_a_whole_report_replaces_it() {
    let decider = StructureDecider::default();
    let report = journal::LoadReport {
        damaged: [("structure:slot-1".into(), "broken batch".into())].into(),
        ..Default::default()
    };
    *decider.report.lock().expect("fresh report") = report.clone();
    poison(&decider.report);
    assert_eq!(decider.report(), report);
    assert!(!decider.report().is_clean());
    assert!(decider.report_poisoned.load(Ordering::Relaxed));
    *decider.report_guard() = journal::LoadReport::default();
    assert!(decider.report().is_clean());
}
