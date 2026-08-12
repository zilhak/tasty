//! `Core` — attach 점유 하의 surface send / adopt. `src/core/mod.rs` 의 `impl Core` 분할.

use super::*;

impl Core {
    /// `DomainIntent::SendToSurface` 본문. ensure_surface_initialized → terminal
    /// lookup → send_bytes / send_key 분기.
    pub(super) fn apply_send_to_surface(
        engine: &mut crate::core::CoreState,
        surface_id: u32,
        payload: crate::core::intent::SendPayload,
    ) -> CoreEvent {
        // §2.4 서버 본인 입력 차단: attach 로 점유된 surface 는 서버 로컬 입력
        // (사용자 GUI 키 / IPC surface.send*) 이 PTY 에 닿지 못한다. client 경유
        // 입력은 단계 4 의 holder-검증 attach 채널로 들어와 이 경로를 우회한다.
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

    /// `DomainIntent::AdoptTerminal` 본문 — headless PTY 를 실제 Surface 로 승격한다
    /// (`pty.attach_surface`, 18-c). `apply_create_tab` 의 tab/pane 트리 등록은 그대로
    /// 하되 **새 Terminal 을 spawn 하지 않는다**: 이미 `TerminalStore` 에 `pty_id` 키로
    /// 들어있는 headless Terminal 을 새 `surface_id` 로 re-key 하고 `pty_registry` 에서
    /// 제거한다. 같은 Terminal 인스턴스(=같은 PTY 자식 프로세스·scrollback)를 옮기는
    /// 것이라 attach 전 상태가 그대로 보존된다.
    ///
    /// borrow/mutation 순서: 검증 → id 발급 → store re-key(waker 재배선 포함) →
    /// pane marker → registry 제거. pane 미존재 등 실패는 store 를 건드리기 전에
    /// bail 해 orphan 을 만들지 않는다.
    pub(super) fn apply_adopt_terminal(
        engine: &mut crate::core::CoreState,
        pane_id: u32,
        pty_id: u32,
    ) -> anyhow::Result<Vec<CoreEvent>> {
        // 1) 검증 (mutation 전). 대상 headless PTY 가 살아있고 pane 이 존재해야 한다.
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

        // 2) 새 id 발급 (apply_create_tab 과 동형).
        let tab_id = engine.next_ids.next_tab();
        let surface_id = engine.next_ids.next_surface();

        // 3) store re-key: headless Terminal 을 pty_id → surface_id 로 옮긴다. 새
        //    surface_id 로 targeted polling 이 이 Terminal 을 그 새 키에서 drain 하도록
        //    waker 를 재배선한다(재배선 없으면 승격된 터미널이 GUI 에서 멈춘 것처럼 보임).
        let Some(terminal) = engine.terminals.remove(pty_id) else {
            anyhow::bail!("headless pty {pty_id} registry/store desync (terminal missing)");
        };
        terminal.rewire_waker(engine.make_waker(surface_id));
        engine.terminals.insert(surface_id, terminal);

        // 4) pane marker: 새 Terminal spawn 없이 트리에만 등록. 에이전트 행동이므로
        //    active_tab 을 바꾸지 않는 background 변형을 쓴다(포커스 독립, 원칙 1·3).
        engine
            .find_pane_by_id_mut(pane_id)
            .expect("pane existence checked above")
            .add_terminal_marker_tab_background(tab_id, surface_id, None);

        // 5) registry 제거: 더 이상 headless 가 아니므로 pty.list 에서 빠지고 이중
        //    등록이 방지된다. 옛 exit-watcher 스레드는 detached 라 자식 reap 을 계속한다.
        engine.pty_registry.remove(pty_id);
        // 옛 pty_id 키의 waker dedup 게이트 제거 — 3)에서 surface_id 로 재배선했으므로
        // 옛 pty_id 게이트는 더 이상 쓰이지 않는다. 미제거 시 승격마다 게이트 누적(누수).
        if let Some(factory) = engine.waker_factory.as_ref() {
            factory.forget_surface(pty_id);
        }
        engine.mark_layout_dirty();

        // attach 점유 중인 workspace 에 새로 생긴 멤버라면 편입 + 즉시 tap
        // (로컬 생성 경로 gap — forward-op 경로와 대칭으로 점유를 상속해야 한다).
        // adopt-terminal 은 항상 터미널을 승격한다(headless PTY).
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
        // 알려진 id 의 detached mirror terminal 을 직접 등록(기본 워크스페이스
        // 터미널과 무관한 deterministic id).
        let sid = 9999;
        engine
            .terminals
            .insert(sid, tasty_terminal::Terminal::new_detached(80, 24));

        // free → 전송 성공.
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));

        // 점유 → 서버 로컬 입력 차단. `hard_occupied: true` 로 명시 구분(Gate4
        // 판단필요: "진짜 없음" 과 같은 메시지로 뭉뚱그리면 안 된다).
        engine.attach.acquire(sid, 1).unwrap();
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(
            ev,
            CoreEvent::SurfaceSent {
                sent: false,
                hard_occupied: true
            }
        ));

        // 해제 → 다시 서버 조작 가능.
        engine.attach.release(sid, 1).unwrap();
        let ev = Core::apply_send_to_surface(&mut engine, sid, SendPayload::Bytes(b"x".to_vec()));
        assert!(matches!(ev, CoreEvent::SurfaceSent { sent: true, .. }));
    }

    /// 존재하지 않는 surface 는 `sent: false` 지만 `hard_occupied: false` —
    /// hard-occupied 와 구분되는 별개 실패 사유임을 확인(위 테스트와 대비쌍).
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
        // ADR-0040: soft 점유는 hard 술어(is_hard_occupied)를 세우지 않으므로 서버 로컬
        // 입력이 계속 도달한다(sent: true). hard 만 차단(위 테스트와 대비).
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
