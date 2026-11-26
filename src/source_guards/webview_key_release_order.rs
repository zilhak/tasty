//! T-W2-6b: WebView 동기화(`sync_webviews`)에서 W2 회수는 W1 모드 결정과 표시 술어 관측 뒤, 표시 적용 전에 한 번
//! 있다. 회수가 성공하면 backend 의 키 관측이 거짓이 되므로, 관측을 회수 뒤로 옮기면 passive 팝업·토스트와 겹친
//! 페이지가 같은 프레임에 숨는다. 판정 결과는 `popup::route::tests::webview_release` 시험이 보고, 이 검사는 호출
//! 위치의 순서만 본다. 계약: `docs/design/systems/webview.md` 의 "포커스" 절.
//! 호스트 루프의 GTK 펌프 세 자리가 이벤트마다 native 포커스 알림을 소비하는 펌프 하나를 부르는지도 본다(T-FWD-8d).

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
        "key_release::release_after_native_focus_notices(",
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

/// T-FWD-8d: 호스트 루프에서 GTK 이벤트를 처리하는 두 자리(WebView 펌프 `pump_webview_key_events`, 트레이 메뉴 펌프
/// `poll_tray_menu_events`)는 이벤트마다 native 입력을 소비하는 `App::pump_gtk_events` 를 부르고, 소비하지 않는
/// `system_tray::pump_gtk_events` 를 직접 부르지 않는다. halt 펌프(`halted_webview_paint_deadline`)는 키·포커스를
/// 전달하지 않으므로 반대로 소비하지 않는 펌프만 부른다. 그 펌프의 순서는 `host_api::webview::keys` 의 T-FWD-8c
/// 시험이 본다.
#[test]
fn every_host_gtk_pump_consumes_the_focus_notices_per_event() {
    let read = |rel: &str| {
        std::fs::read_to_string(repo_root().join(rel))
            .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
            .replace("\r\n", "\n")
    };
    let keys = read("src/app/webview_keys.rs");
    let pump = strip_comments(
        &fn_body(&keys, "pub(crate) fn pump_gtk_events(&mut self)").expect("App::pump_gtk_events"),
    );
    assert!(
        pump.contains(
            "crate::system_tray::pump_gtk_events_each(|| self.consume_webview_focus_requests())"
        ),
        "App GTK 펌프가 이벤트마다 알림을 소비하지 않는다"
    );
    for (rel, func) in [
        (
            "src/app/webview_keys.rs",
            "pub(crate) fn pump_webview_key_events(",
        ),
        ("src/app/event_handler.rs", "fn poll_tray_menu_events("),
    ] {
        let src = read(rel);
        let body = strip_comments(
            &fn_body(&src, func).unwrap_or_else(|| panic!("{rel}에서 `{func}` 본문을 읽지 못했다")),
        );
        assert_eq!(
            body.matches("self.pump_gtk_events();").count(),
            1,
            "{rel} `{func}` 가 App GTK 펌프를 부르지 않는다"
        );
        assert!(
            !body.contains("system_tray::pump_gtk_events()"),
            "{rel} `{func}` 가 알림을 소비하지 않는 펌프를 직접 부른다"
        );
    }
    let halt = strip_comments(
        &fn_body(
            &read("src/app/halt_frame.rs"),
            "pub(crate) fn halted_webview_paint_deadline(",
        )
        .expect("halted_webview_paint_deadline"),
    );
    assert!(
        halt.contains("crate::system_tray::pump_gtk_events();")
            && !halt.contains("self.pump_gtk_events()"),
        "halt 펌프가 native 입력을 소비한다"
    );
}
