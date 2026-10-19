use std::sync::{Arc, Mutex};

use tasty_model::html_script::HtmlScriptState;

use super::*;

struct Doc {
    gate: ScriptGate,
    state: Arc<Mutex<HtmlScriptState>>,
    dir: std::path::PathBuf,
    url: String,
}

impl Drop for Doc {
    fn drop(&mut self) {
        if let Err(e) = std::fs::remove_dir_all(&self.dir) {
            tracing::warn!("temp dir cleanup failed: {e}");
        }
    }
}

fn wait<T>(mut f: impl FnMut() -> Option<T>) -> T {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        if let Some(v) = f() {
            return v;
        }
        assert!(std::time::Instant::now() < deadline, "timed out");
        std::thread::yield_now();
    }
}

/// 허용한 로컬 문서 하나를 화면에 둔 게이트.
fn allowed_doc(tag: &str) -> Doc {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "tasty-scan-resume-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("dir");
    let path = dir.join("doc.html");
    std::fs::write(&path, "<script>a()</script>").expect("write");
    let url = format!("file://{}", path.to_string_lossy().replace('\\', "/"));
    let url = if url.starts_with("file:///") {
        url
    } else {
        url.replacen("file://", "file:///", 1)
    };
    let state = Arc::new(Mutex::new(HtmlScriptState::new(true)));
    let gate = ScriptGate::new(1, state.clone());
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&gate, &url, Some(1), StartKind::NewDocument),
        Start::Js(false)
    );
    gate.committed(Some(&url));
    gate.finished();
    wait(|| (!state.lock().expect("state").scan_pending()).then_some(()));
    state.lock().expect("state").allow_current().expect("allow");
    Doc {
        gate,
        state,
        dir,
        url,
    }
}

/// 재로드: 취소 → 스캔 → 같은 종류로 다시 시작 → 그 탐색은 읽어 둔 결과로 JS를 켠다.
#[test]
fn an_allowed_reload_is_cancelled_scanned_and_replayed_as_a_reload() {
    let doc = allowed_doc("reload");
    let mut resume = ScanResume::new(1);
    resume.host_request(None);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload),
        Start::Cancel
    );
    assert!(
        resume.completed(Some(10)),
        "내부 취소의 완료는 로드 종료가 아니다"
    );
    assert!(!resume.completed(Some(10)), "한 번만 소비한다");
    let (replay, url) = wait(|| resume.poll());
    assert_eq!((replay, url.as_str()), (Replay::Reload, doc.url.as_str()));
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Reload),
        Start::Resumed(true)
    );
    assert!(
        !resume.completed(Some(11)),
        "다시 시작한 탐색의 완료는 실제 종료다"
    );
    assert!(doc.gate.committed(Some(&doc.url)));
    // 재시작 결과는 한 번만 쓴다. 다음 재로드는 다시 취소하고 읽는다.
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(12), StartKind::Reload),
        Start::Cancel
    );
}

/// 내용이 바뀐 파일은 다시 시작해도 켜지 않는다.
#[test]
fn a_changed_file_is_replayed_without_scripts() {
    let doc = allowed_doc("changed");
    std::fs::write(doc.dir.join("doc.html"), "<script>b()</script>").expect("rewrite");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload),
        Start::Cancel
    );
    wait(|| resume.poll());
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Reload),
        Start::Resumed(false)
    );
}

/// 스캔 중 호스트가 새 로드를 요청하면 앞 스캔과 재시작을 버린다.
#[test]
fn a_host_request_during_the_scan_drops_the_replay() {
    let doc = allowed_doc("host");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload),
        Start::Cancel
    );
    resume.host_request(None);
    assert!(!doc.state.lock().expect("state").scan_pending());
    assert!(!resume.waiting());
    assert_eq!(resume.poll(), None);
    assert!(
        resume.completed(Some(10)),
        "취소한 탐색의 완료는 여전히 내부 것이다"
    );
}

/// 읽기가 끝난 뒤 다시 시작하기 전에 다른 탐색이 시작되면 표가 낡아 다시 시작하지 않는다.
#[test]
fn a_navigation_between_the_read_and_the_replay_makes_the_ticket_stale() {
    let doc = allowed_doc("stale");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload),
        Start::Cancel
    );
    // 읽기가 끝나기 전후 어느 때든 다른 탐색이 끼어들면 재시작하지 않는다.
    doc.gate.load_started();
    wait(|| {
        assert_eq!(resume.poll(), None);
        (!resume.waiting()).then_some(())
    });
}

/// 재시작을 기다리는 동안 다른 주소의 탐색이 오면 재시작 결과를 버린다.
/// 그 뒤 원래 주소의 탐색이 와도 새 탐색으로 다시 취소하고 읽는다.
#[test]
fn another_url_while_waiting_for_the_replay_discards_it() {
    let doc = allowed_doc("other");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::NewDocument),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Navigate);
    let other = format!("{}.other.html", doc.url);
    assert_eq!(
        resume.start(&doc.gate, &other, Some(11), StartKind::NewDocument),
        Start::Js(false)
    );
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(12), StartKind::NewDocument),
        Start::Cancel
    );
}

/// 뒤로/앞으로는 호스트가 요청한 방향으로 다시 시작한다. 방향을 모르면 스크립트 없이 진행한다.
#[test]
fn history_steps_replay_in_the_requested_direction_only() {
    let doc = allowed_doc("history");
    let mut resume = ScanResume::new(1);
    resume.host_request(Some(HistoryStep::Back));
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::BackOrForward),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Back);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::BackOrForward),
        Start::Resumed(true)
    );
    doc.gate.committed(Some(&doc.url));
    // 페이지가 시작한 히스토리 이동은 방향을 모른다.
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(12), StartKind::BackOrForward),
        Start::Js(false)
    );
    assert!(!doc.state.lock().expect("state").scan_pending());
    assert!(!resume.completed(Some(12)));
}

/// NavigationId를 모르면 내부 취소를 구별할 수 없어 취소하지 않는다.
#[test]
fn an_unknown_navigation_id_is_not_cancelled() {
    let doc = allowed_doc("noid");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, None, StartKind::Reload),
        Start::Js(false)
    );
    assert!(!doc.state.lock().expect("state").scan_pending());
}

/// 취소가 실패하면 그 ID를 내부 취소로 보지 않고 대기를 버린다.
#[test]
fn a_failed_cancel_forgets_the_id() {
    let doc = allowed_doc("cancelfail");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload),
        Start::Cancel
    );
    resume.cancel_failed(Some(10));
    assert!(!resume.completed(Some(10)));
    assert!(!resume.waiting());
}

/// 앞으로 이동과 종류를 모르는 탐색도 다시 시작할 방법이 정해진다.
#[test]
fn forward_and_unknown_kinds_have_a_replay() {
    let doc = allowed_doc("kinds");
    let mut resume = ScanResume::new(1);
    resume.host_request(Some(HistoryStep::Forward));
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::BackOrForward),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Forward);
    // 다시 시작하는 호출이 실패하면 기다리던 재시작을 버린다.
    resume.replay_failed();
    assert!(!resume.waiting());
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Unknown),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Navigate);
}
