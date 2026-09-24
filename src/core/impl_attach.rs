//! attach 점유 중 로컬 입력을 차단하고 headless PTY를 surface로 옮긴다.

use super::*;

impl Core {
    pub(super) fn apply_send_to_surface(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        payload: crate::core::intent::SendPayload,
    ) -> CoreEvent {
        // 점유 client의 입력은 holder 검사를 거친 attach 채널에서 따로 받는다.
        if engine.attach.is_hard_occupied(surface_id) {
            return CoreEvent::SurfaceSent {
                sent: false,
                hard_occupied: true,
            };
        }
        engine.ensure_surface_initialized(surface_id);
        let sent = if let Some(terminal) = engine.find_terminal_by_id_mut(surface_id) {
            match payload {
                crate::core::intent::SendPayload::Bytes(bytes) => {
                    terminal.send_bytes(&bytes);
                }
                crate::core::intent::SendPayload::Text(text) => {
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

    /// 기존 headless Terminal을 새 surface ID로 옮긴다. PTY와 scrollback을 새로 만들지 않는다.
    /// registry 상태와 pane을 먼저 검사한다. store 항목이 없으면 발급한 ID는 사용하지 못하고 실패한다.
    pub(super) fn apply_adopt_terminal(
        engine: &mut crate::core::CoreState,
        pane_id: u32,
        pty_id: u32,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        match engine.pty_registry.get(pty_id) {
            None => anyhow::bail!("headless pty {pty_id} not found"),
            Some(entry) if entry.has_exited() => {
                anyhow::bail!("headless pty {pty_id} already exited")
            }
            Some(_) => {}
        }
        if engine.find_pane_by_id(pane_id).is_none() {
            anyhow::bail!("pane {pane_id} not found");
        }

        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();

        // 수신 알림이 새 ID의 터미널을 찾도록 waker도 바꾼다.
        let Some(terminal) = engine.terminals.remove(pty_id) else {
            anyhow::bail!("headless pty {pty_id} registry/store desync (terminal missing)");
        };
        terminal.rewire_waker(engine.make_waker(surface_id));
        engine.terminals.insert(surface_id, terminal);

        // 에이전트의 생성이 사용자 선택을 바꾸지 않도록 배경 탭으로 넣는다.
        engine
            .find_pane_by_id_mut(pane_id)
            .expect("pane existence checked above")
            .add_terminal_marker_tab_background(tab_id, surface_id, None);

        // registry에서 빼도 기존 exit watcher는 자식 회수를 계속한다.
        engine.pty_registry.remove(pty_id);
        // 옛 ID의 알림 중복 방지 항목이 승격 때마다 남지 않도록 지운다.
        if let Some(factory) = engine.waker_factory.as_ref() {
            factory.forget_surface(pty_id);
        }
        engine.mark_layout_dirty();

        if let Some(ws_idx) = engine.find_workspace_index_for_pane(pane_id) {
            let ws_id = engine.workspaces[ws_idx].id;
            engine.tap_new_workspace_member(ws_id, surface_id, true);
        }

        let (tab_count, active_tab) = engine
            .find_pane_by_id(pane_id)
            .map(|p| (p.tabs.len(), p.active_tab))
            .unwrap_or((0, 0));

        Ok(vec![CoreEvent::TabCreated {
            pane_id,
            tab_id,
            surface_id,
            tab_count,
            active_tab,
        }])
    }
}

#[cfg(test)]
mod attach_block_tests {
    use super::*;
    use crate::core::intent::SendPayload;

    fn test_engine() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    #[test]
    fn attached_surface_blocks_server_send() {
        let mut engine = test_engine();
        let sid = 9999;
        engine
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24));

        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));

        engine.attach.acquire(sid, 1).unwrap();
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(
            ev,
            CoreEvent::SurfaceSent {
                sent: false,
                hard_occupied: true
            }
        ));

        engine.attach.release(sid, 1).unwrap();
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));
    }

    #[test]
    fn nonexistent_surface_is_not_found_not_hard_occupied() {
        let mut engine = test_engine();
        let ev =
            Core::apply_send_to_surface(&mut engine, 424242, SendPayload::Bytes(b"x".to_vec()));
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
        let mut engine = test_engine();
        let sid = 9998;
        engine
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24));
        engine.occupy_soft(sid, /*parent*/ 1, None).unwrap();
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));
    }
}
