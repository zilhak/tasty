//! T-W2-6b: WebView 동기화(`sync_webviews`)에서 W2 회수는 W1 모드 결정과 표시 술어 관측 뒤, 표시 적용 전에 한 번
//! 있다. 회수가 성공하면 backend 의 키 관측이 거짓이 되므로, 관측을 회수 뒤로 옮기면 passive 팝업·토스트와 겹친
//! 페이지가 같은 프레임에 숨는다. 판정 결과는 `popup::route::tests::webview_release` 시험이 보고, 이 검사는 호출
//! 위치의 순서만 본다. 계약: `docs/design/systems/webview.md` 의 "포커스" 절.

use super::{fn_body, repo_root, strip_comments};

#[test]
fn the_webview_key_release_sits_between_the_display_observation_and_the_reveal() {
    let rel = "src/app/webview_sync.rs";
    let src = std::fs::read_to_string(repo_root().join(rel))
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n");
    let body = strip_comments(
        &fn_body(&src, "fn sync_webviews(")
            .unwrap_or_else(|| panic!("{rel}에서 `fn sync_webviews(` 본문을 읽지 못했다")),
    );
    let at = |needle: &str| {
        assert_eq!(
            body.matches(needle).count(),
            1,
            "`{needle}` 호출은 한 번이다"
        );
        body.find(needle).unwrap()
    };
    let order = [
        "settle_overlay(",
        "hidden_under_toast(",
        "key_release::release_webview_key_focus(",
        "apply_reveal(",
    ];
    for pair in order.windows(2) {
        assert!(
            at(pair[0]) < at(pair[1]),
            "`{}` 가 `{}` 보다 앞이어야 한다",
            pair[0],
            pair[1]
        );
    }
}
