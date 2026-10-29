//! mirror 문서로 오는 host 이벤트(원문 회신·원격 변경 통지) 처리.

use serde_json::Value;

use crate::{
    MIRROR_CHANGED_EVENT, MIRROR_CONTENT_RESULT_EVENT, MarkdownPlugin, MdDoc,
    MirrorContentResultWire, RemoteChange, RemoteRequester,
};

impl MarkdownPlugin {
    /// `markdown_mirror.content_result` — 대기 중인 요청의 회신이면 문서에 반영하고 다시 그린다.
    pub(crate) fn on_mirror_content_result(&mut self, payload: Value) {
        let Ok(reply) = serde_json::from_value::<MirrorContentResultWire>(payload) else {
            tracing::warn!("markdown: malformed {MIRROR_CONTENT_RESULT_EVENT} event");
            return;
        };
        let surface_id = reply.surface_id;
        let request_id = reply.request_id;
        let Some(doc) = self.docs.get_mut(&surface_id) else {
            return;
        };
        let pending = doc.remote.as_ref().and_then(|r| r.pending);
        let applied = doc.apply_remote_result(reply);
        tracing::debug!(
            "markdown surface {surface_id}: remote content result {request_id} (pending {pending:?}) {}",
            if applied { "applied" } else { "ignored" }
        );
        if applied {
            self.reload_webview(surface_id);
        }
    }

    /// `markdown_mirror.changed` — 원문을 보여 주는 문서는 stale 표시만 켜고, 끊김·실패를
    /// 보여 주는 문서는 다시 요청한다([`MdDoc::on_remote_changed`]).
    pub(crate) fn on_mirror_changed(&mut self, payload: &Value) {
        let Some(surface_id) = payload
            .get("surface_id")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
        else {
            tracing::warn!("markdown: malformed {MIRROR_CHANGED_EVENT} event");
            return;
        };
        let change = self
            .docs
            .get_mut(&surface_id)
            .map_or(RemoteChange::Ignore, MdDoc::on_remote_changed);
        match change {
            RemoteChange::Ignore => {}
            RemoteChange::Redraw => self.reload_webview(surface_id),
            RemoteChange::Refetch => {
                self.request_remote_content(surface_id, RemoteRequester::Plugin)
            }
        }
    }
}
