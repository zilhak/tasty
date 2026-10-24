use crate::{AnomalyDetector, AnomalyKind, AnomalyThresholds, MIB};
fn small() -> AnomalyThresholds {
    AnomalyThresholds {
        rss_surge_min_bytes: 0,
        ..Default::default()
    }
}
#[test]
fn default_minimum_filters_small_rss_and_accepts_inclusive_boundary() {
    let d = AnomalyDetector::new();
    for i in 0..6 {
        assert!(
            d.record_rss_sample("small", 4_000_000 + i, 1000 + i, i)
                .is_none()
        );
    }
    for i in 0..4 {
        assert!(
            d.record_rss_sample("boundary", 256 * MIB - 4 + i, 1000 + i, i)
                .is_none()
        );
    }
    assert!(
        d.record_rss_sample("boundary", 256 * MIB, 1004, 4)
            .is_some()
    );
    d.set_thresholds(small()).unwrap();
    for i in 0..4 {
        assert!(d.record_rss_sample("zero", 100 + i, 1000 + i, i).is_none());
    }
    assert!(d.record_rss_sample("zero", 104, 1004, 4).is_some());
}
#[test]
fn lowering_call_threshold_uses_existing_history_and_invalid_updates_are_rejected() {
    let d = AnomalyDetector::new();
    let params = serde_json::json!({});
    for i in 0..9 {
        d.record_call("a", "test", &params, 1000 + i, i);
    }
    let t = AnomalyThresholds {
        call_burst_threshold: 10,
        ..Default::default()
    };
    d.set_thresholds(t).unwrap();
    assert!(
        d.set_thresholds(AnomalyThresholds {
            call_burst_threshold: 0,
            ..t
        })
        .is_err()
    );
    assert!(
        d.record_call("a", "test", &params, 1010, 10)
            .iter()
            .any(|a| a.kind == AnomalyKind::CallBurst)
    );
}
#[test]
fn shrinking_then_growing_a_window_does_not_recover_trimmed_history() {
    let d = AnomalyDetector::new();
    let params = serde_json::json!({});
    let t = AnomalyThresholds {
        call_burst_threshold: 10,
        ..Default::default()
    };
    d.set_thresholds(t).unwrap();
    for i in 0..9 {
        d.record_call("a", "test", &params, 1000 + i, i);
    }
    d.set_thresholds(AnomalyThresholds {
        call_burst_window_ms: 1000,
        ..t
    })
    .unwrap();
    assert!(
        !d.record_call("a", "test", &params, 3000, 20)
            .iter()
            .any(|a| a.kind == AnomalyKind::CallBurst)
    );
    d.set_thresholds(t).unwrap();
    assert!(
        !d.record_call("a", "test", &params, 3001, 21)
            .iter()
            .any(|a| a.kind == AnomalyKind::CallBurst)
    );
}
#[test]
fn loop_episode_survives_edits_until_observations_break_the_new_threshold() {
    let d = AnomalyDetector::new();
    let params = serde_json::json!({});
    let t = AnomalyThresholds {
        slow_loop_threshold: 2,
        slow_loop_window_ms: 300_000,
        ..Default::default()
    };
    d.set_thresholds(t).unwrap();
    d.record_call("a", "test", &params, 1000, 0);
    assert!(
        d.record_call("a", "test", &params, 1001, 1)
            .iter()
            .any(|a| a.kind == AnomalyKind::SlowLoop)
    );
    d.set_thresholds(AnomalyThresholds {
        slow_loop_threshold: 3,
        ..t
    })
    .unwrap();
    assert!(
        !d.record_call("a", "test", &params, 2000, 10)
            .iter()
            .any(|a| a.kind == AnomalyKind::SlowLoop),
        "changing criteria alone must preserve the continuing episode"
    );
    assert!(
        !d.record_call("a", "test", &params, 62_000, 2)
            .iter()
            .any(|a| a.kind == AnomalyKind::SlowLoop)
    );
    d.record_call("a", "test", &params, 400_000, 12);
    d.record_call("a", "test", &params, 400_001, 3);
    assert!(
        d.record_call("a", "test", &params, 400_002, 4)
            .iter()
            .any(|a| a.kind == AnomalyKind::SlowLoop)
    );
}
#[test]
fn rss_sample_resize_and_threshold_edits_keep_reported_episode_state() {
    let d = AnomalyDetector::new();
    let t = small();
    d.set_thresholds(t).unwrap();
    for i in 0..3 {
        assert!(d.record_rss_sample("a", 100 + i, 1000 + i, i).is_none());
    }
    d.set_thresholds(AnomalyThresholds {
        rss_surge_samples: 3,
        ..t
    })
    .unwrap();
    assert!(d.record_rss_sample("a", 103, 1003, 3).is_some());
    d.set_thresholds(AnomalyThresholds {
        rss_surge_samples: 5,
        rss_surge_min_bytes: 256 * MIB,
        ..t
    })
    .unwrap();
    assert!(d.record_rss_sample("a", 104, 62_000, 4).is_none());
    d.set_thresholds(t).unwrap();
    for i in 0..8 {
        assert!(d.record_rss_sample("a", 105 + i, 62_001 + i, i).is_none());
    }
    assert!(d.record_rss_sample("a", 100, 70_000, 20).is_none());
    for i in 0..3 {
        assert!(d.record_rss_sample("a", 101 + i, 70_001 + i, i).is_none());
    }
    assert!(d.record_rss_sample("a", 104, 70_004, 24).is_some());
}
