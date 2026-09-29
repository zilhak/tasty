use super::*;

const A: &str = "file:///docs/a.html";
const C: &str = "file:///docs/c.html";

fn scan(byte: u8) -> ScriptScan {
    ScriptScan {
        fingerprint: Fingerprint([byte; 32]),
        detection: ScriptDetection::Scripts,
    }
}

/// STARTED → RESPONSE → COMMITTED → FINISHED 순서의 main frame 로드. 단계별 JS 값을 돌려준다.
fn load(st: &mut HtmlScriptState, url: &str, s: Option<ScriptScan>) -> [bool; 3] {
    let started = st.on_load_started();
    let response = st.on_main_response(url, s);
    let committed = st.on_committed(Some(url));
    assert_eq!(
        st.on_load_finished(),
        None,
        "commit이 있었으면 복원하지 않는다"
    );
    [started, response, committed]
}

fn allowed_a() -> HtmlScriptState {
    let mut st = HtmlScriptState::new(true);
    load(&mut st, A, Some(scan(1)));
    st.allow_current().expect("allow");
    assert!(st.take_reload_request());
    assert_eq!(load(&mut st, A, Some(scan(1))), [false, true, true]);
    st
}

#[test]
fn a_first_load_under_the_sandbox_keeps_javascript_off() {
    let mut st = HtmlScriptState::new(true);
    assert_eq!(load(&mut st, A, Some(scan(1))), [false, false, false]);
    assert_eq!(st.current_detection(), Some(ScriptDetection::Scripts));
    assert!(st.allowance().is_none());
}

#[test]
fn allowing_records_the_current_document_and_requests_one_reload() {
    let mut st = HtmlScriptState::new(true);
    load(&mut st, A, Some(scan(1)));
    st.allow_current().expect("allow");
    assert_eq!(
        st.allowance(),
        Some(&Allowance {
            url: A.to_string(),
            fingerprint: Fingerprint([1; 32]),
        })
    );
    assert!(st.take_reload_request());
    assert!(!st.take_reload_request(), "요청은 한 번만 꺼낸다");
}

#[test]
fn the_allowed_document_runs_after_the_reload_and_on_later_reloads() {
    let mut st = allowed_a();
    assert!(st.current_is_allowed());
    // 사용자 재로드(F5)도 같은 문서면 허용을 유지한다.
    assert_eq!(load(&mut st, A, Some(scan(1))), [false, true, true]);
    assert!(st.allowance().is_some());
}

#[test]
fn a_fragment_in_the_url_does_not_make_another_document() {
    let mut st = allowed_a();
    assert_eq!(
        load(&mut st, "file:///docs/a.html#section", Some(scan(1))),
        [false, true, true]
    );
    assert_eq!(st.current().map(|d| d.url.as_str()), Some(A));
}

#[test]
fn another_document_clears_the_allowance_and_coming_back_needs_a_new_allow() {
    let mut st = allowed_a();
    assert_eq!(load(&mut st, C, Some(scan(2))), [false, false, false]);
    assert!(st.allowance().is_none());
    assert_eq!(load(&mut st, A, Some(scan(1))), [false, false, false]);
}

#[test]
fn a_changed_file_is_another_document() {
    let mut st = allowed_a();
    assert_eq!(load(&mut st, A, Some(scan(9))), [false, false, false]);
    assert!(st.allowance().is_none());
}

#[test]
fn a_load_that_ends_without_a_commit_restores_the_allowed_document() {
    let mut st = allowed_a();
    // 없는 파일: 응답 단계 없이 실패한다.
    assert!(!st.on_load_started());
    assert_eq!(st.on_load_finished(), Some(true));
    assert!(st.allowance().is_some());
    // 다운로드: main frame 응답은 있지만 commit 없이 끝난다.
    assert!(!st.on_load_started());
    assert!(!st.on_main_response("file:///docs/d.gz", None));
    assert_eq!(st.on_load_finished(), Some(true));
    assert!(st.current_is_allowed());
}

#[test]
fn a_load_that_ends_without_a_commit_keeps_an_unallowed_document_off() {
    let mut st = HtmlScriptState::new(true);
    load(&mut st, A, Some(scan(1)));
    st.on_load_started();
    assert_eq!(st.on_load_finished(), Some(false));
}

#[test]
fn a_commit_without_a_response_clears_the_allowance_and_cannot_be_allowed() {
    let mut st = allowed_a();
    st.on_load_started();
    assert!(!st.on_committed(Some("file:///docs/c.html#top")));
    assert!(st.allowance().is_none());
    assert_eq!(
        st.current(),
        Some(&DocumentRecord {
            url: C.to_string(),
            scan: None,
        })
    );
    assert_eq!(st.allow_current(), Err(AllowError::NoFingerprint));
}

#[test]
fn the_pending_response_does_not_leak_into_the_next_load() {
    let mut st = HtmlScriptState::new(true);
    load(&mut st, C, Some(scan(2)));
    load(&mut st, A, Some(scan(1)));
    st.allow_current().expect("allow");
    // 허용 재로드: 응답 단계에서 일치한다.
    st.on_load_started();
    assert!(st.on_main_response(A, Some(scan(1))));
    st.on_committed(Some(A));
    st.on_load_finished();
    // 뒤로·앞으로 가기가 응답 단계 없이 commit된다.
    st.on_load_started();
    st.on_committed(Some(C));
    st.on_load_finished();
    st.on_load_started();
    assert!(!st.on_committed(Some(A)));
    st.on_load_finished();
    assert!(st.allowance().is_none());
    // 그 뒤 commit 없는 로드는 JS를 복원하지 않는다.
    st.on_load_started();
    assert_eq!(st.on_load_finished(), Some(false));
}

#[test]
fn an_allowed_document_on_screen_does_not_turn_javascript_on_for_a_load_in_flight() {
    let mut st = allowed_a();
    st.on_load_started();
    assert!(!st.effective_js(), "응답 단계 전에는 끈다");
    st.on_main_response(C, Some(scan(2)));
    assert!(!st.effective_js());
    // 로드 중에 sandbox를 다시 켜도 화면 문서의 허용을 새 문서에 적용하지 않는다.
    assert!(!st.set_sandbox(true));
}

#[test]
fn turning_the_sandbox_off_runs_every_document() {
    let mut st = HtmlScriptState::new(false);
    assert_eq!(load(&mut st, A, Some(scan(1))), [true, true, true]);
    assert!(!st.set_sandbox(true));
    assert!(st.set_sandbox(false));
}

#[test]
fn allowing_needs_a_committed_document_with_a_fingerprint() {
    let mut st = HtmlScriptState::new(true);
    assert_eq!(st.allow_current(), Err(AllowError::NoDocument));
    load(&mut st, "about:blank", None);
    assert_eq!(st.allow_current(), Err(AllowError::NoFingerprint));
    assert!(!st.take_reload_request());
}

#[test]
fn banner_flags_reset_for_a_new_document_and_survive_a_reload() {
    let mut st = HtmlScriptState::new(true);
    load(&mut st, A, Some(scan(1)));
    st.banner_mut().dismissed = true;
    load(&mut st, A, Some(scan(1)));
    assert!(
        st.banner().dismissed,
        "같은 문서의 재로드는 표지를 유지한다"
    );
    load(&mut st, C, Some(scan(2)));
    assert_eq!(st.banner(), BannerFlags::default());
}

#[test]
fn strip_fragment_keeps_the_query() {
    assert_eq!(strip_fragment("file:///a.html?x=1#y"), "file:///a.html?x=1");
    assert_eq!(strip_fragment("file:///a.html"), "file:///a.html");
}
