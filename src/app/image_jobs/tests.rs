use super::paste::{self, PasteDelivery, PasteRequest};
use super::{ImageJobRejected, ImageWorker, MAX_HELD_BYTES, MAX_JOBS};
use crate::runtime::surface_binding::SurfaceBinding;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

fn worker<T: Send + 'static>() -> (ImageWorker<T>, mpsc::Receiver<()>) {
    let (wake_tx, wake_rx) = mpsc::channel();
    let wake_tx = std::sync::Mutex::new(wake_tx);
    let worker = ImageWorker::new(Arc::new(move || {
        if let Ok(tx) = wake_tx.lock()
            && tx.send(()).is_err()
        {
            tracing::debug!("test wake receiver closed");
        }
    }));
    (worker, wake_rx)
}

fn wait_results<T: Send + 'static>(worker: &mut ImageWorker<T>, count: usize) -> Vec<T> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut done = Vec::new();
    while done.len() < count {
        assert!(
            Instant::now() < deadline,
            "image job results did not arrive"
        );
        done.extend(worker.take_results());
        std::thread::sleep(Duration::from_millis(1));
    }
    done
}

/// 작업이 끝나지 않아도 접수는 바로 돌아오고, 완료되면 깨우기와 결과가 온다.
#[test]
fn submission_does_not_wait_for_the_file_work() {
    let (mut jobs, wake) = worker::<u32>();
    let (release, gate) = mpsc::channel::<()>();
    let started = Instant::now();
    jobs.submit(1, move || {
        gate.recv().expect("test releases the job");
        7
    })
    .expect("accepted");
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(jobs.take_results().is_empty());
    assert!(jobs.has_pending());

    release.send(()).expect("job waits");
    wake.recv_timeout(Duration::from_secs(10))
        .expect("worker wakes the loop");
    assert_eq!(wait_results(&mut jobs, 1), [7]);
    assert!(!jobs.has_pending());
}

/// 대기 건수와 메모리 바이트 상한을 넘는 요청은 Busy 로 거절하고, 결과를 회수하면 다시 받는다.
#[test]
fn admission_is_bounded_by_job_count_and_held_bytes() {
    let (mut jobs, _wake) = worker::<usize>();
    let (release, gate) = mpsc::channel::<()>();
    let gate = Arc::new(std::sync::Mutex::new(gate));
    for index in 0..MAX_JOBS {
        let gate = gate.clone();
        jobs.submit(0, move || {
            gate.lock().expect("gate").recv().expect("released");
            index
        })
        .expect("within the job bound");
    }
    assert_eq!(jobs.submit(0, || 99), Err(ImageJobRejected::Busy));

    for _ in 0..MAX_JOBS {
        release.send(()).expect("job waits");
    }
    let order = wait_results(&mut jobs, MAX_JOBS);
    assert_eq!(order, (0..MAX_JOBS).collect::<Vec<_>>(), "FIFO completion");

    jobs.submit(MAX_HELD_BYTES, || 1)
        .expect("exactly the byte bound");
    assert_eq!(jobs.submit(1, || 2), Err(ImageJobRejected::Busy));
    assert_eq!(wait_results(&mut jobs, 1), [1]);
    jobs.submit(1, || 3)
        .expect("bytes released with the result");
    assert_eq!(wait_results(&mut jobs, 1), [3]);
}

/// 비어 있는 worker 는 바이트 상한을 넘는 한 건을 받고, 그 건이 끝날 때까지 다음 요청은 거절한다.
#[test]
fn an_idle_worker_accepts_one_oversized_job() {
    let (mut jobs, _wake) = worker::<u32>();
    let (release, gate) = mpsc::channel::<()>();
    jobs.submit(MAX_HELD_BYTES + 1, move || {
        gate.recv().expect("released");
        1
    })
    .expect("an oversized job on an idle worker");
    assert_eq!(jobs.submit(0, || 2), Err(ImageJobRejected::Busy));
    release.send(()).expect("job waits");
    assert_eq!(wait_results(&mut jobs, 1), [1]);
    jobs.submit(MAX_HELD_BYTES + 1, || 3)
        .expect("accepted again once idle");
    assert_eq!(wait_results(&mut jobs, 1), [3]);
}

/// 종료가 시작되면 새 요청을 거절하고, 시작하지 않은 작업은 실행하지 않으며 worker 가 끝난다.
#[test]
fn shutdown_skips_queued_jobs_and_joins_the_worker() {
    let (mut jobs, _wake) = worker::<u32>();
    let (release, gate) = mpsc::channel::<()>();
    let (started_tx, started) = mpsc::channel::<()>();
    let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    jobs.submit(0, move || {
        started_tx.send(()).expect("test waits for the start");
        gate.recv().expect("released");
        1
    })
    .expect("accepted");
    for _ in 0..3 {
        let ran = ran.clone();
        jobs.submit(0, move || {
            ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            2
        })
        .expect("accepted");
    }
    started
        .recv_timeout(Duration::from_secs(10))
        .expect("first job running");
    jobs.begin_shutdown();
    assert_eq!(jobs.submit(0, || 3), Err(ImageJobRejected::ShuttingDown));
    assert_ne!(jobs.poll_shutdown(), 0, "the running job holds the worker");

    release.send(()).expect("job waits");
    let deadline = Instant::now() + Duration::from_secs(10);
    while jobs.poll_shutdown() != 0 {
        assert!(Instant::now() < deadline, "worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(ran.load(std::sync::atomic::Ordering::SeqCst), 0);
}

/// 결과를 전달하지 않고 버리면 임시 디렉터리까지 회수된다. 종료 중 버린 결과도 같다.
#[test]
fn a_discarded_saved_file_is_removed() {
    let saved = paste::save("paste-1.png", b"png").expect("saved");
    let path = saved.path().to_path_buf();
    assert_eq!(std::fs::read(&path).expect("readable"), b"png");
    let dir = path.parent().expect("own dir").to_path_buf();
    drop(saved);
    assert!(!dir.exists(), "undelivered paste directory is reclaimed");

    let (mut jobs, _wake) = worker::<paste::SavedFile>();
    let (path_tx, path_rx) = mpsc::channel();
    jobs.submit(3, move || {
        let saved = paste::save("paste-2.png", b"png").expect("saved");
        path_tx
            .send(saved.path().to_path_buf())
            .expect("test waits");
        saved
    })
    .expect("accepted");
    let path = path_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("job ran");
    let deadline = Instant::now() + Duration::from_secs(10);
    while jobs.poll_shutdown() != 0 {
        assert!(Instant::now() < deadline, "worker did not finish");
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(!path.exists(), "shutdown discards results and their files");
}

fn terminal_with_sink(
    engine: &mut crate::runtime::engine_session::EngineSession,
    sid: u32,
) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    let mut terminal = tasty_terminal::Terminal::new_detached(80, 24);
    terminal.set_input_sink(tx);
    engine
        .borrow_mut()
        .runtime
        .terminals
        .insert(sid, terminal, None);
    rx
}

fn request(target: SurfaceBinding, bracketed: bool) -> PasteRequest {
    PasteRequest {
        target,
        origin_view: std::sync::Weak::new(),
        bracketed,
    }
}

/// 원 터미널이 그대로면 경로를 입력하고 파일을 남긴다. View 가 닫혀도 입력 여부는 바뀌지 않는다.
#[test]
fn a_saved_path_reaches_only_the_original_terminal_generation() {
    let (_state, mut engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let input = terminal_with_sink(&mut engine, sid);
    let target = SurfaceBinding::capture(&engine.read(), sid).expect("binding");

    let saved = paste::save("paste-3.png", b"png").expect("saved");
    let path = saved.path().to_path_buf();
    let delivery = paste::deliver(&mut engine.borrow_mut(), &request(target, true), saved);
    assert_eq!(delivery, PasteDelivery::Delivered);
    let mut expected = b"\x1b[200~".to_vec();
    expected.extend_from_slice(path.to_string_lossy().as_bytes());
    expected.extend_from_slice(b"\x1b[201~");
    assert_eq!(input.try_recv().expect("path written"), expected);
    assert!(path.exists(), "the shell reads the delivered file later");
    std::fs::remove_dir_all(path.parent().expect("own dir")).expect("test cleanup");
}

/// 같은 번호의 새 터미널에는 입력하지 않고 파일을 회수한다.
#[test]
fn a_replaced_terminal_does_not_receive_the_path() {
    let (_state, mut engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    drop(terminal_with_sink(&mut engine, sid));
    let target = SurfaceBinding::capture(&engine.read(), sid).expect("binding");
    let replacement = terminal_with_sink(&mut engine, sid);

    let saved = paste::save("paste-4.png", b"png").expect("saved");
    let dir = saved.path().parent().expect("own dir").to_path_buf();
    let delivery = paste::deliver(&mut engine.borrow_mut(), &request(target, false), saved);
    assert_eq!(delivery, PasteDelivery::TargetRetired);
    assert!(replacement.try_recv().is_err());
    assert!(!dir.exists());
}

/// 점유 중인 터미널에는 입력하지 않고 파일을 회수한다.
#[test]
fn a_hard_occupied_terminal_does_not_receive_the_path() {
    let (_state, mut engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let input = terminal_with_sink(&mut engine, sid);
    let target = SurfaceBinding::capture(&engine.read(), sid).expect("binding");
    engine
        .borrow_mut()
        .live
        .occupancy
        .acquire(sid, 1)
        .expect("occupied");

    let saved = paste::save("paste-5.png", b"png").expect("saved");
    let dir = saved.path().parent().expect("own dir").to_path_buf();
    let delivery = paste::deliver(&mut engine.borrow_mut(), &request(target, false), saved);
    assert_eq!(delivery, PasteDelivery::HardOccupied);
    assert!(input.try_recv().is_err());
    assert!(!dir.exists());
}

/// 붙여넣기 실행은 파일을 만들거나 경로를 입력하지 않고 저장 요청만 남긴다.
#[test]
fn applying_a_local_image_paste_leaves_file_work_to_the_app() {
    let (_state, mut engine) = crate::state::tests::test_state();
    let sid = engine.read().workspace_at(0).unwrap().all_surface_ids()[0];
    let input = terminal_with_sink(&mut engine, sid);
    let target = SurfaceBinding::capture(&engine.read(), sid).expect("binding");
    crate::app::engine_action::EngineAction::PasteImage {
        target,
        view: std::sync::Weak::new(),
        bracketed: false,
        file_name: "paste-6.png".into(),
        png_bytes: b"png".to_vec(),
    }
    .apply(&mut engine.borrow_mut(), None);
    assert!(
        input.try_recv().is_err(),
        "no path before the file is saved"
    );
    let pending = std::mem::take(&mut engine.borrow_mut().remote.pending_image_saves);
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].png_bytes, b"png");
}
