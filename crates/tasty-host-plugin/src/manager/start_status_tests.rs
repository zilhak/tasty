//! 기동 세대별 결과 귀속과 실패 보고 묶음을 확인한다.

use std::time::{Duration, Instant};

use super::*;
use crate::manager::RESTART_FAILURE_WINDOW;

const ID: &str = "com.example.p";

fn fail(t: &mut StartTracker, auto: Option<usize>, now: Instant) {
    t.failed(ID, FailureKind::ConnectFailed, "gone".into(), auto, now);
}

/// 앞 기동의 늦은 실패는 뒤 기동의 결과를 바꾸지 않는다.
#[test]
fn a_late_failure_of_an_earlier_start_does_not_overwrite_the_newer_one() {
    let mut t = StartTracker::default();
    let a = t.begin(ID);
    t.spawned(ID, a);
    let b = t.begin(ID);
    assert_ne!(a, b);
    fail(&mut t, None, Instant::now());
    let s = &t.starts[ID];
    assert_eq!((s.generation, s.state), (b, StartState::Starting));
    assert_eq!(t.last_failure[ID].generation, a);
    t.spawned(ID, b);
    t.connected(ID);
    assert_eq!(t.starts[ID].state, StartState::Connected);
}

/// 띄우기 전에 회수를 기다리던 기동은 같은 세대로 이어진다.
#[test]
fn a_deferred_start_keeps_its_generation() {
    let mut t = StartTracker::default();
    let a = t.begin(ID);
    assert_eq!(t.begin(ID), a);
    t.spawned(ID, a);
    assert_ne!(t.begin(ID), a);
}

/// 결과 전에 거두면 취소로 끝나고, 끝난 기동은 취소로 바꾸지 않는다.
#[test]
fn a_start_withdrawn_before_its_result_is_cancelled() {
    let mut t = StartTracker::default();
    let a = t.begin(ID);
    t.spawned(ID, a);
    t.cancelled(ID);
    assert_eq!(t.starts[ID].state, StartState::Cancelled);
    t.connected(ID);
    assert_eq!(t.starts[ID].state, StartState::Cancelled);

    let b = t.begin(ID);
    t.spawned(ID, b);
    t.connected(ID);
    t.cancelled(ID);
    assert_eq!(t.starts[ID].state, StartState::Connected);
}

/// 실패 창 안의 이어지는 실패는 한 건으로 묶고, 자동 비활성화는 따로 한 건 보고한다.
#[test]
fn failures_in_one_window_are_reported_once_and_auto_disable_once_more() {
    let mut t = StartTracker::default();
    let now = Instant::now();
    for (i, auto) in [None, None, Some(3)].into_iter().enumerate() {
        let g = t.begin(ID);
        t.spawned(ID, g);
        fail(&mut t, auto, now + Duration::from_millis(i as u64));
    }
    let reports = std::mem::take(&mut t.reports);
    assert_eq!(reports.len(), 2, "{reports:?}");
    assert!(
        matches!(&reports[0], FailureReport::Failed { failure, .. } if failure.generation == 1)
    );
    assert!(matches!(
        &reports[1],
        FailureReport::AutoDisabled { failures: 3, failure, .. } if failure.generation == 3
    ));

    let g = t.begin(ID);
    t.spawned(ID, g);
    fail(&mut t, None, now + RESTART_FAILURE_WINDOW);
    assert_eq!(t.reports.len(), 1, "a new window reports again");
}

/// 연결에 성공하면 묶음이 끝나 다음 실패를 다시 보고한다.
#[test]
fn a_connection_ends_the_report_window() {
    let mut t = StartTracker::default();
    let now = Instant::now();
    let a = t.begin(ID);
    t.spawned(ID, a);
    fail(&mut t, None, now);
    let b = t.begin(ID);
    t.spawned(ID, b);
    t.connected(ID);
    let c = t.begin(ID);
    t.spawned(ID, c);
    fail(&mut t, None, now);
    assert_eq!(t.reports.len(), 2);
}
