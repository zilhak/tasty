use super::*;

#[test]
fn reload_of_an_open_document_returns_ok() {
    let mut p = MarkdownPlugin::new(Translator::default());
    p.docs.insert(42, MdDoc::new(None));
    let resp = p
        .markdown_reload(&json!({ "surface": 42 }))
        .expect("reload should succeed");
    assert_eq!(resp["ok"], json!(true));
    assert_eq!(resp["surface_id"], json!(42));
}

/// 이 plugin이 모르는 surface ID는 성공으로 답하지 않는다.
#[test]
fn reload_of_an_unknown_surface_is_invalid_params() {
    let mut p = MarkdownPlugin::new(Translator::default());
    let err = p
        .markdown_reload(&json!({ "surface": 42 }))
        .expect_err("unknown surface must be rejected");
    assert_eq!(err.code, -32602);
    assert!(err.message.contains("42"), "{}", err.message);
    assert!(
        err.message.contains("not a markdown surface"),
        "{}",
        err.message
    );
}

/// 감시의 자기 호출은 surface 해제와 엇갈릴 수 있어 닫힌 surface를 오류로 다루지 않는다.
#[test]
fn watch_reload_of_a_closed_surface_is_ignored() {
    let mut p = MarkdownPlugin::new(Translator::default());
    let resp = p
        .watch_reload(&json!({ "surface": 42 }))
        .expect("closed surface must not fail the watch request");
    assert_eq!(resp["ok"], json!(true));
    assert!(p.docs.is_empty());
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

// 생성 결과의 snapshot에 경로를 넣어 다음 실행에서 복원할 수 있어야 한다.
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

/// 확인 기준을 초과한 파일만 대용량으로 판정한다.
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

/// 파일이 삭제되면 다시 읽을 때 load_error에 기록한다.
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

fn mirror_create(p: &mut MarkdownPlugin, surface_id: u32, file: &str) -> SurfaceResult {
    p.create_surface(SurfaceCreateCtx {
        surface_id,
        kind: "markdown".into(),
        cwd: None,
        params: json!({
            "surface_id": surface_id,
            "kind": "markdown",
            "params": { "display_name": "notes.md", "remote": { "file": file } }
        }),
    })
}

fn content_result(
    surface_id: u32,
    request_id: u64,
    ok: bool,
    source: &str,
) -> MirrorContentResultWire {
    MirrorContentResultWire {
        surface_id,
        request_id,
        ok,
        source: ok.then(|| source.to_string()),
        reason: (!ok).then(|| source.to_string()),
    }
}

/// mirror 문서는 경로가 이 머신에 실재해도 읽지 않는다 — 그 경로는 원격 호스트의 것이다.
#[test]
fn remote_create_never_reads_the_local_file() {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path =
        std::env::temp_dir().join(format!("tasty-md-remote-{}-{seq}.md", std::process::id()));
    std::fs::write(&path, b"# local content").unwrap();
    let mut p = MarkdownPlugin::new(Translator::default());
    let res = mirror_create(&mut p, 5, &path.to_string_lossy());
    let doc = p.docs.get(&5).expect("doc inserted");
    assert!(doc.content.is_empty(), "로컬 파일을 읽으면 안 된다");
    assert!(doc.load_error.is_none());
    assert!(
        doc.base_dir.is_none(),
        "상대경로가 로컬 디렉토리로 풀리면 안 된다"
    );
    assert_eq!(
        doc.file_path.as_deref(),
        Some(path.to_string_lossy().as_ref())
    );
    assert!(doc.remote.is_some());
    // mirror 워크스페이스는 저장되지 않는다 — 원격 경로를 로컬 layout 에 남기지 않는다.
    assert_eq!(res.snapshot, None);

    // 재읽기 경로(idle 감시·markdown.reload)도 로컬 read 로 흘러가지 않는다.
    p.docs.get_mut(&5).unwrap().force_reload();
    assert!(p.docs[&5].content.is_empty());
    let resp = p.markdown_reload(&json!({ "surface": 5 })).unwrap();
    assert_eq!(resp["ok"], json!(true));
    assert!(p.docs[&5].content.is_empty());

    let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시(테스트 결과 무관).
}

#[test]
fn surface_param_remote_file_requires_the_remote_object() {
    assert_eq!(
        surface_param_remote_file(&json!({ "params": { "remote": { "file": "/r/a.md" } } }))
            .as_deref(),
        Some("/r/a.md")
    );
    // 원격이 파일 없이 연 문서도 mirror 문서다.
    assert_eq!(
        surface_param_remote_file(&json!({ "params": { "remote": {} } })).as_deref(),
        Some("")
    );
    assert_eq!(
        surface_param_remote_file(&json!({ "params": { "file": "/l/a.md" } })),
        None
    );
    assert_eq!(
        surface_param_remote_file(&json!({ "remote": { "file": "/r/a.md" } })),
        None
    );
}

/// restore data 는 envelope 이 아니라 params 객체 그 자체다 — kind 대기 placeholder 가
/// 실제화될 때 host 가 생성 params 와 같은 모양을 싣는다. 로컬 문서의 snapshot
/// (`{"file": ...}`)은 mirror 문서로 읽히면 안 된다.
#[test]
fn remote_file_of_reads_restore_data_and_ignores_local_snapshots() {
    assert_eq!(
        remote_file_of(&json!({ "display_name": "a.md", "remote": { "file": "/r/a.md" } }))
            .as_deref(),
        Some("/r/a.md")
    );
    assert_eq!(remote_file_of(&json!({ "file": "/l/a.md" })), None);
}

#[test]
fn remote_result_applies_only_to_the_pending_request() {
    let mut doc = MdDoc::new_remote("/r/notes.md".into());
    // 대기 중인 요청이 없으면 아무것도 반영하지 않는다.
    assert!(!doc.apply_remote_result(content_result(1, 7, true, "# a")));
    assert!(doc.content.is_empty());

    doc.remote.as_mut().unwrap().pending = Some(8);
    doc.remote.as_mut().unwrap().stale = true;
    // 옛 요청의 늦은 회신은 버린다.
    assert!(!doc.apply_remote_result(content_result(1, 7, true, "# old")));
    assert!(doc.content.is_empty());

    assert!(doc.apply_remote_result(content_result(1, 8, true, "# new")));
    assert_eq!(doc.content, "# new");
    let remote = doc.remote.as_ref().unwrap();
    assert_eq!(remote.pending, None);
    assert!(remote.loaded);
    assert!(!remote.stale, "최신 원문을 받으면 stale 표시가 풀린다");
    assert_eq!(
        doc.remote_view(),
        Some(render::RemoteView {
            loading: false,
            stale: false,
            disconnected: false,
        })
    );
}

#[test]
fn remote_failure_moves_reason_into_load_error() {
    let mut doc = MdDoc::new_remote("/r/notes.md".into());
    doc.remote.as_mut().unwrap().pending = Some(3);
    assert!(doc.apply_remote_result(content_result(1, 3, false, "permission denied")));
    assert_eq!(doc.load_error.as_deref(), Some("permission denied"));
    assert_eq!(doc.remote.as_ref().unwrap().pending, None);
}

/// 연결이 끊겨 회신이 영영 안 올 때 host 가 보내는 abandon 은 기다리던 요청을 끝내고 문서를
/// 끊김 상태로 둔다 — 이미 원문을 받은 문서도 그렇다. 받은 원문은 지우지 않고, 다음 성공
/// 회신이 끊김을 푼다.
#[test]
fn abandon_marks_the_document_disconnected_even_after_content_arrived() {
    let mut loading = MdDoc::new_remote("/r/notes.md".into());
    loading.remote.as_mut().unwrap().pending = Some(4);
    assert!(loading.apply_remote_result(content_result(
        1,
        MIRROR_ABANDON_REQUEST_ID,
        false,
        "gone"
    )));
    assert_eq!(loading.remote.as_ref().unwrap().pending, None);
    assert!(loading.remote_view().unwrap().disconnected);

    let mut loaded = MdDoc::new_remote("/r/notes.md".into());
    loaded.remote.as_mut().unwrap().pending = Some(5);
    assert!(loaded.apply_remote_result(content_result(1, 5, true, "# kept")));
    assert!(
        loaded.apply_remote_result(content_result(1, MIRROR_ABANDON_REQUEST_ID, false, "gone")),
        "원문을 받은 문서도 끊김을 그리러 다시 그린다"
    );
    assert!(loaded.remote_view().unwrap().disconnected);
    assert_eq!(loaded.content, "# kept", "받은 원문은 지우지 않는다");
    assert!(
        !loaded.apply_remote_result(content_result(1, MIRROR_ABANDON_REQUEST_ID, false, "gone")),
        "이미 끊김이면 다시 그리지 않는다"
    );

    loaded.remote.as_mut().unwrap().pending = Some(6);
    assert!(loaded.apply_remote_result(content_result(1, 6, true, "# back")));
    assert!(!loaded.remote_view().unwrap().disconnected);
    assert_eq!(loaded.content, "# back");
}

#[test]
fn change_signal_marks_a_shown_document_stale_once_and_ignores_local_documents() {
    let mut doc = MdDoc::new_remote("/r/notes.md".into());
    doc.remote.as_mut().unwrap().pending = Some(1);
    assert!(doc.apply_remote_result(content_result(1, 1, true, "# shown")));
    assert_eq!(doc.on_remote_changed(), RemoteChange::Redraw);
    assert_eq!(
        doc.on_remote_changed(),
        RemoteChange::Ignore,
        "이미 stale 이면 다시 그리지 않는다"
    );
    assert_eq!(doc.content, "# shown", "신호만으로 원문을 다시 받지 않는다");
    assert_eq!(MdDoc::new(None).on_remote_changed(), RemoteChange::Ignore);
}

/// 원문 대신 끊김·실패를 보여 주는 문서는 신호에 다시 요청한다 — 재연결 직후 host 가 보내는
/// 신호가 끊김 화면을 스스로 걷어내는 경로다. 이미 받는 중이면 그 회신을 기다린다.
#[test]
fn change_signal_refetches_a_document_that_shows_no_content() {
    let mut disconnected = MdDoc::new_remote("/r/notes.md".into());
    disconnected.remote.as_mut().unwrap().pending = Some(1);
    assert!(disconnected.apply_remote_result(content_result(1, 1, true, "# shown")));
    assert!(disconnected.apply_remote_result(content_result(
        1,
        MIRROR_ABANDON_REQUEST_ID,
        false,
        "gone"
    )));
    assert_eq!(disconnected.on_remote_changed(), RemoteChange::Refetch);
    assert!(
        !disconnected.remote.as_ref().unwrap().stale,
        "끊김 화면에는 stale 표시를 켜지 않는다"
    );

    let mut failed = MdDoc::new_remote("/r/notes.md".into());
    failed.remote.as_mut().unwrap().pending = Some(2);
    assert!(failed.apply_remote_result(content_result(1, 2, false, "permission denied")));
    assert_eq!(failed.on_remote_changed(), RemoteChange::Refetch);

    failed.remote.as_mut().unwrap().pending = Some(3);
    assert_eq!(failed.on_remote_changed(), RemoteChange::Ignore);
}

// ---- 찾아보기… 의 출발 폴더 ----

#[test]
fn picker_start_reads_local_observed_cwd_not_the_gated_cwd() {
    // `inherit_cwd` 가 꺼져 `cwd` 가 null 이어도 피커는 보고 있던 폴더에서 출발한다.
    let start = PickerStart::from_context(&json!({
        "cwd": null,
        "observed_cwd": "/work/proj",
        "origin_surface_id": 7,
    }));
    assert_eq!(start.dir.as_deref(), Some("/work/proj"));
    assert_eq!(start.origin_surface_id, Some(7));
}

#[test]
fn picker_start_on_mirror_reads_remote_cwd_only() {
    // mirror 면 원격 경로 키만 읽는다 — 로컬 키가 섞여 와도 원격 피커에 로컬 경로를 싣지 않는다.
    let start = PickerStart::from_context(&json!({
        "mirror": true,
        "local_surface_id": 9,
        "origin_surface_id": 9,
        "remote_cwd": "/srv/remote",
        "observed_cwd": "/should/not/be/used",
    }));
    assert_eq!(start.dir.as_deref(), Some("/srv/remote"));
    assert_eq!(start.origin_surface_id, Some(9));
    assert!(start.remote);
}

// ---- 확정 경로 조합 ----

fn start(dir: Option<&str>, remote: bool) -> PickerStart {
    PickerStart {
        dir: dir.map(str::to_string),
        origin_surface_id: Some(1),
        remote,
    }
}

/// 로컬 surface의 상대경로는 observed_cwd 기준으로 조합한다. 구분자는 로컬 OS 규칙이다.
#[test]
fn a_local_relative_path_joins_the_observed_cwd() {
    let expected = std::path::Path::new("/work/proj")
        .join("docs/a.md")
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        popup::resolve_confirmed_path("docs/a.md", &start(Some("/work/proj"), false)),
        expected
    );
}

#[cfg(unix)]
#[test]
fn a_local_absolute_path_is_kept() {
    assert_eq!(
        popup::resolve_confirmed_path("/etc/a.md", &start(Some("/work/proj"), false)),
        "/etc/a.md"
    );
}

/// 원격 경로는 로컬 OS와 관계없이 원격 cwd의 모양으로 구분자를 고른다.
#[test]
fn a_remote_relative_path_uses_the_remote_separator() {
    let r = |dir: &str, input: &str| popup::resolve_confirmed_path(input, &start(Some(dir), true));
    assert_eq!(r("/srv/remote", "a.md"), "/srv/remote/a.md");
    assert_eq!(r("/srv/remote/", "docs/a.md"), "/srv/remote/docs/a.md");
    assert_eq!(r("C:\\Users\\me", "a.md"), "C:\\Users\\me\\a.md");
    assert_eq!(r("C:\\Users\\me\\", "a.md"), "C:\\Users\\me\\a.md");
    assert_eq!(r("\\\\host\\share", "a.md"), "\\\\host\\share\\a.md");
}

#[test]
fn a_remote_absolute_path_is_kept_whatever_its_os() {
    let r = |input: &str| popup::resolve_confirmed_path(input, &start(Some("/srv/remote"), true));
    assert_eq!(r("/etc/a.md"), "/etc/a.md");
    assert_eq!(r("D:\\notes\\a.md"), "D:\\notes\\a.md");
    assert_eq!(r("D:/notes/a.md"), "D:/notes/a.md");
    assert_eq!(r("\\\\host\\share\\a.md"), "\\\\host\\share\\a.md");
}

/// cwd를 모르거나 `~`로 시작하면 조합하지 않고 입력을 그대로 보낸다.
#[test]
fn an_unresolvable_path_is_sent_as_typed() {
    assert_eq!(
        popup::resolve_confirmed_path("a.md", &start(None, true)),
        "a.md"
    );
    assert_eq!(
        popup::resolve_confirmed_path("a.md", &start(Some(""), false)),
        "a.md"
    );
    assert_eq!(
        popup::resolve_confirmed_path("~/a.md", &start(Some("/srv/remote"), true)),
        "~/a.md"
    );
}

#[test]
fn picker_start_tolerates_missing_keys() {
    // 구버전 host(`cwd` 만 싣는다) — 깨지지 않고 비어 있다.
    assert_eq!(
        PickerStart::from_context(&json!({ "cwd": "/old" })),
        PickerStart::default()
    );
}

#[cfg(any(unix, windows))]
#[test]
fn trigger_params_carry_start_dir_and_origin() {
    let start = PickerStart {
        dir: Some("/work/proj".to_string()),
        origin_surface_id: Some(7),
        remote: false,
    };
    let params = popup::file_picker_trigger_params(42, &start);
    assert_eq!(params["owner_popup_instance"], json!(42));
    assert_eq!(params["start_dir"], json!("/work/proj"));
    assert_eq!(params["origin_surface_id"], json!(7));
    assert_eq!(params["filters"], json!(["md", "markdown"]));
}

/// 파일열기 팝업 [열기] 는 자기 팝업을 실어 보낸다 — host 가 이것으로 사용자 조작을 알아본다.
/// 빠지면 사용자가 연 탭이 에이전트 요청으로 분류된다(docs/features/file-handler/index.md#origin-소유권과-비동기-완료).
#[cfg(any(unix, windows))]
#[test]
fn file_open_dispatch_params_carry_the_owner_popup() {
    let params = popup::file_open_dispatch_params(42, "/work/a.md");
    assert_eq!(params["owner_popup_instance"], json!(42));
    assert_eq!(params["path"], json!("/work/a.md"));
    assert_eq!(params["depth"], json!("deep"));
    assert!(params.get("origin_surface_id").is_none());
}

/// 파일열기 팝업의 제자리 변환도 자기 팝업을 실어 보낸다. 주소창 이동은 팝업이 없어 싣지 않는다.
#[test]
fn navigate_params_carry_the_owner_popup_only_from_the_popup() {
    let from_popup = navigate_params(3, " /work/a.md ", Some(42)).expect("params");
    assert_eq!(
        from_popup,
        json!({ "surface_id": 3, "path": "/work/a.md", "owner_popup_instance": 42 })
    );
    let from_addr = navigate_params(3, "/work/a.md", None).expect("params");
    assert!(from_addr.get("owner_popup_instance").is_none());
    assert!(navigate_params(3, "  ", Some(42)).is_none());
}

/// 외부 호출자가 `owner_popup_instance` 를 넣어도 host 로 다시 보낼 때는 빠진다.
#[test]
fn a_forwarded_external_navigate_drops_the_owner_popup() {
    let forwarded = forwarded_navigate_params(
        json!({ "surface_id": 3, "path": "/work/a.md", "owner_popup_instance": 42 }),
    );
    assert_eq!(forwarded, json!({ "surface_id": 3, "path": "/work/a.md" }));
}

/// 링크 열기 요청에 호스트가 보낸 URL과 원래 surface를 전달한다.
/// 호스트가 사용자 입력을 확인하고 같은 Pane에 새 탭을 연다.
#[test]
fn a_file_link_echoes_the_navigation_it_came_from() {
    let url = "about:blank#tasty-nav:link:b.md";
    let params = file_link_params(7, url, std::path::Path::new("/work/b.md"));
    assert_eq!(
        params,
        json!({
            "path": "/work/b.md",
            "depth": "deep",
            "origin_surface_id": 7,
            "user_navigation_url": url,
        })
    );
}

/// 외부 링크는 host `webview.open_external` 로 간다 — host 가 읽는 두 키(`surface_id` · `url`)를
/// 싣는다. 이 plugin 은 OS 열기를 직접 하지 않는다(docs/surfaces/markdown/index.md#내부-동작).
#[test]
fn external_link_params_name_the_clicked_surface_and_the_url() {
    let params = external_link_params(7, "https://example.com/a?b=1");
    assert_eq!(params["surface_id"], json!(7));
    assert_eq!(params["url"], json!("https://example.com/a?b=1"));
}

/// 원격 원문 요청은 에이전트가 건 것에만 `agent_origin` 을 싣는다 — host 는 그 회신의 잘림
/// toast 를 사용자에게 띄우지 않는다(docs/design/systems/toast.md#origin이-적용되는-경로). plugin 이 건 요청은 종전 모양 그대로다.
#[test]
fn only_an_agent_content_request_carries_the_agent_origin() {
    assert_eq!(
        mirror_content_request_params(3, RemoteRequester::Agent),
        json!({ "surface_id": 3, "agent_origin": true })
    );
    assert_eq!(
        mirror_content_request_params(3, RemoteRequester::Plugin),
        json!({ "surface_id": 3 })
    );
}

#[test]
fn set_url_sends_the_document_path_as_the_chrome_label() {
    let params = set_url_params(7, "/docs/readme.md", "<html>body</html>".to_string());
    assert_eq!(params["surface_id"], json!(7));
    assert_eq!(params["url"], json!("<html>body</html>"));
    assert_eq!(params["label"], json!("/docs/readme.md"));
}

#[test]
fn set_url_without_a_path_omits_the_label() {
    let params = set_url_params(7, "", "<html>body</html>".to_string());
    assert!(params.get("label").is_none());
}

fn temp_md(tag: &str, body: &[u8]) -> std::path::PathBuf {
    static SEQ: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!("tasty-md-{tag}-{}-{seq}.md", std::process::id()));
    std::fs::write(&path, body).unwrap();
    path
}

/// 확인을 기다리거나 취소한 문서는 파일이 바뀌어도 감시로 읽지 않는다.
#[test]
fn watch_does_not_read_a_document_waiting_for_the_large_file_answer() {
    let path = temp_md("watch-pending", b"# changed while pending");
    let mut p = MarkdownPlugin::new(Translator::default());
    p.docs.insert(
        7,
        MdDoc::new_deferred(Some(path.to_string_lossy().into_owned())),
    );
    p.watch_reload(&json!({ "surface": 7 })).unwrap();
    assert!(p.docs[&7].pending_large);
    assert!(p.docs[&7].content.is_empty());
    let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시(테스트 결과 무관).
}

/// 명시적 reload는 확인을 다시 거친다. 파일이 기준 아래로 줄었으면 바로 읽는다.
#[test]
fn explicit_reload_reads_a_pending_document_that_is_now_small() {
    let path = temp_md("reload-pending", b"# now small");
    let mut p = MarkdownPlugin::new(Translator::default());
    p.docs.insert(
        8,
        MdDoc::new_deferred(Some(path.to_string_lossy().into_owned())),
    );
    // 확인을 취소해 팝업이 닫힌 상태.
    p.on_large_confirm_closed(8);
    let resp = p.markdown_reload(&json!({ "surface": 8 })).unwrap();
    assert_eq!(resp["deferred"], json!(false));
    assert!(!p.docs[&8].pending_large);
    assert!(p.docs[&8].content.contains("now small"));
    let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시(테스트 결과 무관).
}

#[test]
fn reload_step_applies_the_large_file_gate() {
    use ReloadOrigin::{Explicit, Watch};
    let loaded = MdDoc::new(None);
    // 확인을 취소한 문서: 대기 상태이고 팝업은 닫혔다.
    let mut pending = MdDoc::new_deferred(None);
    pending.confirm_requested = false;
    // 확인을 요청했고 팝업이 아직 닫히지 않은 문서(팝업이 열리기 전 포함).
    let asking = MdDoc::new_deferred(None);
    let mut confirmed = MdDoc::new(None);
    confirmed.large_confirmed = true;
    let big = Some(LARGE_FILE_LIMIT_BYTES + 1);

    // 확인을 기다리거나 취소한 문서: 감시는 읽지 않고, 명시 reload는 다시 묻는다.
    assert_eq!(reload_step(&pending, Watch, big, true), ReloadStep::Skip);
    assert_eq!(reload_step(&pending, Watch, None, true), ReloadStep::Skip);
    assert_eq!(
        reload_step(&pending, Explicit, big, true),
        ReloadStep::Ask(LARGE_FILE_LIMIT_BYTES + 1)
    );
    // 이미 확인을 요청했으면 팝업이 열리기 전이라도 겹쳐 묻지 않는다.
    assert_eq!(reload_step(&asking, Explicit, big, true), ReloadStep::Skip);
    assert_eq!(
        reload_step(&pending, Explicit, None, true),
        ReloadStep::Read
    );

    // 읽어 둔 문서가 기준을 넘게 커지면 감시와 명시 reload 모두 묻는다.
    assert_eq!(
        reload_step(&loaded, Watch, big, true),
        ReloadStep::Ask(LARGE_FILE_LIMIT_BYTES + 1)
    );
    assert_eq!(
        reload_step(&loaded, Explicit, big, true),
        ReloadStep::Ask(LARGE_FILE_LIMIT_BYTES + 1)
    );
    assert_eq!(reload_step(&loaded, Watch, None, true), ReloadStep::Read);

    // [열기]를 고른 문서는 다시 묻지 않는다.
    assert_eq!(reload_step(&confirmed, Watch, big, true), ReloadStep::Read);

    // 확인을 띄울 수 없으면 생성 때처럼 읽는다.
    assert_eq!(reload_step(&loaded, Watch, big, false), ReloadStep::Read);
    assert_eq!(
        reload_step(&pending, Explicit, big, false),
        ReloadStep::Read
    );
}

/// [열기]를 고르면 이후 reload에서 다시 묻지 않도록 기록한다.
#[test]
fn resume_load_remembers_the_confirmation() {
    let path = temp_md("confirm", b"# confirmed body");
    let mut doc = MdDoc::new_deferred(Some(path.to_string_lossy().into_owned()));
    assert!(!doc.large_confirmed);
    doc.resume_load();
    assert!(doc.large_confirmed);
    assert!(doc.content.contains("confirmed body"));
    let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시(테스트 결과 무관).
}

/// 읽어 둔 문서가 커져 확인을 묻는 동안 이전 내용을 보이고, 취소하면 그때 비운다.
/// [열기]로 닫히면 새 내용이 남는다.
#[test]
fn a_grown_document_keeps_old_content_until_the_answer() {
    let path = temp_md("grown", b"# old body");
    let file = path.to_string_lossy().into_owned();
    let mut p = MarkdownPlugin::new(Translator::default());
    p.docs.insert(9, MdDoc::new(Some(file.clone())));
    p.docs.get_mut(&9).unwrap().pending_large = true;
    std::fs::write(&path, b"# new body").unwrap();

    // 확인을 기다리는 동안 감시는 읽지 않고 이전 내용이 남는다.
    p.watch_reload(&json!({ "surface": 9 })).unwrap();
    assert!(p.docs[&9].content.contains("old body"));

    // 취소로 닫히면 비우고 대기 상태로 남는다.
    p.on_large_confirm_closed(9);
    assert!(p.docs[&9].pending_large);
    assert!(p.docs[&9].content.is_empty());

    // [열기]로 닫히면(읽기를 먼저 마친다) 새 내용이 남는다.
    p.docs.get_mut(&9).unwrap().resume_load();
    p.on_large_confirm_closed(9);
    assert!(p.docs[&9].content.contains("new body"));
    let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시(테스트 결과 무관).
}

/// 확인 요청 표시는 팝업이 열리기 전부터 서고, 팝업이 닫히면([열기]·취소 모두) 내린다.
#[test]
fn the_confirm_request_mark_is_set_before_the_popup_and_cleared_on_close() {
    let path = temp_md("asking", b"# body");
    let mut p = MarkdownPlugin::new(Translator::default());
    p.docs.insert(
        11,
        MdDoc::new_deferred(Some(path.to_string_lossy().into_owned())),
    );
    assert!(p.docs[&11].confirm_requested);
    assert!(p.confirm.is_empty(), "팝업은 아직 열리지 않았다");
    p.on_large_confirm_closed(11);
    assert!(!p.docs[&11].confirm_requested);
    let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시(테스트 결과 무관).
}

/// 파일을 읽지 않고 사용자 확인을 기다리면 reload 응답이 그 사실을 알린다.
#[test]
fn reload_reports_whether_the_read_was_deferred() {
    let path = temp_md("deferred-resp", b"# small");
    let file = path.to_string_lossy().into_owned();
    let mut p = MarkdownPlugin::new(Translator::default());
    // 확인 팝업이 아직 닫히지 않은 대기 문서는 읽지 않는다.
    p.docs.insert(12, MdDoc::new_deferred(Some(file.clone())));
    let resp = p.markdown_reload(&json!({ "surface": 12 })).unwrap();
    assert_eq!(resp["deferred"], json!(true));
    assert!(p.docs[&12].content.is_empty());
    // 읽은 문서는 false.
    p.docs.insert(13, MdDoc::new(Some(file)));
    let resp = p.markdown_reload(&json!({ "surface": 13 })).unwrap();
    assert_eq!(resp["deferred"], json!(false));
    let _ = std::fs::remove_file(&path); // best-effort 정리 — 실패 무시(테스트 결과 무관).
}

fn deferred_state_document(source: &str, load_error: Option<&str>, large_deferred: bool) -> String {
    let theme = Theme::with_colors_and_zoom(tasty_themes::mocha_fallback_colors(), false, 1.0);
    let tr = Translator::default();
    render::render_document(render::DocumentInput {
        theme: &theme,
        tr: &tr,
        file_path: "/a/big.md",
        source,
        load_error,
        base_dir: None,
        recent: &[],
        remote: None,
        large_deferred,
    })
}

/// 대용량 확인을 기다리거나 취소한 문서는 빈 파일로 보이지 않는다.
#[test]
fn a_deferred_large_document_is_not_shown_as_empty() {
    let tr = Translator::default();
    let deferred = tr.t("markdown.state.large_deferred");
    let empty = tr.t("markdown.state.empty");

    let html = deferred_state_document("", None, true);
    assert!(html.contains(deferred));
    assert!(!html.contains(empty));

    // 같은 빈 원문이라도 읽은 문서는 빈 파일 상태다.
    let html = deferred_state_document("", None, false);
    assert!(html.contains(empty));
    assert!(!html.contains(deferred));

    // 읽기 실패는 대기 상태보다 앞선다.
    let html = deferred_state_document("", Some("boom"), true);
    assert!(html.contains(tr.t("markdown.state.failed")));
    assert!(!html.contains(deferred));
}

/// 처음 열 때와 취소한 뒤에는 대기 상태를, 커진 파일을 묻는 동안에는 이전 내용을 보인다.
#[test]
fn the_deferred_state_is_shown_only_without_previous_content() {
    let mut doc = MdDoc::new_deferred(None);
    assert!(doc.shows_large_deferred());
    doc.content = "# previous".into();
    assert!(!doc.shows_large_deferred());
    doc.decline_large();
    assert!(doc.shows_large_deferred());
    doc.pending_large = false;
    assert!(!doc.shows_large_deferred());
}
