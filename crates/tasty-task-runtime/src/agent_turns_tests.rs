use super::*;

fn binding(task: &str, attempt: &str, armed: bool) -> TurnBinding {
    TurnBinding::new(1, task.into(), attempt.into(), "claude".into(), armed)
}

fn answer(s: &str) -> TurnEvent {
    TurnEvent::Ended(TurnEnd::Answer(Some(s.into())))
}

#[test]
fn a_second_task_cannot_bind_a_bound_surface() {
    let t = AgentTurns::new();
    t.bind(7, binding("a", "a#1", false)).unwrap();
    assert_eq!(t.bind(7, binding("b", "b#1", false)), Err("a".to_string()));
    // 같은 회차를 다시 묶는 것은 허용한다.
    t.bind(7, binding("a", "a#1", false)).unwrap();
    t.release(&"a".to_string());
    t.bind(7, binding("b", "b#1", false)).unwrap();
    assert_eq!(t.holder(7).as_deref(), Some("b"));
}

#[test]
fn an_end_before_the_turn_started_belongs_to_the_previous_turn() {
    let t = AgentTurns::new();
    t.bind(7, binding("a", "a#1", false)).unwrap();
    assert_eq!(
        t.report(7, "claude", answer("old")),
        ReportOutcome::Ignored("the bound turn has not started yet")
    );
    assert!(matches!(
        t.report(7, "claude", TurnEvent::Started),
        ReportOutcome::Applied { .. }
    ));
    assert!(matches!(
        t.report(7, "claude", answer("new")),
        ReportOutcome::Applied { .. }
    ));
    assert_eq!(
        t.get(7).unwrap().ended,
        Some(TurnEnd::Answer(Some("new".into())))
    );
    // 두 번째 종료는 첫 종료를 바꾸지 않는다.
    assert_eq!(
        t.report(7, "claude", answer("later")),
        ReportOutcome::Ignored("the bound turn already ended")
    );
}

#[test]
fn another_provider_and_unbound_surfaces_do_not_apply() {
    let t = AgentTurns::new();
    assert_eq!(
        t.report(9, "claude", TurnEvent::Started),
        ReportOutcome::Unbound
    );
    t.bind(7, binding("a", "a#1", true)).unwrap();
    assert_eq!(
        t.report(7, "codex", answer("x")),
        ReportOutcome::Ignored("the surface is bound to another provider's turn")
    );
    assert!(t.get(7).unwrap().ended.is_none());
}

#[test]
fn a_released_binding_takes_no_late_report() {
    let t = AgentTurns::new();
    t.bind(7, binding("a", "a#1", true)).unwrap();
    t.release(&"a".to_string());
    assert_eq!(
        t.report(7, "claude", answer("late")),
        ReportOutcome::Unbound
    );
}

#[test]
fn submissions_are_tied_to_the_current_attempt_and_keep_the_first_value() {
    let t = AgentTurns::new();
    assert_eq!(
        t.submit(&"a".into(), "a#1", json!(1)),
        Err((SubmissionRejection::NotRunning, None))
    );
    t.bind(7, binding("a", "a#2", true)).unwrap();
    assert_eq!(
        t.submit(&"a".into(), "a#1", json!(1)),
        Err((SubmissionRejection::StaleAttempt, Some("a#2".into())))
    );
    assert_eq!(
        t.submit(&"b".into(), "a#2", json!(1)),
        Err((SubmissionRejection::NotRunning, None))
    );
    assert_eq!(
        t.submit(&"a".into(), "a#2", json!({"v": 1})),
        Ok(SubmitOutcome::Accepted)
    );
    assert_eq!(
        t.submit(&"a".into(), "a#2", json!({"v": 1})),
        Ok(SubmitOutcome::Duplicate)
    );
    assert_eq!(
        t.submit(&"a".into(), "a#2", json!({"v": 2})),
        Err((SubmissionRejection::Conflict, Some("a#2".into())))
    );
    t.report(7, "claude", answer("done"));
    assert_eq!(
        t.submit(&"a".into(), "a#2", json!({"v": 1})),
        Err((SubmissionRejection::TurnEnded, Some("a#2".into())))
    );
}

#[test]
fn idle_without_a_turn_end_report_stays_active_and_needs_input_waits() {
    let b = binding("a", "a#1", true);
    assert_eq!(
        decide(&b, 7, false, Some("idle"), 0, None),
        TurnPoll::Active {
            awaiting_input: false
        }
    );
    assert_eq!(
        decide(&b, 7, false, Some("needs_input"), 0, None),
        TurnPoll::Active {
            awaiting_input: true
        }
    );
}

#[test]
fn turn_end_without_the_required_submission_is_result_missing() {
    let mut b = binding("a", "a#1", true);
    b.ended = Some(TurnEnd::Answer(Some("I reviewed it".into())));
    let TurnPoll::Failed(msg) = decide(&b, 7, true, Some("idle"), 0, None) else {
        panic!("expected failure");
    };
    assert_eq!(
        FailureCode::parse_message(&msg),
        Some(FailureCode::ResultMissing)
    );
    // string 출력이면 최종 답변이 결과다.
    let TurnPoll::Done(r) = decide(&b, 7, false, Some("idle"), 0, None) else {
        panic!("expected done");
    };
    assert_eq!(r.output.unwrap()["final_answer"], json!("I reviewed it"));
}

#[test]
fn a_submission_wins_over_the_final_answer() {
    let mut b = binding("a", "a#1", true);
    b.submitted = Some(json!({"verdict": "revise"}));
    b.ended = Some(TurnEnd::Answer(Some("text".into())));
    let TurnPoll::Done(r) = decide(&b, 7, true, None, 0, None) else {
        panic!("expected done");
    };
    let out = r.output.unwrap();
    assert_eq!(out["submitted"], json!({"verdict": "revise"}));
    assert_eq!(out["final_answer"], json!("text"));
}

#[test]
fn exit_timeout_and_turn_error_have_distinct_codes() {
    let b = binding("a", "a#1", true);
    let code = |p: TurnPoll| match p {
        TurnPoll::Failed(m) => FailureCode::parse_message(&m),
        _ => None,
    };
    assert_eq!(
        code(decide(&b, 7, false, Some("exited"), 0, None)),
        Some(FailureCode::AgentExited)
    );
    assert_eq!(
        code(decide(&b, 7, false, Some("active"), 10, Some(10))),
        Some(FailureCode::TimedOut)
    );
    let mut e = b.clone();
    e.ended = Some(TurnEnd::Error("overloaded".into()));
    assert_eq!(
        code(decide(&e, 7, false, Some("idle"), 0, None)),
        Some(FailureCode::AgentTurnError)
    );
}

#[test]
fn awaiting_input_is_recorded_once_per_change() {
    let t = AgentTurns::new();
    let a: TaskId = "a".into();
    t.bind(7, binding("a", "a#1", true)).unwrap();
    assert_eq!(t.note_awaiting(&a, false, 5), Some(None), "첫 기록");
    assert_eq!(t.note_awaiting(&a, false, 6), None);
    assert_eq!(t.note_awaiting(&a, true, 7), Some(Some(7)));
    assert_eq!(
        t.note_awaiting(&a, true, 9),
        None,
        "대기 시작 시각은 유지한다"
    );
    assert_eq!(t.note_awaiting(&a, false, 10), Some(None));
}
