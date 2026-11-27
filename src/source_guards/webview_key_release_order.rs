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

/// N1a: native 콜백이 읽는 키 쥘 곳은 게시 함수 하나(`key_release::publish_key_holder`)로만 다리에 넣고, 그 함수는
/// 콜백이 다음에 읽기 전에 키 쥘 곳을 바꿀 수 있는 자리마다 불린다: 누름 전이 뒤(`press_and_publish`), 전달 키 처리
/// 뒤(`dispatch_and_apply` 의 `publish_key_holder`), 프레임 동기화(회수 판정 뒤), 창 이벤트 처리 뒤(그리기 제외),
/// 루프 끝 intent·생명주기 적용 뒤·native 입력 펌프 전, 그리고 GTK 대기 이벤트를 건마다 소비하지 않고 비우는 펌프
/// (native 메뉴 poll, 맞지 않은 view 의 Drop, Linux 만) 바로 앞(`key_release::publish_before_gtk_pump`). 순서의 효과는 `app::webview_keys` 상태 시험
/// (`a_consumed_shortcut_publishes_before_the_next_edit_key`)이 잰다.
#[test]
fn webview_key_holder_publish_sites() {
    let read = |rel: &str| {
        std::fs::read_to_string(repo_root().join(rel))
            .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
            .replace("\r\n", "\n")
    };
    let squeeze = |s: &str| strip_comments(s).split_whitespace().collect::<String>();
    // 게시 함수 밖에서 다리의 키 쥘 곳을 직접 넣는 제품 호출은 없다(시험 모듈 제외).
    let mut direct = Vec::new();
    for (rel, src) in super::rust_sources() {
        let rel = rel.to_string_lossy().into_owned();
        if rel.ends_with("tests.rs")
            || rel.contains("/tests/")
            || rel.starts_with("src/source_guards/")
        {
            continue;
        }
        let product = src.split("\nmod tests {").next().unwrap_or_default();
        let calls = strip_comments(product).matches("set_key_holder(").count()
            - strip_comments(product)
                .matches("fn set_key_holder(")
                .count();
        if calls > 0 {
            direct.push((rel, calls));
        }
    }
    assert_eq!(
        direct,
        [("src/app/webview_sync/key_release.rs".to_string(), 1)],
        "키 쥘 곳을 게시 함수 밖에서 넣는다"
    );
    let keys = read("src/app/webview_keys.rs");
    let press =
        squeeze(&fn_body(&keys, "pub(crate) fn press_and_publish(").expect("press_and_publish"));
    assert!(
        press.ends_with(
            "letchanged=apply_native_press(state,core,surface_id,kind);crate::app::webview_sync::key_release::publish_key_holder(state,core,bridge,bound);changed}"
        ),
        "누름 전이 뒤 게시가 아니다: {press}"
    );
    let apply = squeeze(&fn_body(&keys, "fn dispatch_and_apply(").expect("dispatch_and_apply"));
    assert!(
        apply.contains("consumer.apply_focus_opens(id);consumer.publish_key_holder(id);"),
        "전달 키 처리 뒤 게시가 아니다: {apply}"
    );
    let sync = squeeze(
        &fn_body(&read("src/app/webview_sync.rs"), "fn sync_webviews(").expect("sync_webviews"),
    );
    let release = sync
        .find("key_release::release_after_native_focus_notices(")
        .expect("회수 판정");
    let publish = sync
        .find("key_release::publish_key_holder(")
        .expect("동기화 게시");
    assert!(release < publish, "동기화 게시가 회수 판정 앞이다");
    let handler = read("src/app/event_handler.rs");
    let window = squeeze(&fn_body(&handler, "fn window_event(").expect("window_event"));
    assert!(
        window.ends_with(
            "letredraw=matches!(event,WindowEvent::RedrawRequested);self.route_window_input(id,event);if!redraw{self.publish_webview_key_holders(Some(id));}}"
        ),
        "창 이벤트 처리 뒤 게시가 아니다"
    );
    let wait = squeeze(&fn_body(&handler, "fn about_to_wait(").expect("about_to_wait"));
    let at = |needle: &str| {
        assert_eq!(wait.matches(needle).count(), 1, "`{needle}` 는 한 번이다");
        wait.find(needle).unwrap()
    };
    assert!(at("self.dispatch_pending_intents();") < at("self.publish_webview_key_holders(None);"));
    assert!(
        at("self.process_pending_open_preset_window(event_loop);")
            < at("self.publish_webview_key_holders(None);")
    );
    assert!(at("self.publish_webview_key_holders(None);") < at("self.poll_tray_menu_events();"));
    assert!(at("self.publish_webview_key_holders(None);") < at("self.pump_webview_key_events();"));
    let pump = squeeze(
        &fn_body(
            &read("src/app/webview_sync/key_release.rs"),
            "pub(crate) fn publish_before_gtk_pump<",
        )
        .expect("publish_before_gtk_pump"),
    );
    assert!(
        pump.ends_with("#[cfg(any(target_os=\"linux\",test))]publish_key_holder(state,core,bridge,&|sid|bound.contains(&sid));#[cfg(not(any(target_os=\"linux\",test)))]let_=(state,core,bridge,bound);pump()}"),
        "펌프 앞 게시가 아니다: {pump}"
    );
    let menu = squeeze(
        &fn_body(
            &read("src/view/main/redraw.rs"),
            "pub(crate) fn poll_pending_native_menu(",
        )
        .expect("poll_pending_native_menu"),
    );
    assert_eq!(menu.matches(".poll()").count(), 1, "메뉴 poll 은 한 번이다");
    assert!(
        menu.contains("let(state,bridge)=(&mutself.state,&self.webview_key_bridge);letpolled=publish_before_gtk_pump(state,engine.core,bridge,bound,||handle.poll());"),
        "메뉴 poll 이 게시 뒤가 아니다: {menu}"
    );
    let stale = squeeze(
        &fn_body(
            &read("src/app/webview_sync/key_release.rs"),
            "pub(crate) fn drop_stale_views<",
        )
        .expect("drop_stale_views"),
    );
    assert!(
        stale.contains("publish_before_gtk_pump(state,core,bridge,bound,||drop(view));"),
        "맞지 않은 view 의 Drop 이 게시 뒤가 아니다: {stale}"
    );
}
