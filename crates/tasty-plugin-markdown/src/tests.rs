use super::*;

#[test]
fn reload_with_surface_id_returns_ok() {
    let mut p = MarkdownPlugin::new(Translator::default());
    let resp = p
        .markdown_reload(&json!({ "surface": 42 }))
        .expect("reload should succeed");
    assert_eq!(resp["ok"], json!(true));
    assert_eq!(resp["surface_id"], json!(42));
}

#[test]
fn reload_without_surface_id_is_invalid_params() {
    let mut p = MarkdownPlugin::new(Translator::default());
    let err = p.markdown_reload(&json!({})).unwrap_err();
    assert_eq!(err.code, -32602);
    assert!(err.message.contains("surface"));
}

#[test]
fn create_surface_loads_missing_file_as_error() {
    let mut p = MarkdownPlugin::new(Translator::default());
    // SDK 가 넘기는 envelope 형태(file 은 nested `params.file`)로 구성한다.
    p.create_surface(SurfaceCreateCtx {
        surface_id: 1,
        kind: "markdown".into(),
        cwd: None,
        params: json!({
            "surface_id": 1,
            "kind": "markdown",
            "params": { "file": "\0nonexistent-md-for-test" }
        }),
    });
    let doc = p.docs.get(&1).expect("doc inserted");
    assert!(doc.load_error.is_some());
    assert!(doc.content.is_empty());
}

// round-trip 회귀: create 는 file 을 snapshot 으로 실어야 layout/preset 저장에
// file 이 보존된다(그래야 host 가 snapshot_cache→SavedSurface::Generic.data 로
// round-trip). 이게 없으면(SDK 기본 = 빈 SurfaceResult) plugin surface 가 저장 시
// 내용을 잃는다.
#[test]
fn create_surface_carries_file_in_snapshot() {
    let mut p = MarkdownPlugin::new(Translator::default());
    let res = p.create_surface(SurfaceCreateCtx {
        surface_id: 1,
        kind: "markdown".into(),
        cwd: None,
        params: json!({
            "surface_id": 1,
            "kind": "markdown",
            "params": { "file": "/tmp/x.md" }
        }),
    });
    assert_eq!(
        res.snapshot,
        Some(json!({ "file": "/tmp/x.md" })),
        "create 는 file 을 snapshot 으로 실어야 한다"
    );
}

// layout 재시작 복원 경로: restore 는 create 가 실은 snapshot({file}) 을 그대로
// 받아 같은 문서를 열고, snapshot 을 재반환해 다음 저장에도 file 을 유지한다.
#[test]
fn restore_surface_reopens_from_snapshot_and_re_carries_it() {
    let mut p = MarkdownPlugin::new(Translator::default());
    let res = p.restore_surface(SurfaceRestoreCtx {
        surface_id: 2,
        kind: "markdown".into(),
        data: json!({ "file": "/tmp/x.md" }),
    });
    assert!(p.docs.contains_key(&2), "restore 가 문서를 연다");
    assert_eq!(
        res.snapshot,
        Some(json!({ "file": "/tmp/x.md" })),
        "restore 도 snapshot 을 재반환해야 다음 저장에 file 이 남는다"
    );
}

// file 없는 빈 markdown 은 저장할 snapshot 이 없어 None — 호스트는 기존 캐시 유지.
#[test]
fn create_without_file_yields_no_snapshot() {
    let mut p = MarkdownPlugin::new(Translator::default());
    let res = p.create_surface(SurfaceCreateCtx {
        surface_id: 3,
        kind: "markdown".into(),
        cwd: None,
        params: json!({ "surface_id": 3, "kind": "markdown", "params": {} }),
    });
    assert_eq!(res.snapshot, None);
}

/// 크기게이트 임계값 판정 — 초과만 게이트(경계값·이하·부재는 통과). host
/// `file/dispatch.rs` 의 `size_gate_boundary_and_over` 를 plugin in-process 로 이관.
#[test]
fn file_exceeds_limit_gates_over_only() {
    let dir = std::env::temp_dir();
    let big = dir.join(format!("tasty-md-big-{}.md", std::process::id()));
    let exact = dir.join(format!("tasty-md-exact-{}.md", std::process::id()));
    let small = dir.join(format!("tasty-md-small-{}.md", std::process::id()));
    std::fs::write(&big, vec![b'x'; LARGE_FILE_LIMIT_BYTES as usize + 1]).unwrap();
    std::fs::write(&exact, vec![b'x'; LARGE_FILE_LIMIT_BYTES as usize]).unwrap();
    std::fs::write(&small, vec![b'x'; 500 * 1024]).unwrap();

    assert_eq!(
        file_exceeds_limit(big.to_str().unwrap()),
        Some(LARGE_FILE_LIMIT_BYTES + 1)
    );
    // 정확히 임계값 → None (초과만 게이트).
    assert_eq!(file_exceeds_limit(exact.to_str().unwrap()), None);
    assert_eq!(file_exceeds_limit(small.to_str().unwrap()), None);
    // 없는 파일 → None (게이트 통과, 로드 시 error 표시).
    assert_eq!(file_exceeds_limit("\0nonexistent-md-for-test"), None);

    let _ = std::fs::remove_file(&big); // best-effort 정리 — 실패 무시(테스트 결과 무관).
    let _ = std::fs::remove_file(&exact); // best-effort 정리 — 실패 무시.
    let _ = std::fs::remove_file(&small); // best-effort 정리 — 실패 무시.
}

/// deferred 문서는 [열기] 확정(`resume_load`) 전까지 read 를 보류한다.
#[test]
fn deferred_doc_holds_read_until_resume() {
    let path = std::env::temp_dir().join(format!("tasty-md-deferred-{}.md", std::process::id()));
    std::fs::write(&path, b"# hello deferred").unwrap();
    let mut doc = MdDoc::new_deferred(Some(path.to_string_lossy().into_owned()));
    assert!(doc.pending_large);
    assert!(doc.content.is_empty());
    // 확정 후 실제 로드.
    doc.resume_load();
    assert!(!doc.pending_large);
    assert!(doc.content.contains("hello deferred"));
    // best-effort 정리 — 실패해도 테스트 결과에 영향 없음.
    let _ = std::fs::remove_file(&path);
}

/// `force_reload` 가 외부 삭제를 error 상태로 감지한다 — idle 감시(`watch.rs`)가
/// mtime 변경을 감지했을 때, 그리고 `markdown.reload` IPC 가 명시 호출됐을 때 모두
/// 이 경로 하나로 수렴한다(watch.rs 모듈 문서 — 레이스를 없애는 단일 쓰기 경로).
#[test]
fn force_reload_detects_external_deletion_as_error() {
    let path = std::env::temp_dir().join(format!("tasty-md-delpoll-{}.md", std::process::id()));
    std::fs::write(&path, b"# hello poll").unwrap();
    let mut doc = MdDoc::new(Some(path.to_string_lossy().into_owned()));
    // 정상 로드 baseline.
    assert!(!doc.content.is_empty());
    assert!(doc.load_error.is_none());

    // 외부 삭제 → force_reload 가 read_now 실패를 load_error 로 남긴다.
    std::fs::remove_file(&path).unwrap();
    doc.force_reload();
    assert!(
        doc.load_error.is_some(),
        "삭제가 error 상태로 감지되어야 한다"
    );

    // best-effort 정리 — 이미 삭제되었을 수 있음.
    let _ = std::fs::remove_file(&path);
}

#[test]
fn format_size_and_basename_examples() {
    assert_eq!(format_size(2 * 1024 * 1024 + 200 * 1024), "2.2 MB");
    assert_eq!(format_size(12 * 1024 * 1024), "12 MB");
    #[cfg(not(windows))]
    assert_eq!(basename("/docs/big-notes.md"), "big-notes.md");
}

#[test]
fn parse_recent_extracts_paths_in_order() {
    let v = json!({ "recent": [
        { "path": "/a/first.md", "file_name": "first.md" },
        { "path": "/b/second.md", "file_name": "second.md" },
    ]});
    assert_eq!(parse_recent(&v), vec!["/a/first.md", "/b/second.md"]);
}

#[test]
fn parse_recent_tolerates_missing_or_malformed() {
    assert!(parse_recent(&json!({})).is_empty());
    assert!(parse_recent(&json!({ "recent": "nope" })).is_empty());
    // path 없는 항목은 건너뛴다.
    assert_eq!(
        parse_recent(&json!({ "recent": [{ "file_name": "x" }, { "path": "/ok.md" }] })),
        vec!["/ok.md"]
    );
}

#[test]
fn surface_param_file_reads_nested_and_flat() {
    assert_eq!(
        surface_param_file(&json!({ "params": { "file": "/a/b.md" } })).as_deref(),
        Some("/a/b.md")
    );
    assert_eq!(
        surface_param_file(&json!({ "file": "/c/d.md" })).as_deref(),
        Some("/c/d.md")
    );
    assert_eq!(surface_param_file(&json!({ "params": {} })), None);
}

/// `reload_webview` 는 `self.host` 가 없으면(on_start 전) 조용히 no-op 해야 한다 —
/// panic 하지 않고 경고 로그만 남긴다.
#[test]
fn reload_webview_without_host_is_noop() {
    let mut p = MarkdownPlugin::new(Translator::default());
    p.docs.insert(7, MdDoc::new(None));
    p.reload_webview(7); // host 없음 — panic 하지 않아야 한다.
}
