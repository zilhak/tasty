//! 에이전트 요청의 회신은 사용자 toast 대신 로그로 알린다.
//! 회신에 요청 주체가 없으므로 송신 시 op_id·request_id를 기록한다.

use std::collections::HashSet;

use super::{AttachClientSession, MirrorHost};

/// 회신을 기다리는 에이전트 요청 ID. 회신이나 재연결 때 제거한다.
#[derive(Debug, Default)]
pub(super) struct AgentRequests {
    /// forward 한 구조 op 의 op_id(`PendingStructuralForward::silent_failure`).
    structural: HashSet<u64>,
    /// 원격 markdown 원문 요청의 request_id(`PendingMarkdownContentForward::agent_origin`).
    markdown: HashSet<u64>,
}

impl AgentRequests {
    pub(super) fn note_structural_from(
        &mut self,
        pending: &crate::core::PendingStructuralForward,
        op_id: u64,
    ) {
        if pending.silent_failure {
            self.structural.insert(op_id);
        }
    }

    pub(super) fn forget_structural(&mut self, op_id: u64) {
        self.structural.remove(&op_id);
    }

    pub(super) fn note_markdown_from(
        &mut self,
        req: &crate::core::PendingMarkdownContentForward,
        request_id: u64,
    ) {
        if req.agent_origin {
            self.markdown.insert(request_id);
        }
    }

    pub(super) fn take_markdown(&mut self, request_id: u64) -> bool {
        self.markdown.remove(&request_id)
    }

    pub(super) fn clear(&mut self) {
        self.structural.clear();
        self.markdown.clear();
    }
}

/// 복원할 항목이 없는 응답은 일반 실패와 구별해 알린다.
/// 에이전트 요청의 실패는 사용자 toast로 표시하지 않는다.
pub(super) fn apply_structural_failed(
    sess: &mut AttachClientSession,
    host: &mut MirrorHost<'_>,
    op_id: u64,
    reason: Option<String>,
) {
    let reason = reason.unwrap_or_default();
    if sess.agent_requests.structural.remove(&op_id) {
        tracing::warn!(
            "structural forward op {op_id} (agent origin) failed on the remote: {reason}"
        );
        return;
    }
    if reason == tasty_ipc::stream::STRUCTURAL_REASON_RESTORE_EMPTY {
        host.toast(
            crate::i18n::t("attach.toast.mirror_restore_empty").to_string(),
            crate::adapters::ui::ToastKind::Info,
        );
        return;
    }
    let base = crate::i18n::t("attach.toast.mirror_structural_forward_failed");
    let msg: String = if reason.is_empty() {
        base.to_string()
    } else {
        format!("{base} ({reason})")
    };
    host.toast(msg, crate::adapters::ui::ToastKind::Warning);
}

/// 잘림 안내를 문서 본문에 넣지 않는다. 사용자 요청은 toast, 에이전트 요청은 로그로 알린다.
pub(super) fn notify_markdown_truncated(host: &mut MirrorHost<'_>, local: u32, agent_origin: bool) {
    if agent_origin {
        tracing::info!(
            "markdown mirror surface {local}: agent-requested content arrived truncated (no toast)"
        );
        return;
    }
    host.toast(
        crate::i18n::t("attach.toast.mirror_markdown_truncated").to_string(),
        crate::adapters::ui::ToastKind::Warning,
    );
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::tests::test_session;
    use super::super::{MirrorEvent, apply_mirror_events};
    use super::*;

    fn structural(silent_failure: bool) -> crate::core::PendingStructuralForward {
        crate::core::PendingStructuralForward {
            op: tasty_ipc::stream::StructuralOp::NewTab {
                anchor_surface_id: 1,
                surface_kind: "terminal".to_string(),
                params: serde_json::Value::Null,
            },
            user_triggered: false,
            close_focus_candidates: Vec::new(),
            silent_failure,
        }
    }

    fn markdown(request_id: u64, agent_origin: bool) -> crate::core::PendingMarkdownContentForward {
        crate::core::PendingMarkdownContentForward {
            local_surface_id: 300,
            request_id,
            agent_origin,
        }
    }

    #[test]
    fn an_agent_forward_failure_does_not_toast() {
        let mut sess = test_session(9_000, HashMap::new());
        sess.agent_requests
            .note_structural_from(&structural(true), 5);
        sess.agent_requests
            .note_structural_from(&structural(false), 6);
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
        let (mut state, mut engine) = crate::state::tests::test_state();
        {
            let mut host = MirrorHost::windowed(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![MirrorEvent::StructuralFailed(5, Some("nope".to_string()))],
            );
        }
        assert_eq!(state.toasts.len(), 0, "에이전트 op 의 실패는 로그로 끝난다");
        assert!(
            sess.agent_requests.structural.is_empty(),
            "회신이 오면 표시를 지운다"
        );

        {
            let mut host = MirrorHost::windowed(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![MirrorEvent::StructuralFailed(6, Some("nope".to_string()))],
            );
        }
        assert_eq!(
            state.toasts.len(),
            1,
            "표시 없는 op 의 실패는 toast 를 낸다"
        );
    }

    #[test]
    fn an_agent_markdown_reload_truncation_does_not_toast() {
        let mut sess = test_session(9_000, HashMap::from([(30, 300)]));
        sess.markdown_locals.insert(300);
        sess.agent_requests
            .note_markdown_from(&markdown(11, true), 11);
        sess.agent_requests
            .note_markdown_from(&markdown(12, false), 12);
        let mut plugin_manager: Option<crate::plugin::PluginManager> = None;
        let (mut state, mut engine) = crate::state::tests::test_state();
        let truncated = |request_id| MirrorEvent::MarkdownContentResult {
            request_id,
            surface_id: 30,
            ok: true,
            file: Some("/r/a.md".to_string()),
            source: Some("# a".to_string()),
            truncated: true,
            reason: None,
        };
        {
            let mut host = MirrorHost::windowed(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![truncated(11)],
            );
        }
        assert_eq!(
            state.toasts.len(),
            0,
            "에이전트 요청의 잘림은 toast 를 안 낸다"
        );
        assert!(
            sess.agent_requests.markdown.is_empty(),
            "회신이 오면 표시를 지운다"
        );

        {
            let mut host = MirrorHost::windowed(&mut state, &mut engine);
            apply_mirror_events(
                &mut sess,
                &mut host,
                &mut plugin_manager,
                vec![truncated(12)],
            );
        }
        assert_eq!(
            state.toasts.len(),
            1,
            "plugin 자신의 요청은 종전대로 toast 를 낸다"
        );
    }
}
