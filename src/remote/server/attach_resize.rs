//! attach client의 크기 요청을 적용하고 모든 요청에 응답한다.
//! 크기 변경과 같은 크기 확인은 terminal의 attach tap이 출력과 같은 순서로 Resize를 보낸다.
//! 거절은 크기를 바꾸지 않으므로 이 모듈이 바로 ResizeRejected를 보낸다.
//! 협상 규칙: docs/dev-guide/attach-behavior.md#리사이즈-전파-mirror-geometry.

use crate::core::attach::AttachClientId;
use crate::runtime::engine_access::EngineMut;
use tasty_ipc::stream::{StreamControl, StreamFrame, StreamTag};
use tasty_ipc::stream_hub::{PushResult, StreamHub};

/// 한 engine에서 크기 요청을 처리한 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttachResizeOutcome {
    /// grid를 바꿨거나 이미 같은 크기라 tap이 Resize로 응답한다.
    Answered,
    /// 이 engine의 점유에 그 surface가 없다. 다른 engine이 처리할 수 있다.
    NotHere,
    /// 점유자가 아니거나 terminal이 아니라 적용하지 않았다. 호출자가 거절을 회신한다.
    Rejected,
}

impl EngineMut<'_> {
    /// 점유를 확인한 뒤 실제 PTY 크기 변경을 시도한다. OS PTY 적용 성공은 확인하지 않는다.
    pub fn apply_attached_workspace_resize(
        &mut self,
        client_id: AttachClientId,
        remote_surface_id: u32,
        cols: usize,
        rows: usize,
    ) -> AttachResizeOutcome {
        let Some(ws) = self.live.occupancy.workspace_of_surface(remote_surface_id) else {
            return AttachResizeOutcome::NotHere;
        };
        if self.live.occupancy.workspace_holder(ws) != Some(client_id) {
            return AttachResizeOutcome::Rejected;
        }
        if self
            .runtime
            .terminals
            .resize_for_attach(remote_surface_id, cols, rows)
        {
            AttachResizeOutcome::Answered
        } else {
            AttachResizeOutcome::Rejected
        }
    }
}

/// 적용하지 않은 크기 요청을 요청자에게 알린다. 값은 거절한 요청의 것이다.
pub(crate) fn reply_resize_rejected(
    hub: &StreamHub,
    client_id: AttachClientId,
    surface_id: u32,
    cols: usize,
    rows: usize,
) {
    let reply = StreamControl::ResizeRejected {
        surface_id,
        cols,
        rows,
    };
    let payload = match serde_json::to_vec(&reply) {
        Ok(payload) => payload,
        Err(e) => {
            tracing::warn!("attach resize: 거절 회신을 직렬화하지 못했다: {e}");
            return;
        }
    };
    let result = hub.push(client_id, StreamFrame::new(StreamTag::Control, payload));
    if result != PushResult::Sent {
        // client는 응답 마감으로 같은 실패를 판정하므로 여기서 다시 보내지 않는다.
        tracing::warn!(
            "attach resize: client {client_id} 에 surface {surface_id} 거절 회신을 넣지 못했다: {result:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_terminal::AttachEvent;

    const HOLDER: AttachClientId = 7;

    /// 첫 workspace를 HOLDER가 점유하고 그 안의 terminal ID를 돌려준다.
    fn occupy_first_workspace(engine: &mut EngineMut<'_>) -> u32 {
        let ws = engine.workspace_at(0).expect("workspace index is valid");
        let (ws_id, members) = (ws.id, ws.all_surface_ids());
        engine
            .live
            .occupancy
            .acquire_workspace(ws_id, &members, &members, HOLDER)
            .expect("workspace is free in the fixture");
        *members
            .iter()
            .find(|id| engine.runtime.terminals.contains(**id))
            .expect("fixture workspace has a terminal")
    }

    #[test]
    fn every_request_gets_an_outcome_and_a_same_size_request_is_confirmed_in_order() {
        let (_, mut session) = crate::state::tests::test_state();
        let mut engine = session.borrow_mut();
        let sid = occupy_first_workspace(&mut engine);
        let terminal = engine.runtime.terminals.get_mut(sid).expect("terminal");
        let (cols, rows) = (terminal.cols(), terminal.rows());
        let mut tap = terminal.snapshot_and_stream();

        assert_eq!(
            engine.apply_attached_workspace_resize(HOLDER, sid, cols, rows),
            AttachResizeOutcome::Answered
        );
        assert!(matches!(
            tap.events.try_recv(),
            Ok(AttachEvent::Resize { cols: c, rows: r }) if (c, r) == (cols, rows)
        ));

        assert_eq!(
            engine.apply_attached_workspace_resize(HOLDER + 1, sid, cols + 1, rows),
            AttachResizeOutcome::Rejected,
            "점유자가 아니면 적용하지 않고 거절한다"
        );
        assert_eq!(
            engine.apply_attached_workspace_resize(HOLDER, u32::MAX, cols, rows),
            AttachResizeOutcome::NotHere,
            "점유에 없는 surface는 다른 engine에 맡긴다"
        );
        assert!(
            tap.events.try_recv().is_err(),
            "거절과 다른 engine 요청은 tap에 아무것도 남기지 않는다"
        );
    }

    /// 연결마다 응답 보장을 확인할 수 있도록 workspace descriptor가 capability를 싣는다.
    #[test]
    fn workspace_descriptor_announces_the_resize_ack_capability() {
        let (_, mut session) = crate::state::tests::test_state();
        let engine = session.borrow_mut();
        let ws_id = engine.workspace_at(0).expect("workspace index is valid").id;
        let descriptor = engine.build_workspace_descriptor(
            0,
            ws_id,
            &crate::model::AttachSurfaceClass::default(),
        );
        assert_eq!(
            descriptor[tasty_ipc::stream::DESCRIPTOR_CAPABILITIES],
            serde_json::json!([tasty_ipc::stream::RESIZE_ACK_CAPABILITY])
        );
    }
}
