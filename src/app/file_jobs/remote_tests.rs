use std::time::Duration;

use super::*;
use tasty_ipc::stream_hub::SinkReceiver;

const WAIT: Duration = Duration::from_secs(10);

#[derive(Debug)]
enum Incoming {
    Progress(Progress),
    Finished(Finished),
}

impl Incoming {
    fn parse(value: serde_json::Value) -> Option<Self> {
        match value.get("event").and_then(|v| v.as_str())? {
            PROGRESS_EVENT => serde_json::from_value(value).ok().map(Self::Progress),
            RESULT_EVENT => serde_json::from_value(value).ok().map(Self::Finished),
            _ => None,
        }
    }
}

fn request(op_id: u64, op: &RemoteOp) -> ExplorerOpMsg {
    ExplorerOpMsg::ExplorerOpRequest {
        op_id,
        op: serde_json::to_value(op).unwrap(),
    }
}

/// 다음 통지를 읽는다. 진행 통지는 `progress` 가 참일 때만 돌려주고 아니면 건너뛴다.
fn next(rx: &SinkReceiver, progress: bool) -> Incoming {
    loop {
        let frame = rx.recv_timeout(WAIT).expect("서버 통지가 와야 한다");
        let value: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
        let incoming = Incoming::parse(value).expect("원격 explorer 통지여야 한다");
        if progress || matches!(incoming, Incoming::Finished(_)) {
            return incoming;
        }
    }
}

fn finished(rx: &SinkReceiver) -> Finished {
    match next(rx, false) {
        Incoming::Finished(f) => f,
        Incoming::Progress(_) => unreachable!(),
    }
}

/// 충돌 질문이 담긴 진행 통지가 올 때까지 읽는다.
fn wait_ask(rx: &SinkReceiver) -> Ask {
    loop {
        match next(rx, true) {
            Incoming::Progress(Progress { ask: Some(ask), .. }) => return ask,
            Incoming::Progress(_) => {}
            Incoming::Finished(f) => panic!("질문 전에 끝났다: {f:?}"),
        }
    }
}

fn setup() -> (tempfile::TempDir, StreamHub, SinkReceiver, u32) {
    let dir = tempfile::tempdir().unwrap();
    let hub = StreamHub::new();
    let id = hub.alloc_id();
    let rx = hub.register(id);
    (dir, hub, rx, id)
}

/// client 와 서버가 같은 모양을 쓰는지 고정한다. 바뀌면 옛 peer 와 맞지 않는다.
#[test]
fn the_op_wire_shape_is_fixed() {
    let op = RemoteOp::Create {
        dir: "/srv/a".into(),
        name: "b".into(),
        folder: true,
    };
    assert_eq!(
        serde_json::to_value(&op).unwrap(),
        serde_json::json!({"kind": "create", "dir": "/srv/a", "name": "b", "folder": true})
    );
    let refused = serde_json::from_slice::<serde_json::Value>(&result_message(
        4,
        OpResult::Refused {
            reason: RefusedOp::Busy,
        },
    ))
    .unwrap();
    assert_eq!(
        refused,
        serde_json::json!({"event": "explorer_op_result", "op_id": 4, "result": {"refused": {"reason": "busy"}}})
    );
}

#[test]
fn a_client_that_holds_no_workspace_is_refused() {
    let (dir, hub, rx, id) = setup();
    let jobs = RemoteJobs::default();
    let op = RemoteOp::Create {
        dir: dir.path().into(),
        name: "x".into(),
        folder: true,
    };
    jobs.handle(&hub, id, false, request(1, &op));
    assert_eq!(
        finished(&rx).result,
        OpResult::Refused {
            reason: RefusedOp::NotHolder
        }
    );
    assert!(!dir.path().join("x").exists());
}

#[test]
fn a_malformed_op_is_refused() {
    let (_dir, hub, rx, id) = setup();
    let jobs = RemoteJobs::default();
    jobs.handle(
        &hub,
        id,
        true,
        ExplorerOpMsg::ExplorerOpRequest {
            op_id: 2,
            op: serde_json::json!({"kind": "format_disk"}),
        },
    );
    let f = finished(&rx);
    assert_eq!(f.op_id, 2);
    assert_eq!(
        f.result,
        OpResult::Refused {
            reason: RefusedOp::Invalid
        }
    );
}

#[test]
fn create_and_rename_run_on_the_server() {
    let (dir, hub, rx, id) = setup();
    let jobs = RemoteJobs::default();
    let create = RemoteOp::Create {
        dir: dir.path().into(),
        name: "new".into(),
        folder: true,
    };
    jobs.handle(&hub, id, true, request(1, &create));
    assert_eq!(finished(&rx).result, OpResult::Simple { error: None });
    assert!(dir.path().join("new").is_dir());
    // 같은 이름은 덮어쓰지 않고 실패한다.
    jobs.handle(&hub, id, true, request(2, &create));
    assert!(matches!(
        finished(&rx).result,
        OpResult::Simple { error: Some(_) }
    ));
    let rename = RemoteOp::Rename {
        path: dir.path().join("new"),
        name: "renamed".into(),
    };
    jobs.handle(&hub, id, true, request(3, &rename));
    assert_eq!(finished(&rx).result, OpResult::Simple { error: None });
    assert!(dir.path().join("renamed").is_dir());
}

/// 충돌 질문이 통지로 나가고, client 의 답으로 작업이 이어진다. 답을 기다리는 동안 같은 연결의
/// 새 요청은 바쁨으로 거절한다.
#[test]
fn a_conflict_is_asked_and_answered_over_the_connection() {
    let (dir, hub, rx, id) = setup();
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("a.txt"), "new").unwrap();
    std::fs::write(dst.join("a.txt"), "old").unwrap();
    let jobs = RemoteJobs::default();
    let paste = RemoteOp::Paste {
        paths: vec![src.join("a.txt")],
        destination: dst.clone(),
        cut: false,
    };
    jobs.handle(&hub, id, true, request(7, &paste));
    let ask = wait_ask(&rx);
    assert_eq!(ask.name, "a.txt");
    assert!(!ask.folder_conflict);
    jobs.handle(&hub, id, true, request(8, &paste));
    let busy = finished(&rx);
    assert_eq!(busy.op_id, 8);
    assert_eq!(
        busy.result,
        OpResult::Refused {
            reason: RefusedOp::Busy
        }
    );
    // 다른 작업 번호의 답은 버린다.
    jobs.handle(
        &hub,
        id,
        true,
        ExplorerOpMsg::ExplorerOpAnswer {
            op_id: 99,
            choice: ConflictChoice::Replace,
            apply_all: false,
        },
    );
    jobs.handle(
        &hub,
        id,
        true,
        ExplorerOpMsg::ExplorerOpAnswer {
            op_id: 7,
            choice: ConflictChoice::Skip,
            apply_all: false,
        },
    );
    let done = finished(&rx);
    assert_eq!(done.op_id, 7);
    let OpResult::Report { report, truncated } = done.result else {
        panic!("항목별 결과여야 한다");
    };
    assert!(!truncated);
    assert_eq!(report.skipped, vec![src.join("a.txt")]);
    assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "old");
}

/// 끊긴 연결의 작업은 취소한다. 질문을 기다리던 작업도 멈춘다.
#[test]
fn a_disconnected_client_cancels_its_job() {
    let (dir, hub, rx, id) = setup();
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("a.txt"), "new").unwrap();
    std::fs::write(dst.join("a.txt"), "old").unwrap();
    let jobs = RemoteJobs::default();
    let paste = RemoteOp::Paste {
        paths: vec![src.join("a.txt")],
        destination: dst.clone(),
        cut: true,
    };
    jobs.handle(&hub, id, true, request(1, &paste));
    wait_ask(&rx);
    jobs.forget_client(id);
    let OpResult::Report { report, .. } = finished(&rx).result else {
        panic!("항목별 결과여야 한다");
    };
    assert!(report.cancelled);
    assert!(src.join("a.txt").exists());
    assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "old");
}

/// 결과가 예산을 넘으면 되돌리기 단계와 남은 원본을 빼고 목록을 줄여 프레임에 들어가게 한다.
#[test]
fn an_oversized_report_is_truncated_without_undo_steps() {
    let long = "x".repeat(200);
    let paths: Vec<PathBuf> = (0..10_000)
        .map(|i| PathBuf::from(format!("/srv/{long}/{i}")))
        .collect();
    let report = Report {
        kind: job::OpKind::Copy,
        dest: Some("/srv".into()),
        total: paths.len(),
        done: 1,
        undo: vec![UndoStep::Replaced("/srv/a".into())],
        failed: Vec::new(),
        skipped: paths.clone(),
        cancelled: false,
        trash_unavailable: false,
        leftovers: Vec::new(),
        unprocessed: paths,
    };
    let bytes = result_message(
        1,
        OpResult::Report {
            report,
            truncated: false,
        },
    );
    assert!(bytes.len() <= RESULT_BUDGET);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let Some(Incoming::Finished(Finished {
        result: OpResult::Report { report, truncated },
        ..
    })) = Incoming::parse(value)
    else {
        panic!("결과 통지여야 한다");
    };
    assert!(truncated);
    assert!(report.undo.is_empty());
    assert!(!report.skipped.is_empty());
    assert_eq!(report.total, 10_000);
}

/// 같은 연결이라도 실행 중인 작업과 번호가 다른 답·취소는 그 작업에 닿지 않는다.
#[test]
fn answers_and_cancels_for_another_op_id_are_ignored() {
    let (dir, hub, rx, id) = setup();
    let src = dir.path().join("src");
    let dst = dir.path().join("dst");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::create_dir_all(&dst).unwrap();
    std::fs::write(src.join("a.txt"), "new").unwrap();
    std::fs::write(dst.join("a.txt"), "old").unwrap();
    let jobs = RemoteJobs::default();
    let paste = RemoteOp::Paste {
        paths: vec![src.join("a.txt")],
        destination: dst.clone(),
        cut: false,
    };
    jobs.handle(&hub, id, true, request(5, &paste));
    wait_ask(&rx);
    jobs.handle(&hub, id, true, ExplorerOpMsg::ExplorerOpCancel { op_id: 6 });
    jobs.handle(
        &hub,
        id,
        true,
        ExplorerOpMsg::ExplorerOpAnswer {
            op_id: 6,
            choice: ConflictChoice::Replace,
            apply_all: false,
        },
    );
    let shared = jobs.shared(id, 5).expect("작업이 아직 실행 중이어야 한다");
    assert!(!shared.cancelled());
    assert!(shared.pending_ask().is_some());
    jobs.handle(
        &hub,
        id,
        true,
        ExplorerOpMsg::ExplorerOpAnswer {
            op_id: 5,
            choice: ConflictChoice::Skip,
            apply_all: false,
        },
    );
    let OpResult::Report { report, .. } = finished(&rx).result else {
        panic!("항목별 결과여야 한다");
    };
    assert!(!report.cancelled);
    assert_eq!(report.skipped, vec![src.join("a.txt")]);
    assert_eq!(std::fs::read_to_string(dst.join("a.txt")).unwrap(), "old");
}
