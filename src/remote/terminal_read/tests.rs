use super::*;
use tasty_ipc::stream_hub::StreamHub;

const HOLDER: u32 = 7;

/// 첫 surface 에 PTY 없는 터미널을 두고 그 id 를 돌려준다.
fn with_terminal(engine: &mut EngineMut<'_>, cols: usize, rows: usize) -> u32 {
    let sid = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    engine.runtime.terminals.insert(
        sid,
        tasty_terminal::Terminal::new_detached(cols, rows),
        None,
    );
    sid
}

fn feed(engine: &mut EngineMut<'_>, sid: u32, bytes: &[u8]) {
    engine
        .runtime
        .terminals
        .get_mut(sid)
        .expect("terminal")
        .feed_bytes(bytes);
}

fn request(sid: u32, op: ReadOp) -> TerminalReadMsg {
    TerminalReadMsg::TerminalReadRequest {
        request_id: 1,
        surface_id: sid,
        op,
    }
}

fn answer(sink: &tasty_ipc::stream_hub::SinkReceiver) -> ReadOutcome {
    let frame = sink.try_recv().expect("one result frame");
    assert!(frame.payload.len() <= RESULT_BUDGET);
    match serde_json::from_slice(&frame.payload).expect("result json") {
        TerminalReadMsg::TerminalReadResult { outcome, .. } => outcome,
        other => panic!("not a result: {other:?}"),
    }
}

fn reason(outcome: &ReadOutcome) -> &str {
    match outcome {
        ReadOutcome::Err { data, .. } => data["reason"].as_str().unwrap_or(""),
        ReadOutcome::Ok { .. } => "",
    }
}

fn output(from: ReadFrom) -> ReadOp {
    ReadOp::Output {
        from,
        max_bytes: None,
        strip_ansi: false,
    }
}

/// 다른 surface·다른 client 의 점유는 이 터미널을 읽을 권한이 아니다.
#[test]
fn only_the_holder_of_that_terminal_may_read_it() {
    let mut session = crate::state::tests::test_state().1;
    let mut engine = session.borrow_mut();
    let sid = with_terminal(&mut engine, 20, 4);
    let hub = StreamHub::new();
    let sink = hub.register(HOLDER);
    let other = hub.register(9);
    assert!(handle(
        &mut engine,
        &hub,
        HOLDER,
        &request(sid, ReadOp::Position)
    ));
    assert_eq!(reason(&answer(&sink)), "not_attached", "unheld terminal");
    engine.live.occupancy.acquire(sid, 9).expect("lock");

    assert!(handle(
        &mut engine,
        &hub,
        HOLDER,
        &request(sid, ReadOp::Position)
    ));
    assert_eq!(reason(&answer(&sink)), "not_attached");
    assert!(handle(
        &mut engine,
        &hub,
        9,
        &request(sid, ReadOp::Position)
    ));
    assert!(matches!(answer(&other), ReadOutcome::Ok { .. }));
}

#[test]
fn the_screen_answer_matches_the_local_handler() {
    let mut session = crate::state::tests::test_state().1;
    let mut engine = session.borrow_mut();
    let sid = with_terminal(&mut engine, 20, 4);
    feed(&mut engine, sid, b"one\r\n\x1b[2mdim\x1b[0m\r\nthree");
    engine.live.occupancy.acquire(sid, HOLDER).expect("lock");
    let op = ReadOp::ScreenText {
        lines: None,
        show_dim: false,
    };
    let ReadOutcome::Ok { result } = read(&mut engine, 1, sid, &op) else {
        panic!("screen read failed");
    };
    let local = crate::adapters::ipc::handler::surface::handle_screen_text(
        &engine.as_ref(),
        Value::Null,
        &json!({ "surface_id": sid }),
    );
    assert_eq!(Some(result.clone()), local.result);
    assert_eq!(result["dim_omitted_rows"], 1);
}

/// 제어 문자가 많은 원문은 JSON 에서 커진다. 한 결과는 프레임 예산 안이고 이어 읽으면 원본 전체가 된다.
#[test]
fn escape_heavy_output_fits_each_frame_and_continues_to_the_whole_source() {
    let mut session = crate::state::tests::test_state().1;
    let mut engine = session.borrow_mut();
    let sid = with_terminal(&mut engine, 80, 4);
    let chunk = b"\x1b[0m\x1b[1m".repeat(4096);
    for _ in 0..24 {
        feed(&mut engine, sid, &chunk);
    }
    engine.live.occupancy.acquire(sid, HOLDER).expect("lock");
    let hub = StreamHub::new();
    let sink = hub.register(HOLDER);

    let mut text = String::new();
    let mut from = ReadFrom::Start;
    let mut frames = 0;
    loop {
        assert!(handle(
            &mut engine,
            &hub,
            HOLDER,
            &request(sid, output(from.clone()))
        ));
        let ReadOutcome::Ok { result } = answer(&sink) else {
            panic!("output read failed");
        };
        frames += 1;
        text.push_str(result["text"].as_str().expect("text"));
        let next = result["next_cursor"].as_u64().expect("next");
        if next == result["retention_end"].as_u64().expect("end") {
            break;
        }
        from = ReadFrom::Cursor {
            cursor: next,
            stream: result["stream"].as_str().expect("stream").into(),
        };
    }
    assert!(frames > 1, "the source does not fit one frame");
    assert_eq!(text.as_bytes(), chunk.repeat(24).as_slice());
}

/// 서버 터미널이 바뀌면 마크·cursor 모두 명시 오류다. 처음부터 조용히 읽지 않는다.
#[test]
fn a_stale_stream_is_refused_and_the_shared_mark_is_left_alone() {
    let mut session = crate::state::tests::test_state().1;
    let mut engine = session.borrow_mut();
    let sid = with_terminal(&mut engine, 20, 4);
    feed(&mut engine, sid, b"before");
    engine
        .runtime
        .terminals
        .get_mut(sid)
        .expect("terminal")
        .set_mark();
    feed(&mut engine, sid, b" after");
    engine.live.occupancy.acquire(sid, HOLDER).expect("lock");
    let shared = |engine: &mut EngineMut<'_>| {
        crate::adapters::ipc::handler::surface::handle_read_since_mark(
            engine,
            Value::Null,
            &json!({ "surface_id": sid }),
        )
        .result
    };
    let before = shared(&mut engine);

    let stale = ReadFrom::Cursor {
        cursor: 0,
        stream: "not-this-stream".into(),
    };
    assert_eq!(
        reason(&read(&mut engine, 1, sid, &output(stale))),
        "stream_mismatch"
    );
    let ReadOutcome::Ok { result } = read(&mut engine, 2, sid, &ReadOp::Position) else {
        panic!("position failed");
    };
    assert_eq!(result["retention_end"], 12);
    assert!(result["stream"].is_string());
    let ReadOutcome::Ok { result } = read(&mut engine, 3, sid, &output(ReadFrom::Start)) else {
        panic!("read failed");
    };
    assert_eq!(result["text"], "before after");
    assert_eq!(
        before,
        shared(&mut engine),
        "server shared mark did not move"
    );
}

#[test]
fn a_screen_read_that_cannot_fit_a_frame_is_refused_not_cut() {
    let mut session = crate::state::tests::test_state().1;
    let mut engine = session.borrow_mut();
    let sid = with_terminal(&mut engine, 200, 4);
    let line = format!("{}\r\n", "x".repeat(199));
    for _ in 0..3000 {
        feed(&mut engine, sid, line.as_bytes());
    }
    let op = ReadOp::ScreenText {
        lines: Some(3000),
        show_dim: true,
    };
    assert_eq!(reason(&read(&mut engine, 1, sid, &op)), "too_large");
}

#[test]
fn a_missing_terminal_is_left_to_the_next_engine() {
    let mut session = crate::state::tests::test_state().1;
    let mut engine = session.borrow_mut();
    let sid = with_terminal(&mut engine, 20, 4);
    engine.live.occupancy.acquire(sid, HOLDER).expect("lock");
    engine.runtime.terminals.remove(sid);
    let hub = StreamHub::new();
    let sink = hub.register(HOLDER);
    // 다른 창의 engine 은 터미널이 없으면 점유와 관계없이 다음 engine 에 넘긴다.
    assert!(!handle(
        &mut engine,
        &hub,
        9,
        &request(sid, ReadOp::Position)
    ));
    assert!(!handle(
        &mut engine,
        &hub,
        HOLDER,
        &request(sid, ReadOp::Position)
    ));
    assert!(
        sink.try_recv().is_err(),
        "no answer from an engine without the terminal"
    );
    reply_missing(&hub, HOLDER, &request(sid, ReadOp::Position));
    assert_eq!(reason(&answer(&sink)), "no_terminal");
}
