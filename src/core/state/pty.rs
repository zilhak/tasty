use std::time::Instant;

use crate::runtime::engine_access::EngineMut;
use tasty_terminal::{Terminal, TerminalEvent};

use super::CoreState;

impl crate::runtime::engine_access::EngineRef<'_> {
    pub fn is_surface_deferred(&self, surface: u32) -> bool {
        self.runtime.surfaces.get(&surface).is_some_and(|surface| {
            surface
                .as_any()
                .is::<crate::runtime::surface_restorer::JournalPlaceholder>()
        })
    }
    pub(crate) fn find_mesh_surface_info(&self, surface: u32) -> Option<(String, String)> {
        self.runtime
            .surfaces
            .get(&surface)?
            .attach_mesh_info()
            .map(|(kind, plugin)| (kind.into(), plugin.into()))
    }
    pub(crate) fn find_egui_mesh_surface(
        &self,
        surface: u32,
    ) -> Option<&crate::runtime::egui_mesh_surface::EguiMeshSurface> {
        self.runtime.surfaces.get(&surface)?.as_any().downcast_ref()
    }
}

impl EngineMut<'_> {
    /// 유휴 TTL이 지난 등록을 지우고 Terminal과 waker 기록도 함께 정리한다.
    /// 실제 자식 종료·회수는 같은 항목에 있던 Pty owner의 플랫폼별 Drop이 수행한다.
    pub(crate) fn sweep_idle_ptys(&mut self, now: Instant) -> Vec<u32> {
        let expired = self.runtime.terminals.expired_standalone_ids(now);
        for pty_id in &expired {
            let pty_id = *pty_id;
            self.runtime.terminals.remove(pty_id);
            if let Some(factory) = self.runtime.waker_factory.as_ref() {
                factory.forget_surface(pty_id);
            }
            tracing::debug!("headless pty {pty_id} swept (idle TTL exceeded)");
        }
        expired
    }

    pub fn send_fast_init(&mut self, surface_id: u32) {
        if let Err(e) = crate::surface_meta::SurfaceMetaStore::ensure_created(surface_id) {
            tracing::warn!("surface_meta ensure_created failed for surface {surface_id}: {e}");
        }
        let scrollback_limit = self.settings.general.scrollback_lines;
        let disk_swap = self.settings.performance.scrollback_disk_swap;
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.set_scrollback_limit(scrollback_limit);
            if disk_swap {
                terminal.enable_disk_scrollback(surface_id);
            }
        }
        // bash 초기화 파일은 rcfile 인자로 전달한다. 여기서는 사용자 startup_command만 입력한다.
        let startup = self.settings.general.startup_command.trim();
        if !startup.is_empty() {
            let line = format!("{startup}\n");
            if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
                terminal.send_key(&line);
            }
        }
    }

    /// 대기 중인 scrollback을 꺼내 Terminal에 적용한다. Terminal이 없으면 꺼낸 내용은 버린다.
    /// 이미 시작한 PTY의 첫 출력보다 먼저 적용된다고 보장하지는 않는다.
    pub fn apply_pending_scrollback_inject(&mut self, surface_id: u32) {
        let Some(lines) = self.runtime.pending_scrollback_inject.remove(&surface_id) else {
            return;
        };
        if lines.is_empty() {
            return;
        }
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.inject_scrollback(lines);
            let prefill = terminal.rows() / 2;
            terminal.prefill_visible_from_scrollback(prefill);
        }
    }

    /// 트리는 유지하고 store의 Terminal을 교체한 뒤 기존 Terminal을 drop한다.
    /// 기존 ID가 없으면 새 Terminal을 등록한 상태에서 Err를 반환한다.
    pub fn replace_terminal_by_id(
        &mut self,
        surface_id: u32,
        new_terminal: (Terminal, tasty_terminal::Pty),
    ) -> anyhow::Result<()> {
        if let Some(old) =
            self.runtime
                .terminals
                .replace(surface_id, new_terminal.0, Some(new_terminal.1))
        {
            drop(old);
            return Ok(());
        }
        anyhow::bail!("Surface {} not found", surface_id)
    }

    /// OutputAppended 발생 여부를 현재 observer 목록에 맞춘다. 생성 직후만 설정하면 등록 변경을 놓친다.
    pub(crate) fn sync_output_event_gates(&mut self) {
        let router = &self.observer_router;
        let hooks = &self.hooks;
        for (sid, t) in self.runtime.terminals.iter_mut() {
            t.set_output_events_enabled(router.wants(sid) || hooks.has_output_match_hook(sid));
        }
    }

    pub fn process_all(&mut self) -> bool {
        self.sync_output_event_gates();
        self.runtime.terminals.process_all()
    }

    /// Windows 절전 복귀 후 살아 있는 자식에 wake를 시도하고 비셸 전경 surface를 알림 후보로 반환한다.
    /// 실제 응답 회복을 확인한 결과는 아니다.
    #[cfg(all(windows, feature = "gui"))]
    pub(crate) fn wake_terminals_after_resume(&mut self) -> Vec<u32> {
        let mut suspects = Vec::new();
        let ids: Vec<_> = self.runtime.terminals.iter().map(|(sid, _)| sid).collect();
        for sid in ids {
            let (cols, rows) = self
                .runtime
                .terminals
                .get(sid)
                .expect("collected ID")
                .dimensions();
            let Some(pty) = self.runtime.terminals.pty_mut(sid) else {
                continue;
            };
            if !pty.check_alive() {
                continue; // 죽음 — process_all 의 ProcessExited cascade 가 정리.
            }
            if let Err(error) = pty.apply_os_resize(cols, rows) {
                tracing::warn!("wake_nudge PTY resize failed: {error}");
            }
            let shell_pid = pty.process_id();
            if let Some(info) = pty.foreground_process_info()
                && Some(info.pid) != shell_pid
                && !tasty_terminal::foreground_process::is_known_shell_name(&info.name)
            {
                suspects.push(sid);
            }
        }
        suspects
    }

    pub fn process_surface(&mut self, surface_id: u32) -> bool {
        let enabled =
            self.observer_router.wants(surface_id) || self.hooks.has_output_match_hook(surface_id);
        if let Some(t) = self.runtime.terminals.get_mut(surface_id) {
            t.set_output_events_enabled(enabled);
        }
        self.runtime.terminals.process_surface(surface_id)
    }

    #[cfg(feature = "gui")]
    pub fn flush_all_pty_resizes(&mut self) -> bool {
        self.runtime.terminals.flush_pty_resizes()
    }

    /// 락을 즉시 얻지 못한 Terminal은 이번 수집에서 건너뛴다.
    /// 그 이벤트는 큐에 남지만 여기서 다음 호출 시각이나 전달 성공까지 보장하지는 않는다.
    pub fn collect_events(&mut self) -> Vec<TerminalEvent> {
        let mut all_events = Vec::new();
        for (sid, terminal) in self.runtime.terminals.iter_mut() {
            let Some(mut events) = terminal.try_take_events() else {
                continue;
            };
            for event in &mut events {
                event.surface_id = sid;
            }
            all_events.extend(events);
        }
        all_events
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    // 이유: 현재 호출자가 없으며 터미널 ID 조회 API를 유지한다.
    #[allow(dead_code)]
    pub fn all_terminal_surface_ids(&mut self) -> Vec<u32> {
        self.runtime.terminals.iter().map(|(id, _)| id).collect()
    }
}

impl EngineMut<'_> {
    pub fn mark_layout_dirty(&mut self) {
        self.persistence.dirty.mark_dirty();
    }
}
