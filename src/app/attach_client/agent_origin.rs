//! 에이전트 요청의 회신은 사용자 toast 대신 로그로 알린다.
//! 회신에 요청 주체가 없으므로 송신 시 op_id·request_id를 기록한다.

use tasty_remote::client_session::AttachClientSession;

/// 복원할 항목이 없는 응답은 일반 실패와 구별해 알린다.
/// 에이전트 요청의 실패는 사용자 toast로 표시하지 않는다.
pub(super) fn apply_structural_failed(
    sess: &mut AttachClientSession,
    toast: impl FnOnce(String, crate::adapters::ui::ToastKind),
    op_id: u64,
    reason: Option<String>,
) {
    let reason = reason.unwrap_or_default();
    if sess.state.agent_requests.structural.remove(&op_id) {
        tracing::warn!(
            "structural forward op {op_id} (agent origin) failed on the remote: {reason}"
        );
        return;
    }
    if reason == tasty_ipc::stream::STRUCTURAL_REASON_RESTORE_EMPTY {
        toast(
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
    toast(msg, crate::adapters::ui::ToastKind::Warning);
}

/// 잘림 안내를 문서 본문에 넣지 않는다. 사용자 요청은 toast, 에이전트 요청은 로그로 알린다.
pub(super) fn notify_markdown_truncated(
    toast: impl FnOnce(String, crate::adapters::ui::ToastKind),
    local: u32,
    agent_origin: bool,
) {
    if agent_origin {
        tracing::info!(
            "markdown mirror surface {local}: agent-requested content arrived truncated (no toast)"
        );
        return;
    }
    toast(
        crate::i18n::t("attach.toast.mirror_markdown_truncated").to_string(),
        crate::adapters::ui::ToastKind::Warning,
    );
}
