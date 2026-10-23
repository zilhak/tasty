//! Install native navigation and response decisions on the owning WebKit thread.
use super::*;

pub(super) fn install(
    webview: &WebView,
    entry_source: &Option<EntryResponseSource>,
    block_remote: &Rc<Cell<bool>>,
    pending_navigations: &Rc<RefCell<Vec<PendingNavigation>>>,
    script_gate: &Rc<RefCell<Option<ScriptGate>>>,
    surface_id: u32,
) {
    {
        let entry = entry_source.clone();
        let block = block_remote.clone();
        let pending_nav = pending_navigations.clone();
        let gate = script_gate.clone();
        webview.connect_decide_policy(move |wv, decision, decision_type| {
            let uri = match decision_type {
                PolicyDecisionType::Response => decision
                    .downcast_ref::<ResponsePolicyDecision>()
                    .and_then(|d| d.request())
                    .and_then(|r| r.uri()),
                PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction => {
                    decision
                        .downcast_ref::<NavigationPolicyDecision>()
                        .and_then(|d| d.navigation_action())
                        .and_then(|a| a.request())
                        .and_then(|r| r.uri())
                }
                _ => None,
            };
            if let Some(source) = &entry
                && (matches!(decision_type, PolicyDecisionType::NewWindowAction)
                    || uri
                        .as_ref()
                        .is_none_or(|uri| source.response(uri.as_str()).is_none()))
            {
                decision.ignore();
                return true;
            }
            // navigation action에는 frame 정보가 없어 main frame 응답에서만 문서 JS를 정한다(docs/surfaces/html/index.md).
            if matches!(decision_type, PolicyDecisionType::Response)
                && let Some(gate) = gate.borrow().as_ref()
                && decision
                    .downcast_ref::<ResponsePolicyDecision>()
                    .is_some_and(|d| d.is_main_frame_main_resource())
                && let Some(uri) = &uri
            {
                match gate.main_response(uri.as_str()) {
                    Response::Decided(js) => set_js(wv, js),
                    Response::Deferred(scan) => {
                        // 스캔이 JS 값을 정하는 응답(허용한 문서의 재로드)은 결정을 보류하고
                        // 작업 스레드에서 읽는다. GTK 루프는 그동안 다른 이벤트를 처리한다.
                        defer_response(wv.clone(), decision.clone(), scan, surface_id);
                        return true;
                    }
                }
            }
            if let Some(uri) = &uri
                && !matches!(decision_type, PolicyDecisionType::Response)
            {
                tracing::debug!(
                    "WebView surface {surface_id}: navigation {decision_type:?} {}",
                    uri.as_str()
                );
            }
            // 탐색 시도는 차단 여부와 별개로 기록한다. 응답 정책 이벤트는 제외한다.
            if matches!(
                decision_type,
                PolicyDecisionType::NavigationAction | PolicyDecisionType::NewWindowAction
            ) && let Some(uri) = &uri
            {
                let user_gesture = decision
                    .downcast_ref::<NavigationPolicyDecision>()
                    .and_then(|d| d.navigation_action())
                    .is_some_and(|a| a.is_user_gesture());
                pending_nav.borrow_mut().push(PendingNavigation {
                    url: uri.as_str().to_string(),
                    user_gesture,
                });
            }
            if !block.get() {
                return false;
            }
            if let Some(uri) = &uri {
                let s = uri.as_str();
                if s.starts_with("http://") || s.starts_with("https://") {
                    decision.ignore();
                    return true;
                }
            }
            false
        });
    }
}
