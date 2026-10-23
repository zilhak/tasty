//! Apply an already compiled content filter without changing entry isolation.
use objc2::rc::Retained;
use objc2_web_kit::{WKContentRuleList, WKUserContentController, WKWebView};

/// 기존 content rule을 모두 지우고 차단이 켜졌으며 규칙이 준비된 경우 다시 넣는다.
pub(super) fn apply_block_state(
    webview: &WKWebView,
    block_remote: bool,
    rule_list: Option<&WKContentRuleList>,
) {
    // SAFETY: main thread WKWebView API — configuration()/userContentController() 및
    // add/removeAllContentRuleLists 는 main thread only. 호출 경로(new 의 main-thread
    // completion handler / set_remote_content_allowed)가 main thread 를 보장한다.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        let ucc: Retained<WKUserContentController> =
            webview.configuration().userContentController();
        ucc.removeAllContentRuleLists();
        if block_remote && let Some(list) = rule_list {
            ucc.addContentRuleList(list);
        }
    }
    tracing::debug!(
        block_remote,
        rule_compiled = rule_list.is_some(),
        "webview 원격 차단 상태 적용 (removeAll + 차단 시 add)"
    );
}
