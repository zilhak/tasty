//! durable command identity: 재요청 조회·중복·충돌·진행 갱신.

use crate::{
    CommandLookup, CommandStatus, CommandUpdate, CommitOutcome, CommitRequest, ExpectedRevision,
    StoreError, StreamId,
};

use super::common::{append, command, db_path, fresh, key, open};

#[test]
fn committed_key_survives_reopen_and_is_not_executed_again() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let (mut store, epoch) = open(&path);
    let mut request = CommitRequest::new(epoch);
    request.command = Some(command("cmd-1", Some(key("agent", "close-7")), b"digest"));
    request
        .appends
        .push(append("engine-a", ExpectedRevision::NoStream, &["closed"]));
    store.commit(&request).expect("commit");
    drop(store);

    // 재시작 뒤 같은 요청. 대상은 이미 닫혔지만 원래 결과로 답할 수 있어야 한다.
    let (mut store, epoch) = open(&path);
    let hit = store
        .lookup_command(&key("agent", "close-7"), b"digest")
        .expect("lookup");
    let CommandLookup::Hit(record) = hit else {
        panic!("expected hit, got {hit:?}");
    };
    assert_eq!(record.resolved, b"target=pane:1");
    assert_eq!(record.response.as_deref(), Some(&b"ok"[..]));
    assert_eq!(record.status, CommandStatus::Completed);
    assert_eq!(record.batch_ids.len(), 1);

    // lookup을 건너뛴 재제출도 저장소가 합류시킨다.
    request.writer_epoch = epoch;
    let outcome = store.commit(&request).expect("resubmit");
    assert!(matches!(outcome, CommitOutcome::Duplicate(ref r) if r.command_id == "cmd-1"));
    assert_eq!(
        store
            .stream_revision(&StreamId::new("engine-a"))
            .expect("head"),
        Some(1)
    );
    assert_eq!(store.current_cut().expect("cut").last_batch, Some(1));
}

#[test]
fn same_key_with_different_request_conflicts() {
    let (_dir, mut store, epoch) = fresh();
    let mut first = CommitRequest::new(epoch);
    first.command = Some(command("cmd-1", Some(key("agent", "k")), b"one"));
    store.commit(&first).expect("first");

    let lookup = store
        .lookup_command(&key("agent", "k"), b"two")
        .expect("lookup");
    assert!(matches!(lookup, CommandLookup::DigestMismatch(_)));

    let mut second = CommitRequest::new(epoch);
    second.command = Some(command("cmd-2", Some(key("agent", "k")), b"two"));
    second
        .appends
        .push(append("engine-a", ExpectedRevision::Any, &["x"]));
    let err = store.commit(&second).expect_err("conflict");
    assert!(matches!(err, StoreError::KeyConflict { .. }), "{err:?}");
    assert_eq!(store.command("cmd-2").expect("read"), None);
    assert_eq!(store.current_cut().expect("cut").last_batch, None);

    // 다른 호출자 범위의 같은 키는 별개다.
    let mut other = CommitRequest::new(epoch);
    other.command = Some(command("cmd-3", Some(key("plugin", "k")), b"two"));
    assert!(matches!(
        store.commit(&other).expect("other scope"),
        CommitOutcome::Committed { batch: None }
    ));
    assert_eq!(
        store
            .lookup_command(&key("nobody", "k"), b"two")
            .expect("miss"),
        CommandLookup::Miss
    );
}

#[test]
fn keyless_internal_commands_do_not_collide() {
    let (_dir, mut store, epoch) = fresh();
    for id in ["internal-1", "internal-2"] {
        let mut request = CommitRequest::new(epoch);
        request.command = Some(command(id, None, b"same"));
        store.commit(&request).expect("keyless");
    }
    assert!(store.command("internal-2").expect("read").is_some());

    let mut reused = CommitRequest::new(epoch);
    reused.command = Some(command("internal-1", None, b"same"));
    assert!(matches!(store.commit(&reused), Err(StoreError::Sqlite(_))));
}

#[test]
fn progress_updates_stop_at_a_terminal_status() {
    let (_dir, mut store, epoch) = fresh();
    let mut accept = CommitRequest::new(epoch);
    let mut cmd = command("cmd-1", Some(key("agent", "k")), b"d");
    cmd.status = CommandStatus::InProgress;
    cmd.response = None;
    accept.command = Some(cmd);
    store.commit(&accept).expect("accept");

    // 결과 명령의 이벤트와 원래 명령의 완료를 한 batch에 확정한다.
    let mut finish = CommitRequest::new(epoch);
    finish.command = Some(command("result-1", None, b"r"));
    finish
        .appends
        .push(append("engine-a", ExpectedRevision::Any, &["done"]));
    finish.command_updates.push(CommandUpdate {
        command_id: "cmd-1".to_owned(),
        status: CommandStatus::Completed,
        response: Some(b"tab=5".to_vec()),
    });
    store.commit(&finish).expect("finish");
    let record = store.command("cmd-1").expect("read").expect("exists");
    assert_eq!(record.status, CommandStatus::Completed);
    assert_eq!(record.response.as_deref(), Some(&b"tab=5"[..]));

    let mut again = CommitRequest::new(epoch);
    again
        .appends
        .push(append("engine-a", ExpectedRevision::Any, &["late"]));
    again.command_updates.push(CommandUpdate {
        command_id: "cmd-1".to_owned(),
        status: CommandStatus::Failed,
        response: None,
    });
    let err = store.commit(&again).expect_err("terminal");
    assert!(matches!(err, StoreError::CommandFinished { .. }), "{err:?}");
    assert_eq!(
        store
            .stream_revision(&StreamId::new("engine-a"))
            .expect("head"),
        Some(1),
        "the rejected update must roll back its events"
    );

    let mut unknown = CommitRequest::new(epoch);
    unknown.command_updates.push(CommandUpdate {
        command_id: "missing".to_owned(),
        status: CommandStatus::Completed,
        response: None,
    });
    assert!(matches!(
        store.commit(&unknown),
        Err(StoreError::UnknownCommand(_))
    ));
}

#[test]
fn progress_does_not_move_back() {
    let (_dir, mut store, epoch) = fresh();
    let mut accept = CommitRequest::new(epoch);
    let mut cmd = command("cmd-1", Some(key("agent", "k")), b"d");
    cmd.status = CommandStatus::Accepted;
    cmd.response = None;
    accept.command = Some(cmd);
    store.commit(&accept).expect("accept");

    let update = |status, response: &[u8]| {
        let mut request = CommitRequest::new(epoch);
        request.command_updates.push(CommandUpdate {
            command_id: "cmd-1".to_owned(),
            status,
            response: Some(response.to_vec()),
        });
        request
    };
    store
        .commit(&update(CommandStatus::InProgress, b"step 1"))
        .expect("forward");
    // 진행 중 응답은 같은 단계에서 교체할 수 있다.
    store
        .commit(&update(CommandStatus::InProgress, b"step 2"))
        .expect("same stage");

    let err = store
        .commit(&update(CommandStatus::Accepted, b"back"))
        .expect_err("regression");
    assert!(
        matches!(
            err,
            StoreError::CommandRegression {
                from: CommandStatus::InProgress,
                to: CommandStatus::Accepted,
                ..
            }
        ),
        "{err:?}"
    );
    let record = store.command("cmd-1").expect("read").expect("exists");
    assert_eq!(record.status, CommandStatus::InProgress);
    assert_eq!(record.response.as_deref(), Some(&b"step 2"[..]));

    store
        .commit(&update(CommandStatus::Cancelled, b"cancelled"))
        .expect("finish");
}

#[test]
fn events_carry_their_command_and_causation() {
    let (_dir, mut store, epoch) = fresh();
    let mut request = CommitRequest::new(epoch);
    request.command = Some(command("cmd-9", None, b"d"));
    let mut a = append("engine-a", ExpectedRevision::NoStream, &["e1"]);
    a.events[0].causation_id = Some("fx-3".to_owned());
    request.appends.push(a);
    store.commit(&request).expect("commit");

    let events = store
        .read_stream(&StreamId::new("engine-a"), None, 10)
        .expect("read");
    assert_eq!(events[0].command_id.as_deref(), Some("cmd-9"));
    assert_eq!(events[0].causation_id.as_deref(), Some("fx-3"));
    assert_eq!(events[0].stream_revision, 1);
}
