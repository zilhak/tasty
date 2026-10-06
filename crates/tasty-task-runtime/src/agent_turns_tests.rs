use super::*;

const TOKEN: &str = "tok";

fn binding(task: &str, attempt: &str, armed: bool) -> TurnBinding {
    TurnBinding::new(
        1,
        task.into(),
        attempt.into(),
        "claude".into(),
        TOKEN.into(),
        armed,
    )
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
    t.release(1, &"a".to_string());
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
    t.release(1, &"a".to_string());
    assert_eq!(
        t.report(7, "claude", answer("late")),
        ReportOutcome::Unbound
    );
}

#[test]
fn submissions_are_tied_to_the_current_attempt_and_keep_the_first_value() {
    let t = AgentTurns::new();
    assert_eq!(
        t.submit(1, &"a".into(), "a#1", TOKEN, json!(1)),
        Err((SubmissionRejection::NotRunning, None))
    );
    t.bind(7, binding("a", "a#2", true)).unwrap();
    assert_eq!(
        t.submit(1, &"a".into(), "a#1", TOKEN, json!(1)),
        Err((SubmissionRejection::StaleAttempt, Some("a#2".into())))
    );
    assert_eq!(
        t.submit(1, &"b".into(), "a#2", TOKEN, json!(1)),
        Err((SubmissionRejection::NotRunning, None))
    );
    assert_eq!(
        t.submit(1, &"a".into(), "a#2", TOKEN, json!({"v": 1})),
        Ok(SubmitOutcome::Accepted)
    );
    assert_eq!(
        t.submit(1, &"a".into(), "a#2", TOKEN, json!({"v": 1})),
        Ok(SubmitOutcome::Duplicate)
    );
    assert_eq!(
        t.submit(1, &"a".into(), "a#2", TOKEN, json!({"v": 2})),
        Err((SubmissionRejection::Conflict, Some("a#2".into())))
    );
    // 토큰이 다르면 같은 회차라도 받지 않는다.
    assert_eq!(
        t.submit(1, &"a".into(), "a#2", "other", json!({"v": 1})),
        Err((SubmissionRejection::WrongToken, Some("a#2".into())))
    );
    t.report(7, "claude", answer("done"));
    assert_eq!(
        t.submit(1, &"a".into(), "a#2", TOKEN, json!({"v": 1})),
        Err((SubmissionRejection::TurnEnded, Some("a#2".into())))
    );
}

/// task id 는 workspace 마다 정해진다. 다른 workspace 의 같은 이름 task 를 풀거나 찾거나
/// 그 회차에 제출하지 않는다.
#[test]
fn the_same_task_name_in_two_workspaces_stays_apart() {
    let t = AgentTurns::new();
    let in_ws = |ws: u32| {
        TurnBinding::new(
            ws,
            "review".into(),
            "review#1".into(),
            "claude".into(),
            TOKEN.into(),
            true,
        )
    };
    t.bind(7, in_ws(1)).unwrap();
    t.bind(8, in_ws(2)).unwrap();
    let review: TaskId = "review".into();
    assert_eq!(t.find(1, &review).map(|(s, _)| s), Some(7));
    assert_eq!(t.find(2, &review).map(|(s, _)| s), Some(8));
    assert_eq!(
        t.submit(2, &review, "review#1", TOKEN, json!("from ws2")),
        Ok(SubmitOutcome::Accepted)
    );
    assert!(t.get(7).unwrap().submitted.is_none());
    assert_eq!(t.note_awaiting(2, &review, true, 5), Some(Some(5)));
    assert_eq!(t.get(7).unwrap().awaiting_since, None);
    // 한쪽이 끝나 풀려도 다른 workspace 의 묶음은 남는다.
    t.release(1, &review);
    assert!(t.get(7).is_none());
    assert_eq!(t.find(2, &review).map(|(s, _)| s), Some(8));
    assert_eq!(
        t.submit(1, &review, "review#1", TOKEN, json!("x")),
        Err((SubmissionRejection::NotRunning, None))
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
    // 제출 없는 보고를 낸다. 결과 확정이 result_missing 으로 끝내고 답은 기록에 남는다.
    let TurnPoll::Done(r) = decide(&b, 7, true, Some("idle"), 0, None) else {
        panic!("expected a report without a submission");
    };
    let out = r.output.unwrap();
    assert!(out.get("submitted").is_none());
    assert_eq!(out["final_answer"], json!("I reviewed it"));
    assert!(out.get("final_answer_truncated").is_none());
    // 기록에는 상한까지만 남긴다.
    let long = "가".repeat(tasty_agent::task::agent::ANSWER_EXCERPT_CHARS + 5);
    b.ended = Some(TurnEnd::Answer(Some(long)));
    let TurnPoll::Done(r) = decide(&b, 7, true, Some("idle"), 0, None) else {
        panic!("expected a report without a submission");
    };
    let out = r.output.unwrap();
    assert_eq!(
        out["final_answer"].as_str().unwrap().chars().count(),
        tasty_agent::task::agent::ANSWER_EXCERPT_CHARS
    );
    assert_eq!(out["final_answer_truncated"], json!(true));
    b.ended = Some(TurnEnd::Answer(Some("I reviewed it".into())));
    // string 출력에서 답이 없으면 그 자리에서 result_missing 이다.
    let mut none = binding("a", "a#1", true);
    none.ended = Some(TurnEnd::Answer(None));
    let TurnPoll::Failed(msg) = decide(&none, 7, false, Some("idle"), 0, None) else {
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
    assert_eq!(t.note_awaiting(1, &a, false, 5), Some(None), "첫 기록");
    assert_eq!(t.note_awaiting(1, &a, false, 6), None);
    assert_eq!(t.note_awaiting(1, &a, true, 7), Some(Some(7)));
    assert_eq!(
        t.note_awaiting(1, &a, true, 9),
        None,
        "대기 시작 시각은 유지한다"
    );
    assert_eq!(t.note_awaiting(1, &a, false, 10), Some(None));
}
