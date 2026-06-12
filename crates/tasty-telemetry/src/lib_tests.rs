//! `lib_tests` 단위 테스트.

use std::str::FromStr;

use crate::*;

#[test]
fn metric_validation() {
    assert!(validate_metric("input_tokens").is_ok());
    assert!(validate_metric("a").is_ok());
    assert!(validate_metric("a1_b").is_ok());
    assert!(validate_metric("").is_err());
    assert!(validate_metric("Input_tokens").is_err()); // 대문자
    assert!(validate_metric("1abc").is_err()); // 숫자 시작
    assert!(validate_metric("ab.cd").is_err()); // dot 금지
    assert!(validate_metric(&"x".repeat(65)).is_err()); // 길이
}

#[test]
fn agent_validation() {
    assert!(validate_agent_id("_host").is_ok());
    assert!(validate_agent_id("claude_s42").is_ok());
    assert!(validate_agent_id("Codex-1").is_ok());
    assert!(validate_agent_id("").is_err());
    assert!(validate_agent_id("a b").is_err());
    assert!(validate_agent_id("a.b").is_err());
}

#[test]
fn event_key_format() {
    let k = event_key(1_700_000_000_000, 5);
    assert_eq!(k, "tasty.telemetry.event.1700000000000.0005");
    // ts 가 13자 zero-pad 인지
    assert!(k.starts_with("tasty.telemetry.event."));
    assert_eq!(k.len(), "tasty.telemetry.event.1700000000000.0005".len());
}

#[test]
fn bucket_key_format() {
    let k = bucket_key(
        Window::OneMinute,
        "input_tokens",
        "claude_s1",
        1_700_000_000_000,
    );
    assert_eq!(
        k,
        "tasty.telemetry.bucket.1m.input_tokens.claude_s1.1700000000000"
    );
}

#[test]
fn op_signed() {
    assert_eq!(Op::Inc.signed(5.0), 5.0);
    assert_eq!(Op::Dec.signed(5.0), -5.0);
    assert_eq!(Op::Set.signed(5.0), 5.0);
}

#[test]
fn window_align() {
    assert_eq!(Window::OneMinute.align(123_456), 120_000);
    assert_eq!(Window::OneHour.align(3_700_000), 3_600_000);
    assert_eq!(Window::OneDay.align(86_400_001), 86_400_000);
}

#[test]
fn summarize_basic() {
    let evs = vec![
        TelemetryEvent::new("a", "input_tokens", 100.0, Op::Inc, 1000).unwrap(),
        TelemetryEvent::new("a", "input_tokens", 50.0, Op::Inc, 2000).unwrap(),
        TelemetryEvent::new("a", "input_tokens", 200.0, Op::Set, 3000).unwrap(),
        TelemetryEvent::new("b", "input_tokens", 30.0, Op::Inc, 1500).unwrap(),
    ];
    let s = summarize_events(evs);
    assert_eq!(s.len(), 2);
    let a = s.iter().find(|x| x.agent == "a").unwrap();
    assert_eq!(a.count, 3);
    // sum: Set은 sum을 통째 교체 → 마지막이 Set(200)이라 200
    assert_eq!(a.sum, 200.0);
    assert_eq!(a.last, 200.0);
    assert_eq!(a.max, 200.0);
    assert_eq!(a.min, 50.0);
}

#[test]
fn summarize_inc_only() {
    let evs = vec![
        TelemetryEvent::new("a", "files_read", 1.0, Op::Inc, 1000).unwrap(),
        TelemetryEvent::new("a", "files_read", 1.0, Op::Inc, 1001).unwrap(),
        TelemetryEvent::new("a", "files_read", 1.0, Op::Inc, 1002).unwrap(),
    ];
    let s = summarize_events(evs);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].sum, 3.0);
    assert_eq!(s[0].count, 3);
}

#[test]
fn aggregate_buckets_1m() {
    let evs = vec![
        // 1분 윈도우 0~60s
        TelemetryEvent::new("a", "m", 1.0, Op::Inc, 10_000).unwrap(),
        TelemetryEvent::new("a", "m", 2.0, Op::Inc, 30_000).unwrap(),
        // 다른 분 윈도우 60~120s
        TelemetryEvent::new("a", "m", 5.0, Op::Inc, 70_000).unwrap(),
    ];
    let buckets = aggregate_into_buckets(evs, Window::OneMinute);
    assert_eq!(buckets.len(), 2);
    assert_eq!(buckets[0].window_start, 0);
    assert_eq!(buckets[0].sum, 3.0);
    assert_eq!(buckets[0].count, 2);
    assert_eq!(buckets[1].window_start, 60_000);
    assert_eq!(buckets[1].sum, 5.0);
}

#[test]
fn top_by_agent() {
    let evs = vec![
        TelemetryEvent::new("a", "m", 1000.0, Op::Inc, 1).unwrap(),
        TelemetryEvent::new("b", "m", 500.0, Op::Inc, 1).unwrap(),
        TelemetryEvent::new("a", "m", 100.0, Op::Inc, 2).unwrap(),
    ];
    let t = top_n(evs, "agent", 5);
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].key, "a");
    assert_eq!(t[0].sum, 1100.0);
    assert_eq!(t[1].key, "b");
}

#[test]
fn op_from_str() {
    assert!(matches!(Op::from_str("set"), Ok(Op::Set)));
    assert!(matches!(Op::from_str("inc"), Ok(Op::Inc)));
    assert!(matches!(Op::from_str("dec"), Ok(Op::Dec)));
    assert!(Op::from_str("nope").is_err());
}

#[test]
fn window_from_str() {
    assert!(matches!(Window::from_str("1m"), Ok(Window::OneMinute)));
    assert!(matches!(Window::from_str("1h"), Ok(Window::OneHour)));
    assert!(matches!(Window::from_str("1d"), Ok(Window::OneDay)));
    assert!(Window::from_str("2m").is_err());
}

#[test]
fn cap_window_parse() {
    assert!(matches!(CapWindow::from_str("total"), Ok(CapWindow::Total)));
    assert!(matches!(CapWindow::from_str("1h"), Ok(CapWindow::Hour)));
    assert!(matches!(CapWindow::from_str("1d"), Ok(CapWindow::Day)));
    assert!(CapWindow::from_str("1m").is_err());
    assert_eq!(CapWindow::Total.span_ms(), None);
    assert_eq!(CapWindow::Hour.span_ms(), Some(3_600_000));
}

#[test]
fn cap_action_parse() {
    assert!(matches!(CapAction::from_str("stop"), Ok(CapAction::Stop)));
    assert!(matches!(CapAction::from_str("pause"), Ok(CapAction::Pause)));
    assert!(matches!(
        CapAction::from_str("require_approval"),
        Ok(CapAction::RequireApproval)
    ));
    assert!(matches!(
        CapAction::from_str("notify"),
        Ok(CapAction::Notify)
    ));
    assert!(CapAction::from_str("bogus").is_err());
}

#[test]
fn cap_key_format() {
    assert_eq!(cap_key("cap_abc"), "tasty.telemetry.cap.cap_abc");
}

#[test]
fn telemetry_seq_monotonic() {
    let s = TelemetrySeq::new();
    assert_eq!(s.next(), 0);
    assert_eq!(s.next(), 1);
    assert_eq!(s.next(), 2);
}

#[test]
fn anomaly_key_format() {
    let k = anomaly_key(1_700_000_000_000, "anom_xyz");
    assert!(k.starts_with("tasty.telemetry.anomaly."));
    assert!(k.contains("1700000000000"));
    assert!(k.ends_with(".anom_xyz"));
}

#[test]
fn anomaly_kind_token() {
    assert_eq!(AnomalyKind::CallBurst.as_token(), "call_burst");
    assert_eq!(AnomalyKind::SlowLoop.as_token(), "slow_loop");
    assert_eq!(AnomalyKind::RssSurge.as_token(), "rss_surge");
}

#[test]
fn anomaly_detector_under_threshold_silent() {
    let d = AnomalyDetector::new();
    for i in 0..(CALL_BURST_THRESHOLD - 1) {
        assert!(
            d.record_call("agent_a", "ipc.foo", 1_000 + i as u64, i as u64)
                .is_none()
        );
    }
}

#[test]
fn anomaly_detector_burst_fires() {
    let d = AnomalyDetector::new();
    for i in 0..(CALL_BURST_THRESHOLD - 1) {
        assert!(
            d.record_call("agent_a", "ipc.foo", 1_000 + i as u64, i as u64)
                .is_none()
        );
    }
    // 임계 번째 호출에서 발화.
    let a = d
        .record_call(
            "agent_a",
            "ipc.foo",
            1_000 + CALL_BURST_THRESHOLD as u64,
            9999,
        )
        .expect("anomaly fired");
    assert_eq!(a.kind, AnomalyKind::CallBurst);
    assert_eq!(a.agent, "agent_a");
    assert_eq!(a.subject, "ipc.foo");
    assert!(a.detail["count"].as_u64().unwrap() >= CALL_BURST_THRESHOLD as u64);
}

#[test]
fn anomaly_detector_dedup_within_cooldown() {
    let d = AnomalyDetector::new();
    // 1초 간격으로 임계 회 호출 → 발화.
    let base = 0u64;
    for i in 0..CALL_BURST_THRESHOLD {
        let _ = d.record_call("a", "m", base + i as u64, i as u64);
    }
    // 직후 동일 호출 — dedup 으로 None.
    let after = d.record_call("a", "m", base + CALL_BURST_THRESHOLD as u64 + 1, 1);
    assert!(
        after.is_none(),
        "second burst within cooldown must be deduped"
    );
}

#[test]
fn anomaly_detector_window_slides() {
    let d = AnomalyDetector::new();
    // 윈도우보다 오래된 호출은 evict 돼야 함.
    for i in 0..(CALL_BURST_THRESHOLD - 1) {
        let _ = d.record_call("a", "m", i as u64, i as u64);
    }
    // 윈도우 밖에서 다시 1회 → 윈도우 카운트는 1 이라 trigger 안 됨.
    let far = CALL_BURST_WINDOW_MS + 1_000;
    assert!(d.record_call("a", "m", far, 9999).is_none());
}
