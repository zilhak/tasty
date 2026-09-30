//! fork된 자식이 exec 전까지 writer 잠금의 열린 파일 설명을 공유해도, 잠금을 놓은 뒤 다시 여는
//! 저장소가 writer가 될 수 있는지 확인한다.
//!
//! `pre_exec`가 있는 `Command`는 fork·exec 경로를, 없는 것은 posix_spawn을 쓴다. 두 경로 모두
//! 자식이 exec하기 전까지 부모의 fd를 복제해 가지므로, 부모가 방금 닫은 잠금 파일의 설명이
//! 잠시 남는다. 시험은 두 경로를 함께 돌린다.

#![cfg(unix)]

use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use tasty_event_store::{EventStore, StoreError};

const JOURNAL: &str = "journal-lock-inheritance";
const SPAWNERS: usize = 4;
const RUN_FOR: Duration = Duration::from_secs(3);

fn spawn_loop(stop: &AtomicBool, fork_path: bool) -> usize {
    let mut spawned = 0;
    while !stop.load(Ordering::SeqCst) {
        let mut command = Command::new("true");
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if fork_path {
            // SAFETY: 빈 closure는 fork 뒤 자식에서 메모리 할당이나 잠금 없이 바로 반환한다.
            unsafe {
                command.pre_exec(|| Ok(()));
            }
        }
        let mut child = command.spawn().expect("spawn child");
        child.wait().expect("wait child");
        spawned += 1;
    }
    spawned
}

#[test]
fn reopened_store_becomes_writer_while_children_are_forked() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("journal.db");
    drop(EventStore::open(&path, JOURNAL).expect("create journal"));

    let stop = Arc::new(AtomicBool::new(false));
    let spawners: Vec<_> = (0..SPAWNERS)
        .map(|i| {
            let stop = Arc::clone(&stop);
            thread::spawn(move || spawn_loop(&stop, i % 2 == 0))
        })
        .collect();

    let mut attempts = 0usize;
    let mut locked = 0usize;
    let deadline = Instant::now() + RUN_FOR;
    while Instant::now() < deadline {
        let mut store = EventStore::open(&path, JOURNAL).expect("open journal");
        attempts += 1;
        match store.acquire_writer() {
            Ok(_) => {}
            Err(StoreError::WriterLocked) => locked += 1,
            Err(other) => panic!("unexpected error: {other:?}"),
        }
    }
    stop.store(true, Ordering::SeqCst);
    let spawned: usize = spawners
        .into_iter()
        .map(|h| h.join().expect("spawner"))
        .sum();

    assert!(spawned > 0, "no child was spawned");
    assert_eq!(
        locked, 0,
        "{locked} of {attempts} reopens saw WriterLocked while {spawned} children were spawned"
    );
}
