//! 실제 파일 state.db와 실제 writer로 최근 파일·튜토리얼의 저장 의미를 확인한다. 범용 실행기의
//! 접수·수집·재시도 자체는 `tasty_utils::batch_writer`의 시험이 맡고, 여기서는 state 요청의 매핑과
//! 그 부수 상태(표시·확정 revision, 버전 체인, 디스크 결과)를 본다.

use super::*;
use crate::recent_files::{Entry, MAX_ENTRIES};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tasty_utils::batch_writer::{ManualClock, SystemClock};

const WAIT: Duration = Duration::from_secs(10);

struct Fixture {
    _dir: tempfile::TempDir,
    path: PathBuf,
    parts: StateWriterParts,
    shared: SharedState,
}

fn fixture_with(
    prepare: impl FnOnce(&Connection),
    limits: Limits,
    clock: Arc<dyn Clock>,
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let db = crate::db::Db::open(&path).unwrap();
    prepare(&db.conn);
    let (parts, shared) = start(db, limits, clock, Arc::new(|| {})).unwrap();
    Fixture {
        _dir: dir,
        path,
        parts,
        shared,
    }
}

fn fixture() -> Fixture {
    fixture_with(|_| {}, Limits::default(), Arc::new(SystemClock))
}

/// 수집 창이 시계를 움직일 때까지 열려 있는 fixture.
fn held_fixture(prepare: impl FnOnce(&Connection), limits: Limits) -> (Fixture, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::default());
    let f = fixture_with(prepare, limits, clock.clone());
    (f, clock)
}

fn release(clock: &ManualClock) {
    clock.advance(Duration::from_millis(50));
}

/// `kind`의 확정 revision이 `target`이 될 때까지 결과를 거둔다.
fn settle_to(f: &mut Fixture, kind: &str, target: u64) {
    let deadline = Instant::now() + WAIT;
    while f.shared.recent.revisions(kind).committed < target {
        assert!(
            Instant::now() < deadline,
            "{kind} never reached revision {target}"
        );
        f.parts.poll();
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// 쓰기 결과가 모두 거둬질 때까지 기다린다(성공 여부와 무관).
fn settle_all(f: &mut Fixture) {
    let deadline = Instant::now() + WAIT;
    loop {
        f.parts.poll();
        if f.shared.recent.pending() == 0 {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "recent results were never reaped"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn wait(receipt: &TutorialReceipt) -> Result<Reply, String> {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(result) = receipt.try_take() {
            return result;
        }
        assert!(
            Instant::now() < deadline,
            "tutorial receipt never completed"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn saved(result: Result<Reply, String>) -> (Record, bool) {
    match result {
        Ok(Reply::Saved { record, updated }) => (record, updated),
        other => panic!("expected a saved reply, got {other:?}"),
    }
}

fn shutdown(f: &mut Fixture) {
    let deadline = Instant::now() + WAIT;
    while f.parts.poll_shutdown() != 0 {
        assert!(Instant::now() < deadline, "state writer did not join");
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// 디스크의 행을 읽기 전용 연결로 최신순으로 읽는다. writer가 살아 있어도 된다.
fn disk(path: &Path, kind: &str) -> Vec<(String, i64)> {
    let conn = Connection::open(path).unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT path, opened_at FROM recent_files WHERE kind = ?1
             ORDER BY opened_at DESC, rowid DESC",
        )
        .unwrap();
    stmt.query_map([kind], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn disk_paths(path: &Path, kind: &str) -> Vec<String> {
    disk(path, kind).into_iter().map(|(p, _)| p).collect()
}

/// 앱을 다시 켤 때와 같이 파일을 다시 열어 부팅 읽기를 한다.
fn reopen(path: &Path) -> std::collections::HashMap<String, Vec<Entry>> {
    let mut db = crate::db::Db::open(path).unwrap();
    crate::recent_files::load_at_boot(&mut db.conn)
}

fn reopened_paths(path: &Path, kind: &str) -> Vec<String> {
    reopen(path)
        .remove(kind)
        .unwrap_or_default()
        .into_iter()
        .map(|e| e.path)
        .collect()
}

fn save(
    queue: &StateQueue,
    owner: &Arc<()>,
    topic: &'static str,
    step: &'static str,
    completed: bool,
    expected: i64,
) -> TutorialReceipt {
    submit_tutorial(
        queue,
        StateRequest::TutorialSave {
            owner: owner.clone(),
            save: Save {
                topic,
                revision: 1,
                step,
                completed,
                expected,
            },
        },
    )
    .unwrap()
}

fn load(queue: &StateQueue, owner: &Arc<()>, topics: &[&'static str]) -> TutorialReceipt {
    submit_tutorial(
        queue,
        StateRequest::TutorialLoad {
            owner: owner.clone(),
            topics: topics.iter().map(|t| (*t, 1)).collect(),
        },
    )
    .unwrap()
}

#[test]
fn two_windows_share_one_list_and_the_disk_holds_the_last_committed_one() {
    let mut f = fixture();
    let mut first = f.shared.recent.clone();
    let mut second = f.shared.recent.clone();
    first.add("markdown", "/notes/first.md".into());
    second.add("markdown", "/notes/second.md".into());
    first.add("markdown", "/notes/./first.md".into());
    assert_eq!(first.get("markdown"), second.get("markdown"));
    assert_eq!(second.get("markdown")[0], "/notes/./first.md");
    settle_to(&mut f, "markdown", 3);
    // 같은 정규화 키의 옛 표기는 디스크에서도 사라지고 새 표기가 맨 앞이다.
    assert_eq!(
        disk_paths(&f.path, "markdown"),
        vec!["/notes/./first.md", "/notes/second.md"]
    );

    for i in 0..12 {
        second.add("markdown", format!("/notes/{i}.md"));
    }
    first.add("directory", "/work".into());
    settle_to(&mut f, "markdown", 15);
    settle_to(&mut f, "directory", 1);
    assert_eq!(first.get("markdown").len(), MAX_ENTRIES);
    assert_eq!(first.get("markdown")[0], "/notes/11.md");
    assert!(f.shared.recent.unsaved().is_empty());
    shutdown(&mut f);
    assert_eq!(reopened_paths(&f.path, "markdown"), first.get("markdown"));
    assert_eq!(reopened_paths(&f.path, "directory"), vec!["/work"]);
    assert_eq!(disk(&f.path, "markdown").len(), MAX_ENTRIES);
}

/// 같은 초에 연 항목도 다시 열 때 목록 순서가 같다. 이미 있던 경로를 맨 앞으로 옮긴 경우를 포함한다.
#[test]
fn entries_opened_in_the_same_second_reopen_in_list_order() {
    let mut f = fixture();
    let entry = |path: &str| Entry {
        path: path.into(),
        opened_at: 1_000,
    };
    for entries in [
        vec![entry("/c"), entry("/b"), entry("/a")],
        vec![entry("/a"), entry("/c"), entry("/b")],
    ] {
        let receipt = f
            .shared
            .queue
            .submit(StateRequest::recent(RecentSnapshot {
                kind: "markdown".into(),
                revision: 1,
                entries,
            }))
            .unwrap();
        assert!(receipt.wait_timeout(WAIT).is_some_and(|c| c.took_effect()));
    }
    let rows = disk(&f.path, "markdown");
    assert!(rows.windows(2).all(|w| w[0].1 > w[1].1), "{rows:?}");
    assert!(rows.iter().all(|(_, ts)| *ts - 1_000 < MAX_ENTRIES as i64));
    shutdown(&mut f);
    assert_eq!(reopened_paths(&f.path, "markdown"), vec!["/a", "/c", "/b"]);
}

#[test]
fn boot_reading_cleans_legacy_rows_and_the_first_save_prunes_to_the_cap() {
    let mut f = fixture_with(
        |conn| {
            for i in 0..12 {
                conn.execute(
                    "INSERT INTO recent_files(kind, path, opened_at) VALUES('markdown', ?1, ?2)",
                    rusqlite::params![format!("/m/{i}.md"), 100 + i],
                )
                .unwrap();
            }
            // 같은 키의 옛 표기와 이전 테이블의 행.
            conn.execute(
                "INSERT INTO recent_files(kind, path, opened_at) VALUES('markdown', '/m/./11.md', 50)",
                [],
            )
            .unwrap();
            conn.execute_batch(
                "INSERT INTO recent_markdown(path, opened_at) VALUES('/legacy.md', 10);",
            )
            .unwrap();
            conn.execute("DELETE FROM meta WHERE key = 'recent_files_migrated'", [])
                .unwrap();
        },
        Limits::default(),
        Arc::new(SystemClock),
    );
    let list = f.shared.recent.get("markdown");
    assert_eq!(list.len(), MAX_ENTRIES);
    assert_eq!(list[0], "/m/11.md");
    assert!(!disk_paths(&f.path, "markdown").contains(&"/m/./11.md".to_string()));
    // 부팅 읽기는 상한 밖의 행을 지우지 않는다. 첫 저장이 정리한다.
    assert_eq!(disk(&f.path, "markdown").len(), 13);
    let mut recent = f.shared.recent.clone();
    recent.add("markdown", "/m/new.md".into());
    settle_to(&mut f, "markdown", 1);
    assert_eq!(disk_paths(&f.path, "markdown"), recent.get("markdown"));
}

/// 저장 단계 하나가 실패하면 같은 묶음의 최근 목록·튜토리얼 저장이 모두 되돌려지고, 화면 목록은
/// 남되 확정 revision과 튜토리얼 버전 체인은 앞으로 가지 않는다.
#[test]
fn a_failed_step_rolls_back_the_batch_and_keeps_the_chain_where_it_was() {
    let (mut f, clock) = held_fixture(
        |conn| {
            conn.execute_batch(
                "CREATE TRIGGER boom BEFORE INSERT ON recent_files WHEN NEW.path = '/boom'
                 BEGIN SELECT RAISE(ABORT, 'injected insert failure'); END;",
            )
            .unwrap();
        },
        Limits::default(),
    );
    let owner = Arc::new(());
    let mut recent = f.shared.recent.clone();
    recent.add("markdown", "/notes/x.md".into());
    let first = save(&f.shared.queue, &owner, "basics", "one", false, 0);
    release(&clock);
    settle_to(&mut f, "markdown", 1);
    assert_eq!(saved(wait(&first)).0.version, 1);

    // 저장이 먼저 실행돼 체인 사본을 앞으로 보낸 뒤 같은 묶음의 최근 목록이 실패한다.
    let failing = save(&f.shared.queue, &owner, "basics", "two", false, 1);
    recent.add("markdown", "/notes/./x.md".into());
    recent.add("markdown", "/boom".into());
    release(&clock);
    assert!(wait(&failing).is_err());
    settle_all(&mut f);
    assert_eq!(recent.get("markdown"), vec!["/boom", "/notes/./x.md"]);
    assert_eq!(
        recent.revisions("markdown"),
        crate::recent_files::Revisions {
            visible: 3,
            committed: 1
        }
    );
    // 앞 단계에서 지운 같은 키의 옛 표기도 되돌려졌다.
    assert_eq!(disk_paths(&f.path, "markdown"), vec!["/notes/x.md"]);

    Connection::open(&f.path)
        .unwrap()
        .execute_batch("DROP TRIGGER boom;")
        .unwrap();
    // 체인이 실패한 저장으로 앞서 갔다면 이 저장은 다른 쓰기로 보이고 반영되지 않는다.
    let retry = save(&f.shared.queue, &owner, "basics", "two", false, 1);
    recent.add("markdown", "/notes/y.md".into());
    release(&clock);
    let (record, updated) = saved(wait(&retry));
    assert!(updated);
    assert_eq!(record.version, 2);
    assert_eq!(record.resume_step.as_deref(), Some("two"));
    settle_to(&mut f, "markdown", 4);
    assert_eq!(disk_paths(&f.path, "markdown"), recent.get("markdown"));
}

/// commit 단계 실패는 확정 여부를 모르는 결과로 다루고, 확정 revision과 체인을 올리지 않는다.
#[test]
fn a_commit_that_fails_is_not_reported_as_saved() {
    let (mut f, clock) = held_fixture(
        |conn| {
            conn.execute_batch(
                "CREATE TABLE fk_parent(id INTEGER PRIMARY KEY);
                 CREATE TABLE fk_child(pid INTEGER REFERENCES fk_parent(id) DEFERRABLE INITIALLY DEFERRED);
                 CREATE TRIGGER late AFTER INSERT ON recent_files WHEN NEW.path = '/late'
                 BEGIN INSERT INTO fk_child VALUES(1); END;",
            )
            .unwrap();
        },
        Limits::default(),
    );
    let owner = Arc::new(());
    let mut recent = f.shared.recent.clone();
    recent.add("markdown", "/late".into());
    let receipt = f
        .shared
        .queue
        .submit(StateRequest::TutorialSave {
            owner: owner.clone(),
            save: Save {
                topic: "basics",
                revision: 1,
                step: "one",
                completed: false,
                expected: 0,
            },
        })
        .unwrap();
    release(&clock);
    let completion = receipt.wait_timeout(WAIT).expect("completion");
    assert!(
        matches!(completion, Completion::Unknown(_)),
        "{completion:?}"
    );
    settle_all(&mut f);
    assert_eq!(recent.revisions("markdown").committed, 0);
    assert!(disk(&f.path, "markdown").is_empty());
    // 체인은 실패한 commit의 버전을 채택하지 않았다.
    let again = save(&f.shared.queue, &owner, "basics", "two", false, 0);
    release(&clock);
    let (record, updated) = saved(wait(&again));
    assert!(updated);
    assert_eq!(record.version, 1);
}

/// 다른 연결이 쓰기 잠금을 쥔 동안에도 추가는 바로 돌아오고, 잠금이 풀리면 저장이 확정된다.
#[test]
fn adding_while_another_connection_holds_the_lock_returns_at_once() {
    let mut f = fixture();
    let blocker = Connection::open(&f.path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let mut recent = f.shared.recent.clone();
    let started = Instant::now();
    recent.add("markdown", "/a.md".into());
    let took = started.elapsed();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(recent.revisions("markdown").committed, 0);
    blocker.execute_batch("ROLLBACK;").unwrap();
    settle_to(&mut f, "markdown", 1);
    assert!(
        took < Duration::from_millis(100),
        "add waited {took:?} for the lock"
    );
    assert_eq!(disk_paths(&f.path, "markdown"), vec!["/a.md"]);
}

/// 잠금 경합은 아무것도 쓰지 않은 busy로 돌아와 다시 실행할 수 있다.
#[test]
fn lock_contention_maps_to_busy_with_nothing_written() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.db");
    let db = crate::db::Db::open(&path).unwrap();
    db.conn.busy_timeout(Duration::ZERO).unwrap();
    let mut executor =
        StateExecutor::new(db.conn, Arc::new(tasty_memory::DbLatencyStats::default()));
    let blocker = Connection::open(&path).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE;").unwrap();
    let owner = Arc::new(());
    let request = || StateRequest::TutorialSave {
        owner: owner.clone(),
        save: Save {
            topic: "basics",
            revision: 1,
            step: "one",
            completed: false,
            expected: 0,
        },
    };
    let mut batch = [request()];
    assert!(matches!(executor.execute(&mut batch), Execution::Busy(_)));
    assert_eq!(executor.latency.snapshot().commits, 0);
    blocker.execute_batch("ROLLBACK;").unwrap();
    match executor.execute(&mut batch) {
        Execution::Committed(results) => match &results[..] {
            [Ok(StateOutput::Tutorial(Reply::Saved { record, updated }))] => {
                assert!(*updated);
                assert_eq!(record.version, 1);
            }
            other => panic!("unexpected results {other:?}"),
        },
        other => panic!("expected a commit, got {other:?}"),
    }
}

/// 같은 수집 창에 넣은 병합 가능한 요청은 commit 하나로 확정된다. 부팅 읽기는 계측에 들지 않는다.
#[test]
fn a_burst_in_one_collection_window_commits_fewer_times_than_requests() {
    let (mut f, clock) = held_fixture(|_| {}, Limits::default());
    let before = f.parts.latency.snapshot().commits;
    assert_eq!(before, 0, "boot reading must not count as a write commit");
    let (a, b) = (Arc::new(()), Arc::new(()));
    let mut recent = f.shared.recent.clone();
    for i in 0..3 {
        recent.add("markdown", format!("/m/{i}.md"));
    }
    recent.add("directory", "/d".into());
    let sa = save(&f.shared.queue, &a, "basics", "one", false, 0);
    let sb = save(&f.shared.queue, &b, "panes", "one", false, 0);
    let requests = 6;
    release(&clock);
    settle_to(&mut f, "markdown", 3);
    settle_to(&mut f, "directory", 1);
    assert!(saved(wait(&sa)).1 && saved(wait(&sb)).1);
    let stats = f.parts.writer.stats();
    let commits = f.parts.latency.snapshot().commits - before;
    assert_eq!(stats.admitted, requests);
    assert_eq!(stats.batches, 1);
    assert_eq!(
        stats.superseded, 2,
        "older markdown lists are replaced in the batch"
    );
    assert_eq!(commits, 1);
    assert!(commits < requests);
    // 대체된 목록을 뺀 네 쓰기(최근 목록 둘, 튜토리얼 저장 둘)가 한 commit에 확정됐다.
    assert_eq!(
        f.parts.latency.snapshot().writes_per_commit,
        [0, 1, 0, 0, 0, 0]
    );
    assert_eq!(disk_paths(&f.path, "markdown"), recent.get("markdown"));
}

/// 앞 요청이 끝난 뒤 넣는 순차 저장은 commit 수가 줄지 않고 하나하나 확정된다.
#[test]
fn sequential_saves_commit_one_by_one() {
    let mut f = fixture();
    let mut recent = f.shared.recent.clone();
    for i in 1..=4u64 {
        recent.add("markdown", format!("/s/{i}.md"));
        settle_to(&mut f, "markdown", i);
        assert_eq!(disk_paths(&f.path, "markdown"), recent.get("markdown"));
    }
    assert_eq!(f.parts.latency.snapshot().commits, 4);
}

#[test]
fn tutorial_saves_keep_cas_completion_and_own_rapid_writes() {
    let f = fixture();
    let q = &f.shared.queue;
    let (a, b) = (Arc::new(()), Arc::new(()));
    // 두 View가 같은 버전으로 다른 위치를 저장하면 뒤 저장은 앞 위치를 되돌리지 않는다.
    let (first, updated) = saved(wait(&save(q, &a, "basics", "a-step", false, 0)));
    assert!(updated);
    let (seen, updated) = saved(wait(&save(q, &b, "basics", "b-step", false, 0)));
    assert!(!updated);
    assert_eq!(seen, first);
    // 오래된 버전의 완료도 반영되고, 뒤의 미완료 저장이 완료를 되돌리지 않는다.
    let (done, _) = saved(wait(&save(q, &b, "basics", "b-step", true, 0)));
    assert!(done.completed);
    let (still, _) = saved(wait(&save(q, &a, "basics", "a-later", false, 1)));
    assert!(still.completed);
    assert_eq!(still.resume_step.as_deref(), Some("a-later"));

    // 결과를 기다리지 않은 빠른 연속 저장은 자기 성공만 따라 잇는다.
    let rapid: Vec<_> = ["r1", "r2", "r3"]
        .into_iter()
        .map(|step| save(q, &a, "panes", step, false, 0))
        .collect();
    let versions: Vec<_> = rapid.iter().map(|r| saved(wait(r))).collect();
    assert!(versions.iter().all(|(_, updated)| *updated));
    assert_eq!(
        versions.iter().map(|(r, _)| r.version).collect::<Vec<_>>(),
        vec![1, 2, 3]
    );
    // 다른 View가 쓴 뒤의 옛 체인 저장은 그 변경을 채택하지 않는다.
    let (other, updated) = saved(wait(&save(q, &b, "panes", "b", false, 3)));
    assert!(updated);
    let (late, updated) = saved(wait(&save(q, &a, "panes", "late", false, 0)));
    assert!(!updated);
    assert_eq!(late, other);
}

/// 접수한 저장은 View(소유자·receipt)를 버려도 실행되고, 뒤의 읽기는 그 저장 뒤에 실행된다.
#[test]
fn an_accepted_save_survives_its_view_and_a_later_load_sees_it() {
    let f = fixture();
    let q = &f.shared.queue;
    {
        let owner = Arc::new(());
        let receipt = save(q, &owner, "basics", "kept", true, 0);
        drop(owner);
        drop(receipt);
    }
    let observer = Arc::new(());
    match wait(&load(q, &observer, &["basics", "panes"])) {
        Ok(Reply::Loaded(records)) => {
            assert!(records[0].completed);
            assert_eq!(records[0].resume_step.as_deref(), Some("kept"));
            assert_eq!(records[1], Record::default());
        }
        other => panic!("expected loaded records, got {other:?}"),
    }
}

/// 예산이 차면 접수를 거절하고 화면 목록은 남긴다. 거둔 결과는 예산을 돌려준다.
#[test]
fn a_full_queue_refuses_without_losing_the_visible_list_and_reaping_frees_the_budget() {
    let limits = Limits {
        queue_items: 2,
        ..Limits::default()
    };
    let (mut f, clock) = held_fixture(|_| {}, limits);
    let mut recent = f.shared.recent.clone();
    recent.add("markdown", "/1.md".into());
    recent.add("markdown", "/2.md".into());
    recent.add("markdown", "/3.md".into());
    assert!(matches!(
        submit_tutorial(
            &f.shared.queue,
            StateRequest::TutorialLoad {
                owner: Arc::new(()),
                topics: vec![("basics", 1)],
            }
        ),
        Err(SubmitError::Saturated { .. })
    ));
    assert_eq!(f.parts.writer.stats().rejected_saturated, 2);
    release(&clock);
    settle_to(&mut f, "markdown", 2);
    settle_all(&mut f);
    assert_eq!(recent.get("markdown"), vec!["/3.md", "/2.md", "/1.md"]);
    assert_eq!(
        f.shared.recent.unsaved(),
        vec![(
            "markdown".to_string(),
            crate::recent_files::Revisions {
                visible: 3,
                committed: 2
            }
        )]
    );
    assert_eq!(f.shared.recent.pending(), 0);
    assert_eq!(f.parts.writer.stats().held_items, 0);
    assert_eq!(disk_paths(&f.path, "markdown"), vec!["/2.md", "/1.md"]);
}

/// 종료는 접수한 저장을 마지막 flush까지 실행하고, 그 뒤의 변경은 접수하지 않는다.
#[test]
fn shutdown_drains_accepted_saves_and_refuses_later_ones() {
    let mut f = fixture();
    let mut recent = f.shared.recent.clone();
    recent.add("markdown", "/before.md".into());
    let owner = Arc::new(());
    let accepted = save(&f.shared.queue, &owner, "basics", "one", false, 0);
    shutdown(&mut f);
    assert_eq!(recent.revisions("markdown").committed, 1);
    assert!(saved(wait(&accepted)).1);
    recent.add("markdown", "/after.md".into());
    assert!(matches!(
        submit_tutorial(
            &f.shared.queue,
            StateRequest::TutorialLoad {
                owner,
                topics: vec![("basics", 1)],
            }
        ),
        Err(SubmitError::Closed)
    ));
    assert_eq!(recent.get("markdown"), vec!["/after.md", "/before.md"]);
    assert_eq!(f.shared.recent.unsaved().len(), 1);
    assert_eq!(reopened_paths(&f.path, "markdown"), vec!["/before.md"]);
}

#[test]
fn results_that_did_not_take_effect_never_count_as_saved() {
    let error = StateError("disk I/O error".into());
    for completion in [
        Completion::Failed(error.clone()),
        Completion::Unknown(error.clone()),
        Completion::Lost,
        Completion::Done(Err(error.clone())),
    ] {
        assert!(matches!(
            recent_outcome(&completion),
            RecentOutcome::NotCommitted(_)
        ));
        assert!(tutorial_reply(completion).is_err());
    }
    let superseded: StateCompletion = Completion::Superseded {
        by: 7,
        outcome: Box::new(Completion::Done(Ok(()))),
    };
    assert_eq!(recent_outcome(&superseded), RecentOutcome::Superseded);
    assert_eq!(
        recent_outcome(&Completion::Done(Ok(StateOutput::Recent))),
        RecentOutcome::Committed
    );
}
