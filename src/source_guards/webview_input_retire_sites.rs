//! P5: native 입력 대기열의 정리·재판정·binding 검사 자리. 판정 결과는 `host_api::webview::stale_press_tests` 와
//! `popup::route::tests::webview_release` 시험이 보고, 이 검사는 제품이 그 자리를 건너뛰는 우회를 잡는다.

use super::{fn_body, repo_root, strip_comments};

fn body_of(rel: &str, signature: &str) -> String {
    let src = std::fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n");
    let body = fn_body(&src, signature)
        .unwrap_or_else(|| panic!("{rel}에서 `{signature}` 본문을 읽지 못했다"));
    strip_comments(&body)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}

fn pos(body: &str, needle: &str, what: &str) -> usize {
    body.find(needle)
        .unwrap_or_else(|| panic!("{what}: `{needle}` 이 없다"))
}

/// WebView 를 지우는 자리(binding 이 맞지 않은 view, 닫힌 surface 의 view)는 그 surface 의 대기열 항목을 지우고
/// (`retire_view`), 유지한 HTTP 세션이 새 문서로 바뀐 자리는 새 binding 을 적기 전에 옛 문서 항목을 지운다
/// (`retire_page`). 제품에서 `view.webviews` 를 지우는 자리는 이 두 곳뿐이다.
#[test]
fn every_webview_removal_and_page_swap_retires_its_queue_entries() {
    let rel = "src/app/webview_sync.rs";
    let sync = body_of(rel, "pub(crate) fn synchronize(");
    let swap = pos(
        &sync,
        "view.webview_key_bridge.retire_page(sid);",
        "문서 교체",
    );
    let capture = pos(&sync, "NativeWebviewBinding::capture(", "새 binding");
    assert!(swap < capture, "새 binding 을 적은 뒤에 옛 항목을 지운다");
    pos(
        &sync,
        "forsurfaceinstale{view.webviews.remove(&surface);view.webview_key_bridge.retire_view(surface);",
        "맞지 않은 view 지움",
    );
    let webviews = body_of(rel, "fn sync_webviews(");
    pos(
        &webviews,
        "view.webviews.retain(|sid,_|all_html_ids.contains(sid));closed.into_iter().for_each(|sid|view.webview_key_bridge.retire_view(sid));",
        "닫힌 surface 지움",
    );
    let src = std::fs::read_to_string(repo_root().join(rel)).expect("webview_sync.rs");
    let removals =
        src.matches("view.webviews.remove(").count() + src.matches("view.webviews.retain(").count();
    assert_eq!(removals, 2, "WebView 를 지우는 새 자리가 생겼다");
}

/// 미룬 회수의 재판정(`release_retry_due`)은 창마다 대기열 소비 여부와 관계없이 본다. 대기열이 이미 비었다는 조기
/// 반환 뒤에 두면 retire 나 동기화의 선두 소비가 비운 대기열의 의무가 남는다.
#[test]
fn the_held_release_retry_runs_even_when_the_queue_is_already_empty() {
    let drain = body_of(
        "src/app/webview_keys.rs",
        "pub(crate) fn consume_webview_focus_requests(",
    );
    assert!(
        !drain.contains("return;"),
        "소비 함수에 조기 반환이 있다: {drain}"
    );
    let consume = pos(
        &drain,
        "ifmain.webview_key_bridge.has_focus_requests(){consume_native_inputs(self,id);}",
        "소비는 대기열이 있을 때만",
    );
    let retry = pos(
        &drain,
        "key_release::release_retry_due(&mutmain.state,&main.webview_key_bridge,)",
        "재판정",
    );
    assert!(consume < retry, "재판정이 소비보다 앞이다");
}

/// native 누름 알림의 두 소비 입구(동기화의 선두 소비, 창 소비 루프의 누름)는 전이와 게시(`press_and_publish`) 전에
/// 그 surface 의 binding 을 본다(`webview_binding_current`). 동기화는 그 판정을 `bound` 로 넘긴다.
#[test]
fn both_press_entrances_check_the_binding_before_the_press_and_its_publish() {
    let keys = "src/app/webview_keys.rs";
    let leading = body_of(keys, "pub(crate) fn consume_native_focus_requests(");
    pos(
        &leading,
        "ifbound(surface_id){changed|=press_and_publish(",
        "선두 소비",
    );
    let src = std::fs::read_to_string(repo_root().join(keys)).expect("webview_keys.rs");
    let app = src
        .find("impl NativeInputConsumer for App {")
        .expect("App 소비자");
    // 같은 파일의 시험 대역도 `native_press` 를 정의하므로 App 소비자 impl 만 잘라 읽는다.
    let end = app + src[app..].find("\n}\n").expect("App 소비자 끝");
    let native: String =
        strip_comments(&fn_body(&src[app..end], "fn native_press(").expect("App::native_press"))
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
    let bound = pos(
        &native,
        "if!crate::app::webview_sync::webview_binding_current(",
        "창 소비 루프의 누름",
    );
    let press = pos(&native, "press_and_publish(", "전이와 게시");
    assert!(bound < press, "binding 검사가 전이 뒤다");
    let sync = body_of("src/app/webview_sync.rs", "fn sync_webviews(");
    let closure = pos(
        &sync,
        "letbound=|sid|webview_binding_current(&view.webviews,runtime,engine,sid);",
        "동기화의 binding 판정",
    );
    let release = pos(
        &sync,
        "key_release::release_after_native_focus_notices(&mutview.state,engine.core,&view.webview_key_bridge,&bound,",
        "동기화의 선두 소비",
    );
    assert!(closure < release);
}
