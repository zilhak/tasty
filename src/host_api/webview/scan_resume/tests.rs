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
        resume.start(&gate, &url, Some(1), StartKind::NewDocument, true),
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
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
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
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Reload, true),
        Start::Resumed(true)
    );
    assert!(
        !resume.completed(Some(11)),
        "다시 시작한 탐색의 완료는 실제 종료다"
    );
    assert!(doc.gate.committed(Some(&doc.url)));
    // 재시작 결과는 한 번만 쓴다. 다음 재로드는 다시 취소하고 읽는다.
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(12), StartKind::Reload, true),
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
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
        Start::Cancel
    );
    wait(|| resume.poll());
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Reload, true),
        Start::Resumed(false)
    );
}

/// 스캔 중 호스트가 새 로드를 요청하면 앞 스캔과 재시작을 버린다.
#[test]
fn a_host_request_during_the_scan_drops_the_replay() {
    let doc = allowed_doc("host");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
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
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
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
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::NewDocument, true),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Navigate);
    let other = format!("{}.other.html", doc.url);
    assert_eq!(
        resume.start(&doc.gate, &other, Some(11), StartKind::NewDocument, true),
        Start::Js(false)
    );
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(12), StartKind::NewDocument, true),
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
        resume.start(
            &doc.gate,
            &doc.url,
            Some(10),
            StartKind::BackOrForward,
            true
        ),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Back);
    assert_eq!(
        resume.start(
            &doc.gate,
            &doc.url,
            Some(11),
            StartKind::BackOrForward,
            true
        ),
        Start::Resumed(true)
    );
    doc.gate.committed(Some(&doc.url));
    // 페이지가 시작한 히스토리 이동은 방향을 모른다.
    assert_eq!(
        resume.start(
            &doc.gate,
            &doc.url,
            Some(12),
            StartKind::BackOrForward,
            true
        ),
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
        resume.start(&doc.gate, &doc.url, None, StartKind::Reload, true),
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
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
        Start::Cancel
    );
    resume.cancel_failed(Some(10));
    assert!(!resume.completed(Some(10)));
    assert!(!resume.waiting());
}

/// 앞으로 이동을 다시 시작한다. 종류를 모르는 탐색(구형 런타임)은 재시작을 알아볼 수 없어
/// 취소하지 않고 스크립트 없이 진행한다.
#[test]
fn forward_replays_and_an_unknown_kind_is_not_cancelled() {
    let doc = allowed_doc("kinds");
    let mut resume = ScanResume::new(1);
    resume.host_request(Some(HistoryStep::Forward));
    assert_eq!(
        resume.start(
            &doc.gate,
            &doc.url,
            Some(10),
            StartKind::BackOrForward,
            true
        ),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Forward);
    // 다시 시작하는 호출이 실패하면 기다리던 재시작을 버린다.
    resume.replay_failed();
    assert!(!resume.waiting());
    resume.host_request(None);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Unknown, true),
        Start::Js(false)
    );
    assert!(!resume.completed(Some(11)));
}

/// 재시작을 기다리는 동안 같은 주소지만 종류가 다른 탐색이 먼저 오면 새 탐색이다.
/// 읽어 둔 결과를 쓰지 않고 다시 취소·스캔하며, 뒤에 온 실제 재시작도 새 스캔을 거친다.
#[test]
fn a_same_url_navigation_of_another_kind_does_not_take_the_replay_scan() {
    let doc = allowed_doc("samekind");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Reload);
    // 재시작(Reload) 신호 전에 같은 주소의 새 문서 탐색이 온다.
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::NewDocument, true),
        Start::Cancel,
        "읽어 둔 결과를 쓰지 않는다"
    );
    // 그 뒤에 온 실제 재시작 신호도 기다리는 재시작이 없으므로 새 탐색으로 취소된다.
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(12), StartKind::Reload, true),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Reload);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(13), StartKind::Reload, true),
        Start::Resumed(true)
    );
}

/// 페이지 스크립트가 시작한 같은 주소·같은 종류 탐색(IsUserInitiated 거짓)은 재시작이 아니다.
#[test]
fn a_page_script_navigation_does_not_take_the_replay_scan() {
    let doc = allowed_doc("script");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
        Start::Cancel
    );
    wait(|| resume.poll());
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Reload, false),
        Start::Cancel
    );
}

/// 주소·종류·사용자 시작 여부가 모두 같은 탐색은 재시작인지 알 수 없다(API 한계). 그래서 결과를
/// 파일 판에 묶는다. 판이 그대로면 그 탐색이 여는 내용이 스캔한 내용과 같아 결과를 받는다.
#[test]
fn an_identical_navigation_gets_the_scan_only_for_the_same_file_version() {
    let doc = allowed_doc("identical");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::NewDocument, true),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Navigate);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::NewDocument, true),
        Start::Resumed(true)
    );
}

/// 재시작을 기다리는 동안 같은 주소의 파일을 제자리에서 바꾸면, 그 뒤 오는 탐색은 재시작이든 같은
/// 조건의 사용자 탐색이든 앞 결과를 받지 않는다. 다시 취소하고 자기 스캔으로 JS를 정한다.
#[test]
fn a_file_rewritten_before_the_replay_starts_is_scanned_again() {
    let doc = allowed_doc("rewrite");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Reload);
    std::fs::write(doc.dir.join("doc.html"), "<script>b()</script>").expect("rewrite");
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Reload, true),
        Start::Cancel,
        "바뀐 파일 판에는 앞 결과를 넘기지 않는다"
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Reload);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(12), StartKind::Reload, true),
        Start::Resumed(false),
        "자기 스캔은 허용한 지문과 달라 JS를 끈다"
    );
}

/// 이름 바꾸기로 같은 내용의 새 파일로 교체해도 다른 파일 판이다. 앞 결과를 넘기지 않고 다시 읽는다.
#[test]
fn a_file_replaced_by_rename_is_another_version() {
    let doc = allowed_doc("rename");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
        Start::Cancel
    );
    wait(|| resume.poll());
    let tmp = doc.dir.join("next.html");
    std::fs::write(&tmp, "<script>a()</script>").expect("write");
    std::fs::rename(&tmp, doc.dir.join("doc.html")).expect("rename");
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Reload, true),
        Start::Cancel
    );
    wait(|| resume.poll());
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(12), StartKind::Reload, true),
        Start::Resumed(true),
        "같은 내용이면 자기 스캔으로 다시 켠다"
    );
}

/// 경로가 사라지면 파일 판을 확인할 수 없어 결과를 넘기지 않는다.
#[test]
fn a_missing_file_gets_no_replay_scan() {
    let doc = allowed_doc("missing");
    let mut resume = ScanResume::new(1);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
        Start::Cancel
    );
    wait(|| resume.poll());
    std::fs::remove_file(doc.dir.join("doc.html")).expect("remove");
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::Reload, true),
        Start::Cancel
    );
}

/// 다시 시작한 탐색을 계속 알아보지 못하면(예: 실제 종류가 예상과 다름) 재시작을 두 번까지만 하고
/// 그 뒤에는 취소하지 않고 스크립트 없이 진행한다. 취소와 재시작이 끝없이 이어지지 않는다.
#[test]
fn unrecognised_replays_stop_after_the_limit() {
    let doc = allowed_doc("limit");
    let mut resume = ScanResume::new(1);
    // 재시작 신호가 매번 예상과 다른 종류로 오는 경우를 흉내 낸다.
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(10), StartKind::Reload, true),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Reload);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(11), StartKind::NewDocument, true),
        Start::Cancel
    );
    assert_eq!(wait(|| resume.poll()).0, Replay::Navigate);
    assert_eq!(MAX_UNMATCHED_REPLAYS, 2);
    let id = 12;
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(id), StartKind::Reload, true),
        Start::Js(false)
    );
    assert!(!doc.state.lock().expect("state").scan_pending());
    // 알아본 재시작이나 호스트 요청 뒤에는 다시 취소·재시작한다.
    resume.host_request(None);
    assert_eq!(
        resume.start(&doc.gate, &doc.url, Some(id + 1), StartKind::Reload, true),
        Start::Cancel
    );
}
