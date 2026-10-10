use std::collections::HashMap;
use std::path::PathBuf;

use super::*;
use crate::app::file_jobs::job::{Answer, Ask, EntryFacts, OpKind as Kind, Report, Snapshot};
use crate::intent::{IntentOrigin, UserSource};

fn user() -> IntentOrigin {
    IntentOrigin::User {
        source: UserSource::ContextMenu,
    }
}

/// 원격 쓰기를 받는 mirror 칸에 요청 하나를 넣고 그 요청을 꺼낸다.
fn queued(operation: Operation) -> (u32, super::super::Request) {
    let (mut state, mut engine) = crate::state::tests::test_mirror_state();
    let read = engine.read();
    let sid = read.workspace_at(0).unwrap().all_surface_ids()[0];
    let ws = read.mirror_workspace_of(sid).expect("mirror 칸");
    drop(read);
    engine.remote.explorer_writable.insert(ws);
    state.request_explorer_file(&engine.read(), sid, operation, user());
    let request = state
        .explorer_file_requests
        .0
        .pop_front()
        .expect("받아야 한다");
    (ws, request)
}

fn sent(rx: &std::sync::mpsc::Receiver<tasty_remote::connection::QueuedFrame>) -> ExplorerOpMsg {
    let frame = rx.try_recv().expect("보낸 프레임이 있어야 한다");
    assert_eq!(frame.frame.tag, StreamTag::Control);
    serde_json::from_slice(&frame.frame.payload).unwrap()
}

fn start(
    files: &mut super::super::ExplorerFiles,
    sessions: &[AttachClientSession],
    ws: u32,
    request: super::super::Request,
) -> Arc<Shared> {
    let shared = Arc::new(Shared::interactive(|| {}));
    let started = files.start_remote(
        sessions,
        Start {
            window: winit::window::WindowId::from(1u64),
            engine: crate::runtime::engine_session::EngineId::for_test(),
            workspace: ws,
            target: request.target,
            operation: request.operation,
        },
        Arc::clone(&shared),
    );
    assert!(started.is_ok());
    shared
}

/// 원격 쓰기를 받는 mirror 칸의 요청은 대기열에 들어가고 이력에 쌓지 않는다. 받지 않으면 거절한다.
#[test]
fn a_writable_mirror_cell_queues_requests_without_history() {
    // 로컬이면 이력에 쌓을 작업(붙여넣기·이름 변경·새 항목)도 원격이면 쌓지 않는다.
    for operation in [
        Operation::Paste {
            paths: vec!["/srv/a".into()],
            destination: "/srv/b".into(),
            cut: false,
        },
        Operation::Rename {
            path: "/srv/a".into(),
            name: "b".into(),
        },
        Operation::Create {
            dir: "/srv".into(),
            name: "c".into(),
            folder: true,
        },
    ] {
        assert!(super::super::history::Source::of(&operation).is_some());
        let (_, request) = queued(operation);
        assert!(request.recorded.is_none());
    }

    let (mut state, engine) = crate::state::tests::test_mirror_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Trash(vec!["/srv/a".into()]),
        user(),
    );
    assert!(state.explorer_file_requests.0.is_empty());
    // 원격 파일을 이 컴퓨터에서 열지 않는다.
    let (mut state, mut engine) = crate::state::tests::test_mirror_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let ws = engine.read().mirror_workspace_of(sid).unwrap();
    engine.remote.explorer_writable.insert(ws);
    state.request_explorer_file(
        &engine.read(),
        sid,
        Operation::Open("/srv/a".into()),
        user(),
    );
    assert!(state.explorer_file_requests.0.is_empty());
}

/// 요청을 보내고, 서버 진행·질문을 옮기고, 사용자의 답과 취소를 보내고, 결과로 끝낸다.
#[test]
fn a_remote_job_relays_progress_answers_and_its_result() {
    let (ws, request) = queued(Operation::Paste {
        paths: vec!["/srv/a.txt".into()],
        destination: "/srv/dst".into(),
        cut: true,
    });
    let (sess, rx) = crate::app::attach_client::tests::test_session_with_frames(ws, HashMap::new());
    let mut sessions = vec![sess];
    let mut files = super::super::ExplorerFiles::default();
    let shared = start(&mut files, &sessions, ws, request);
    assert!(files.remote_busy(ws));
    let ExplorerOpMsg::ExplorerOpRequest { op_id, op } = sent(&rx) else {
        panic!("작업 요청이어야 한다");
    };
    assert_eq!(op["kind"], "paste");
    assert_eq!(op["cut"], true);

    let facts = EntryFacts {
        is_dir: false,
        size: Some(1),
        items: None,
        modified: None,
    };
    let ask = serde_json::json!({
        "event": PROGRESS_EVENT, "op_id": op_id,
        "progress": Snapshot {
            items_done: 0,
            items_total: 1,
            bytes_done: 0,
            bytes_total: 1,
            current: "a.txt".into(),
            removing: false,
        },
        "ask": Ask {
            name: "a.txt".into(),
            folder: "/srv/dst".into(),
            existing: facts.clone(),
            incoming: facts,
            folder_conflict: false,
            remaining: 0,
        },
    });
    // 다른 작업 번호의 통지는 버린다.
    let mut other = ask.clone();
    other["op_id"] = (op_id + 9).into();
    sessions[0].transport.explorer_inbox.extend([other, ask]);
    assert!(files.poll_remote(&mut sessions).is_empty());
    assert_eq!(shared.pending_ask().map(|a| a.name), Some("a.txt".into()));
    assert_eq!(shared.snapshot().items_total, 1);

    shared.answer(Answer {
        choice: Choice::Replace,
        apply_all: true,
    });
    shared.cancel();
    assert!(files.poll_remote(&mut sessions).is_empty());
    assert_eq!(
        sent(&rx),
        ExplorerOpMsg::ExplorerOpAnswer {
            op_id,
            choice: ConflictChoice::Replace,
            apply_all: true,
        }
    );
    assert_eq!(sent(&rx), ExplorerOpMsg::ExplorerOpCancel { op_id });
    // 취소는 한 번만 보낸다.
    assert!(files.poll_remote(&mut sessions).is_empty());
    assert!(rx.try_recv().is_err());

    let report = Report {
        kind: Kind::Move,
        dest: Some("/srv/dst".into()),
        total: 1,
        done: 0,
        undo: Vec::new(),
        failed: Vec::new(),
        skipped: Vec::new(),
        cancelled: true,
        trash_unavailable: false,
        leftovers: Vec::new(),
        unprocessed: vec!["/srv/a.txt".into()],
        resend: None,
    };
    sessions[0].transport.explorer_inbox.push(serde_json::json!({
        "event": RESULT_EVENT, "op_id": op_id,
        "result": {"report": {"report": serde_json::to_value(&report).unwrap(), "truncated": false}},
    }));
    let done = files.poll_remote(&mut sessions);
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].remote, Some(ws));
    assert!(done[0].recorded.is_none());
    assert!(matches!(&done[0].result, Done::Report(r) if r.cancelled));
    assert!(!files.remote_busy(ws));
}

/// 결과 전에 연결이 사라지면 결과를 모른다고 끝낸다. 서버 거절은 실패로 끝낸다.
#[test]
fn a_lost_connection_or_refusal_ends_the_job() {
    let (ws, request) = queued(Operation::Trash(vec!["/srv/a".into()]));
    let (sess, _rx) =
        crate::app::attach_client::tests::test_session_with_frames(ws, HashMap::new());
    let mut sessions = vec![sess];
    let mut files = super::super::ExplorerFiles::default();
    start(&mut files, &sessions, ws, request);
    sessions[0].transport.frame_tx.epoch().retire();
    let done = files.poll_remote(&mut sessions);
    assert!(matches!(
        &done[0].result,
        Done::Simple(Err(e)) if *e == crate::i18n::t("explorer.state.remote_result_unknown")
    ));

    let (ws, request) = queued(Operation::Rename {
        path: "/srv/a".into(),
        name: "b".into(),
    });
    let (sess, rx) = crate::app::attach_client::tests::test_session_with_frames(ws, HashMap::new());
    let mut sessions = vec![sess];
    start(&mut files, &sessions, ws, request);
    let ExplorerOpMsg::ExplorerOpRequest { op_id, .. } = sent(&rx) else {
        panic!("작업 요청이어야 한다");
    };
    sessions[0]
        .transport
        .explorer_inbox
        .push(serde_json::json!({
            "event": RESULT_EVENT, "op_id": op_id, "result": {"refused": {"reason": "busy"}},
        }));
    let done = files.poll_remote(&mut sessions);
    assert!(matches!(
        &done[0].result,
        Done::Simple(Err(e)) if *e == crate::i18n::t("explorer.state.remote_refused")
    ));
}

#[test]
fn only_explorer_op_frames_become_mirror_events() {
    let progress = br#"{"event":"explorer_op_progress","op_id":1}"#;
    assert!(matches!(
        mirror_event(progress),
        Some(MirrorEvent::ExplorerOp(_))
    ));
    assert!(mirror_event(br#"{"event":"list_dir_result","request_id":1}"#).is_none());
    assert!(mirror_event(b"not json").is_none());
}

/// 서버가 결과를 줄여 보내면 보냈던 경로를 Retry 목록으로 붙인다. 줄이지 않은 결과는 자기 목록을 쓴다.
#[test]
fn a_truncated_result_keeps_the_paths_that_were_sent() {
    let paths = vec![
        PathBuf::from("/srv/a"),
        PathBuf::from("/srv/b"),
        PathBuf::from("/srv/c"),
    ];
    for truncated in [true, false] {
        let (ws, request) = queued(Operation::Trash(paths.clone()));
        let (sess, rx) =
            crate::app::attach_client::tests::test_session_with_frames(ws, HashMap::new());
        let mut sessions = vec![sess];
        let mut files = super::super::ExplorerFiles::default();
        start(&mut files, &sessions, ws, request);
        let ExplorerOpMsg::ExplorerOpRequest { op_id, .. } = sent(&rx) else {
            panic!("작업 요청이어야 한다");
        };
        let report = Report {
            kind: Kind::Trash,
            dest: None,
            total: 3,
            done: 1,
            undo: Vec::new(),
            failed: Vec::new(),
            skipped: vec!["/srv/b".into()],
            cancelled: false,
            trash_unavailable: false,
            leftovers: Vec::new(),
            unprocessed: Vec::new(),
            resend: None,
        };
        sessions[0].transport.explorer_inbox.push(serde_json::json!({
            "event": RESULT_EVENT, "op_id": op_id,
            "result": {"report": {"report": serde_json::to_value(&report).unwrap(), "truncated": truncated}},
        }));
        let done = files.poll_remote(&mut sessions);
        let Done::Report(r) = &done[0].result else {
            panic!("항목별 결과여야 한다");
        };
        let expected = if truncated {
            paths.clone()
        } else {
            vec![PathBuf::from("/srv/b")]
        };
        assert_eq!(r.retryable(), expected);
    }
}

/// 연결이 끊겨 버린 대기 요청은 칸마다 그 수를 원격 실패 토스트로 알린다.
#[test]
fn requests_dropped_by_a_lost_connection_are_counted_per_cell() {
    let (mut state, _engine) = crate::state::tests::test_state();
    let mut lost = super::super::ui_sync::LostRequests::default();
    lost.add(5);
    lost.add(6);
    lost.add(5);
    lost.notify(&mut state);
    let toast = |n: u64| {
        crate::i18n::t_fmt(
            "explorer.state.operation_failed",
            &crate::i18n::t_count("explorer.state.remote_queue_dropped", n, &[&n.to_string()]),
        )
    };
    assert_eq!(state.toasts.messages(), vec![toast(2), toast(1)]);
    assert_ne!(toast(2), toast(1));
}
