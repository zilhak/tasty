//! attach 점유 중 로컬 입력을 차단하고 headless PTY를 surface로 옮긴다.

use super::*;
use crate::runtime::engine_access::EngineMut;

/// 한 surface에 입력을 쓴 결과. 원격 점유와 대상 부재를 구분한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SurfaceSendOutcome {
    Sent,
    HardOccupied,
    NotFound,
}

impl AppServices {
    /// `SendToSurface`를 실시간 적용 경로로 바로 실행한다. 요청 큐를 거치지 않는다.
    pub(crate) fn send_to_surface(
        &mut self,
        engine: &mut EngineMut<'_>,
        surface_id: u32,
        payload: crate::app::command::SendPayload,
    ) -> SurfaceSendOutcome {
        let intent = crate::app::command::DomainIntent::SendToSurface {
            surface_id,
            payload,
        };
        let events = match self.apply_live(engine, intent) {
            Ok(events) => events,
            Err(_) => return SurfaceSendOutcome::NotFound,
        };
        match events.into_iter().next() {
            Some(CoreEvent::SurfaceSent { sent: true, .. }) => SurfaceSendOutcome::Sent,
            Some(CoreEvent::SurfaceSent {
                hard_occupied: true,
                ..
            }) => SurfaceSendOutcome::HardOccupied,
            _ => SurfaceSendOutcome::NotFound,
        }
    }

    pub(super) fn apply_send_to_surface(
        engine: &mut EngineMut<'_>,
        surface_id: u32,
        payload: crate::app::command::SendPayload,
    ) -> CoreEvent {
        // 점유 client의 입력은 holder 검사를 거친 attach 채널에서 따로 받는다.
        if engine.live.occupancy.is_hard_occupied(surface_id) {
            return CoreEvent::SurfaceSent {
                sent: false,
                hard_occupied: true,
            };
        }
        #[cfg(feature = "gui")]
        if let crate::app::command::SendPayload::Bound { generation, .. } = &payload
            && !engine
                .runtime
                .terminals
                .matches_generation(surface_id, *generation)
        {
            return CoreEvent::SurfaceSent {
                sent: false,
                hard_occupied: false,
            };
        }
        let sent = if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
            match payload {
                crate::app::command::SendPayload::Bytes(bytes) => {
                    terminal.send_bytes(&bytes);
                }
                #[cfg(feature = "gui")]
                crate::app::command::SendPayload::Bound { bytes, .. } => {
                    terminal.send_bytes(&bytes);
                }
                crate::app::command::SendPayload::Text(text) => {
                    terminal.send_key(&text);
                }
            }
            true
        } else {
            false
        };
        CoreEvent::SurfaceSent {
            sent,
            hard_occupied: false,
        }
    }
}

#[cfg(test)]
mod attach_block_tests {
    use super::*;
    use crate::app::command::SendPayload;

    fn test_engine() -> crate::runtime::engine_session::EngineSession {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        crate::runtime::engine_session::EngineSession::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn attached_surface_blocks_server_send() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let sid = 9999;
        engine
            .runtime
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24), None);

        let ev =
            AppServices::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));

        engine.live.occupancy.acquire(sid, 1).unwrap();
        let ev =
            AppServices::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(
            ev,
            CoreEvent::SurfaceSent {
                sent: false,
                hard_occupied: true
            }
        ));

        engine.live.occupancy.release(sid, 1).unwrap();
        let ev =
            AppServices::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));
    }

    #[test]
    fn nonexistent_surface_is_not_found_not_hard_occupied() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let ev = AppServices::apply_send_to_surface(
            &mut engine,
            424242,
            SendPayload::Bytes(b"x".to_vec()),
        );
        assert!(matches!(
            ev,
            CoreEvent::SurfaceSent {
                sent: false,
                hard_occupied: false
            }
        ));
    }

    #[test]
    fn soft_occupied_surface_allows_server_send() {
        let mut engine_session = test_engine();
        let mut engine = engine_session.borrow_mut();
        let sid = 9998;
        engine
            .runtime
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24), None);
        engine.occupy_soft(sid, /*parent*/ 1, None).unwrap();
        let ev =
            AppServices::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));
    }
}
