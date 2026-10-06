//! 저장소를 열기 전에 writer 잠금을 선점하는 경로. 부팅 첫머리에서 같은 홈을 쓰는 다른 프로세스를
//! 감지하고, 얻은 잠금을 그대로 저장소에 넘겨 writer가 되는지 확인한다.

use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use tasty_event_store::{
    EventStore, StoreError, WRITER_LOCK_WAIT, WriterPreempt, preempt_writer_lock,
};

const JOURNAL: &str = "journal-preempt";

#[test]
fn preempt_acquires_on_an_empty_home_and_the_store_becomes_writer_with_it() {
    let dir = tempfile::tempdir().unwrap();
    // 상위 디렉터리가 아직 없어도 만든다(첫 실행의 `<home>/structure`).
    let db = dir.path().join("structure").join("journal.db");
    let WriterPreempt::Acquired(lock) = preempt_writer_lock(&db).unwrap() else {
        panic!("an empty home must be free");
    };
    let mut store = EventStore::open(&db, JOURNAL).unwrap();
    store.acquire_writer_with(lock).unwrap();
    assert!(store.is_writer());
    // 다른 저장소는 기다린 끝에 거절된다.
    let mut other = EventStore::open(&db, JOURNAL).unwrap();
    assert!(matches!(
        other.acquire_writer(),
        Err(StoreError::WriterLocked)
    ));
}

#[test]
fn preempt_reports_held_while_another_store_is_writer() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("journal.db");
    let mut store = EventStore::open(&db, JOURNAL).unwrap();
    store.acquire_writer().unwrap();
    let started = Instant::now();
    assert!(matches!(
        preempt_writer_lock(&db).unwrap(),
        WriterPreempt::Held
    ));
    // 계속 쥐고 있으면 재시도 구간을 다 쓴 뒤에 거절한다.
    assert!(started.elapsed() >= WRITER_LOCK_WAIT);
    store.release_writer();
    assert!(matches!(
        preempt_writer_lock(&db).unwrap(),
        WriterPreempt::Acquired(_)
    ));
}

#[test]
fn preempt_acquires_a_lock_released_within_the_retry_window() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("journal.db");
    let mut store = EventStore::open(&db, JOURNAL).unwrap();
    store.acquire_writer().unwrap();
    // 비정상 종료한 이전 소유자의 잠금이 잠시 뒤 풀리는 경우.
    let release_after = Duration::from_millis(300);
    let owner = thread::spawn(move || {
        thread::sleep(release_after);
        store.release_writer();
        store
    });
    let started = Instant::now();
    let outcome = preempt_writer_lock(&db).unwrap();
    let waited = started.elapsed();
    drop(owner.join().unwrap());
    assert!(
        matches!(outcome, WriterPreempt::Acquired(_)),
        "a lock released within the retry window must be acquired"
    );
    assert!(waited < WRITER_LOCK_WAIT, "waited {waited:?}");
}

#[test]
fn a_held_preempt_lock_blocks_the_next_preempt_until_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("journal.db");
    let WriterPreempt::Acquired(lock) = preempt_writer_lock(&db).unwrap() else {
        panic!("first preempt must acquire");
    };
    assert!(matches!(
        preempt_writer_lock(&db).unwrap(),
        WriterPreempt::Held
    ));
    drop(lock);
    assert!(matches!(
        preempt_writer_lock(&db).unwrap(),
        WriterPreempt::Acquired(_)
    ));
}

#[test]
fn a_lock_for_another_journal_file_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.db");
    let b = dir.path().join("b.db");
    let WriterPreempt::Acquired(lock) = preempt_writer_lock(&a).unwrap() else {
        panic!("preempt must acquire");
    };
    let mut store = EventStore::open(&b, JOURNAL).unwrap();
    assert!(matches!(
        store.acquire_writer_with(lock),
        Err(StoreError::WriterLockMismatch)
    ));
    assert!(!store.is_writer());
    // 거절된 잠금은 놓였다.
    assert!(matches!(
        preempt_writer_lock(&a).unwrap(),
        WriterPreempt::Acquired(_)
    ));
}

#[test]
fn concurrent_preempts_yield_exactly_one_owner() {
    const CONTENDERS: usize = 8;
    // 진 쪽은 재시도 구간을 다 쓰고 거절되므로 한 회차가 그만큼 걸린다.
    for _ in 0..3 {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(dir.path().join("structure").join("journal.db"));
        let start = Arc::new(Barrier::new(CONTENDERS));
        let decided = Arc::new(Barrier::new(CONTENDERS));
        let handles: Vec<_> = (0..CONTENDERS)
            .map(|_| {
                let db = db.clone();
                let start = start.clone();
                let decided = decided.clone();
                thread::spawn(move || {
                    start.wait();
                    let outcome = preempt_writer_lock(&db).unwrap();
                    let acquired = matches!(outcome, WriterPreempt::Acquired(_));
                    // 모두 결과를 정할 때까지 잠금을 쥐고 있어야 늦게 시도한 쪽이 Held를 본다.
                    decided.wait();
                    drop(outcome);
                    acquired
                })
            })
            .collect();
        let owners = handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .filter(|acquired| *acquired)
            .count();
        assert_eq!(owners, 1, "exactly one contender must own the writer lock");
    }
}
