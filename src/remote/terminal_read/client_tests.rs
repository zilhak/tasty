use super::*;

const WS: u32 = 3;
const LOCAL: u32 = 70;
const REMOTE: u32 = 515;

fn epoch() -> ConnectionEpoch {
    tasty_remote::connection::channel().0.epoch()
}

fn view<'a>(
    epoch: &ConnectionEpoch,
    connected: bool,
    map: &'a HashMap<u32, u32>,
) -> SessionView<'a> {
    SessionView {
        workspace: WS,
        capable: true,
        connected,
        epoch: epoch.clone(),
        remote_to_local: map,
    }
}

fn mapping() -> HashMap<u32, u32> {
    HashMap::from([(REMOTE, LOCAL)])
}

fn output() -> Request {
    Request::Output {
        cursor: None,
        max_bytes: None,
        strip_ansi: false,
    }
}

fn sent_op(sends: &[(u32, TerminalReadMsg)]) -> (u64, u32, ReadOp) {
    match sends {
        [
            (
                WS,
                TerminalReadMsg::TerminalReadRequest {
                    request_id,
                    surface_id,
                    op,
                },
            ),
        ] => (*request_id, *surface_id, op.clone()),
        other => panic!("expected one request, got {other:?}"),
    }
}

fn error_reason(response: &JsonRpcResponse) -> Value {
    response
        .error
        .as_ref()
        .expect("error")
        .data
        .clone()
        .expect("data")["reason"]
        .clone()
}

#[test]
fn a_result_replaces_the_remote_surface_id_with_the_mirror_one() {
    let mut c = Client::default();
    let (e, map) = (epoch(), mapping());
    let id = c.begin(WS, LOCAL, output(), Instant::now());
    let (rid, remote, op) = sent_op(&c.sync(&[view(&e, true, &map)]));
    assert_eq!((rid, remote), (id, REMOTE));
    assert!(matches!(
        op,
        ReadOp::Output {
            from: ReadFrom::Start,
            ..
        }
    ));
    c.receive(
        &e,
        id,
        ReadOutcome::Ok {
            result: json!({ "text": "x", "surface_id": REMOTE }),
        },
    );
    let r = c
        .take_reply(id, json!(1), Instant::now())
        .expect("answered");
    let result = r.result.expect("ok");
    assert_eq!(result["surface_id"], LOCAL);
    assert_eq!(result["remote_surface_id"], REMOTE);
    assert_eq!(result["remote"], true);
}

/// 보낸 연결과 다른 세대로 도착한 결과는 버린다. 대기 읽기는 재연결을 본 뒤 끊김으로 끝난다.
#[test]
fn an_answer_from_another_connection_is_dropped() {
    let mut c = Client::default();
    let (old, new, map) = (epoch(), epoch(), mapping());
    let id = c.begin(WS, LOCAL, output(), Instant::now());
    c.sync(&[view(&old, true, &map)]);
    c.receive(
        &new,
        id,
        ReadOutcome::Ok {
            result: json!({ "text": "late" }),
        },
    );
    assert!(c.take_reply(id, json!(1), Instant::now()).is_none());
    c.sync(&[view(&new, true, &map)]);
    let r = c
        .take_reply(id, json!(1), Instant::now())
        .expect("answered");
    assert_eq!(error_reason(&r), "remote_disconnected");
}

#[test]
fn a_read_waiting_for_a_reconnecting_session_ends_as_disconnected() {
    let mut c = Client::default();
    let (e, map) = (epoch(), mapping());
    let id = c.begin(WS, LOCAL, output(), Instant::now());
    c.sync(&[view(&e, true, &map)]);
    c.sync(&[view(&e, false, &map)]);
    let r = c
        .take_reply(id, json!(1), Instant::now())
        .expect("answered");
    assert_eq!(error_reason(&r), "remote_disconnected");
}

#[test]
fn an_unanswered_read_times_out() {
    let mut c = Client::default();
    let (e, map) = (epoch(), mapping());
    let start = Instant::now();
    let id = c.begin(WS, LOCAL, output(), start);
    c.sync(&[view(&e, true, &map)]);
    assert!(c.take_reply(id, json!(1), start).is_none());
    let r = c
        .take_reply(id, json!(1), start + REPLY_LIMIT)
        .expect("answered");
    assert_eq!(error_reason(&r), "remote_timeout");
}

/// set mark 는 서버 위치를 받고 로컬 마크를 적용한 뒤에만 mirror 마크가 된다. 다음 읽기는 그 위치부터다.
#[test]
fn set_mark_records_the_server_position_only_after_the_local_mark() {
    let mut c = Client::default();
    let (e, map) = (epoch(), mapping());
    let id = c.begin(WS, LOCAL, Request::Mark, Instant::now());
    let (_, _, op) = sent_op(&c.sync(&[view(&e, true, &map)]));
    assert_eq!(op, ReadOp::Position);
    c.receive(
        &e,
        id,
        ReadOutcome::Ok {
            result: json!({ "stream": "s1", "retention_end": 42 }),
        },
    );
    assert!(c.take_reply(id, json!(1), Instant::now()).is_none());
    assert!(c.marks.is_empty(), "not before the local mark");
    assert_eq!(c.marks_to_apply(), vec![(id, LOCAL)]);
    c.mark_applied(id, true);
    assert!(
        c.take_reply(id, json!(1), Instant::now())
            .expect("answered")
            .result
            .is_some()
    );

    let read = c.begin(WS, LOCAL, output(), Instant::now());
    let (_, _, op) = sent_op(&c.sync(&[view(&e, true, &map)]));
    assert_eq!(
        op,
        ReadOp::Output {
            from: ReadFrom::Cursor {
                cursor: 42,
                stream: "s1".into()
            },
            max_bytes: None,
            strip_ansi: false,
        }
    );
    // 서버 터미널이 바뀌었으면 명시 오류와 다시 설정 안내. 마크는 지운다.
    c.receive(
        &e,
        read,
        ReadOutcome::Err {
            code: -32602,
            message: "mismatch".into(),
            data: json!({ "reason": "stream_mismatch" }),
        },
    );
    let r = c
        .take_reply(read, json!(1), Instant::now())
        .expect("answered");
    assert_eq!(error_reason(&r), "stream_mismatch");
    assert!(r.error.expect("error").message.contains("set mark again"));
    assert!(c.marks.is_empty());
}

#[test]
fn a_failed_set_mark_changes_no_mark() {
    let mut c = Client::default();
    let (e, map) = (epoch(), mapping());
    let id = c.begin(WS, LOCAL, Request::Mark, Instant::now());
    c.sync(&[view(&e, true, &map)]);
    c.receive(
        &e,
        id,
        ReadOutcome::Err {
            code: -32000,
            message: "no".into(),
            data: json!({ "reason": "not_attached" }),
        },
    );
    assert!(
        c.marks_to_apply().is_empty(),
        "the local mark is not touched"
    );
    let r = c
        .take_reply(id, json!(1), Instant::now())
        .expect("answered");
    assert_eq!(error_reason(&r), "not_attached");
    assert!(c.marks.is_empty());
}

/// 호출자가 준 cursor 는 마크와 무관하게 그대로 보내고, stream 오류에 마크 안내를 붙이지 않는다.
#[test]
fn an_explicit_cursor_error_passes_through_unchanged() {
    let mut c = Client::default();
    let (e, map) = (epoch(), mapping());
    c.marks.insert((WS, REMOTE), ("s1".into(), 42));
    let id = c.begin(
        WS,
        LOCAL,
        Request::Output {
            cursor: Some((5, "s0".into())),
            max_bytes: None,
            strip_ansi: false,
        },
        Instant::now(),
    );
    c.sync(&[view(&e, true, &map)]);
    c.receive(
        &e,
        id,
        ReadOutcome::Err {
            code: -32602,
            message: "mismatch".into(),
            data: json!({ "reason": "stream_mismatch" }),
        },
    );
    let r = c
        .take_reply(id, json!(1), Instant::now())
        .expect("answered");
    assert_eq!(r.error.expect("error").message, "mismatch");
    assert!(c.marks.contains_key(&(WS, REMOTE)));
}

/// 지원 확인 뒤 세션이 구 서버로 바뀌었으면 보내지 않고 바로 끝낸다. 10초를 기다리지 않는다.
#[test]
fn a_read_for_a_session_without_the_capability_ends_at_once() {
    let mut c = Client::default();
    let (e, map) = (epoch(), mapping());
    let id = c.begin(WS, LOCAL, output(), Instant::now());
    let mut v = view(&e, true, &map);
    v.capable = false;
    assert!(c.sync(&[v]).is_empty());
    let r = c
        .take_reply(id, json!(1), Instant::now())
        .expect("answered");
    assert_eq!(error_reason(&r), "remote_read_unsupported");
}
