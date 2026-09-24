use std::time::Instant;

use tasty_terminal::{Terminal, TerminalEvent};

use super::CoreState;

impl CoreState {
    /// 유휴 TTL이 지난 등록을 지우고 Terminal과 waker 기록도 함께 정리한다.
    /// 실제 자식 종료·회수는 Terminal의 소유권과 플랫폼별 Drop 처리에 달려 있다.
    pub(crate) fn sweep_idle_ptys(&mut self, now: Instant) -> Vec<u32> {
        let expired = self.pty_registry.sweep_idle(now);
        for pty_id in &expired {
            let pty_id = *pty_id;
            self.terminals.remove(pty_id);
            if let Some(factory) = self.waker_factory.as_ref() {
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

    pub fn is_surface_deferred(&self, surface_id: u32) -> bool {
        for ws in &self.workspaces {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        if tab.is_surface_deferred(surface_id) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// mesh 메타데이터를 찾는다. 허용 종류·plugin 검증은 attach 호출자가 따로 한다.
    pub(crate) fn find_mesh_surface_info(&self, surface_id: u32) -> Option<(String, String)> {
        for ws in &self.workspaces {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        if let Some(layout) = tab.layout_if_initialized()
                            && let Some(surface) = layout.find_surface(surface_id)
                            && let Some((kind, plugin_id)) = surface.attach_mesh_info()
                        {
                            return Some((kind.to_string(), plugin_id.to_string()));
                        }
                    }
                }
            }
        }
        None
    }

    /// attach의 surface.create에 필요한 파일·표시 이름까지 얻으려고 host의 mesh 타입을 조회한다.
    pub(crate) fn find_egui_mesh_surface(
        &self,
        surface_id: u32,
    ) -> Option<&crate::core::egui_mesh_surface::EguiMeshSurface> {
        for ws in &self.workspaces {
            let pane_ids = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout().find_pane(pane_id) {
                    for tab in &pane.tabs {
                        if let Some(layout) = tab.layout_if_initialized()
                            && let Some(surface) = layout.find_surface(surface_id)
                            && let Some(ms) = surface
                                .as_any()
                                .downcast_ref::<crate::core::egui_mesh_surface::EguiMeshSurface>(
                            )
                        {
                            return Some(ms);
                        }
                    }
                }
            }
        }
        None
    }

    /// deferred PTY 생성에 성공하면 true다. 포커스·활성 workspace·활성 tab은 바꾸지 않는다.
    pub fn ensure_surface_initialized(&mut self, surface_id: u32) -> bool {
        let mut spawned: Option<(Terminal, Option<String>)> = None;
        'outer: for ws in &mut self.workspaces {
            let pane_ids: Vec<u32> = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    for tab in &mut pane.tabs {
                        if let Some(result) = tab.ensure_initialized(surface_id) {
                            spawned = Some(result);
                            break 'outer;
                        }
                    }
                }
            }
        }
        if let Some((terminal, persist_id)) = spawned {
            self.terminals.insert(surface_id, terminal);
            if let Some(pid) = persist_id {
                self.terminals.set_scrollback_persist_id(surface_id, pid);
            }
            self.send_fast_init(surface_id);
            self.apply_pending_scrollback_inject(surface_id);
            true
        } else {
            false
        }
    }

    /// 등록된 종류로 placeholder 복원을 시도한다. 종류가 없거나 복원에 실패하면 placeholder가 남는다.
    #[cfg(any(feature = "gui", test))]
    pub fn reify_plugin_surface(&mut self, surface_id: u32) -> bool {
        let registry = self.surface_registry.clone();
        for ws in &mut self.workspaces {
            let pane_ids: Vec<u32> = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    for tab in &mut pane.tabs {
                        if tab.reify_deferred_plugin(surface_id, |kind, snap| {
                            let def = registry.get_live(kind)?;
                            (def.restore)(surface_id, snap).ok()
                        }) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// 대기 중인 scrollback을 꺼내 Terminal에 적용한다. Terminal이 없으면 꺼낸 내용은 버린다.
    /// 이미 시작한 PTY의 첫 출력보다 먼저 적용된다고 보장하지는 않는다.
    pub fn apply_pending_scrollback_inject(&mut self, surface_id: u32) {
        let Some(lines) = self.pending_scrollback_inject.remove(&surface_id) else {
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
        new_terminal: Terminal,
    ) -> anyhow::Result<()> {
        if let Some(old) = self.terminals.replace(surface_id, new_terminal) {
            drop(old);
            return Ok(());
        }
        anyhow::bail!("Surface {} not found", surface_id)
    }

    /// OutputAppended 발생 여부를 현재 observer 목록에 맞춘다. 생성 직후만 설정하면 등록 변경을 놓친다.
    pub(crate) fn sync_output_event_gates(&mut self) {
        let router = &self.observer_router;
        let hook_manager = &self.hook_manager;
        for (sid, t) in self.terminals.iter_mut() {
            t.set_output_events_enabled(
                router.wants(sid) || hook_manager.has_output_match_hook(sid),
            );
        }
    }

    pub fn process_all(&mut self) -> bool {
        self.sync_output_event_gates();
        self.terminals.process_all()
    }

    /// Windows 절전 복귀 후 살아 있는 자식에 wake를 시도하고 비셸 전경 surface를 알림 후보로 반환한다.
    /// 실제 응답 회복을 확인한 결과는 아니다.
    #[cfg(all(windows, feature = "gui"))]
    pub(crate) fn wake_terminals_after_resume(&mut self) -> Vec<u32> {
        let mut suspects = Vec::new();
        for (sid, term) in self.terminals.iter_mut() {
            if !term.check_process_alive() {
                continue; // 죽음 — process_all 의 ProcessExited cascade 가 정리.
            }
            term.wake_nudge();
            let shell_pid = term.process_id();
            if let Some(info) = term.foreground_process_info()
                && Some(info.pid) != shell_pid
                && !tasty_terminal::foreground_process::is_known_shell_name(&info.name)
            {
                suspects.push(sid);
            }
        }
        suspects
    }

    pub fn process_surface(&mut self, surface_id: u32) -> bool {
        let enabled = self.observer_router.wants(surface_id)
            || self.hook_manager.has_output_match_hook(surface_id);
        if let Some(t) = self.terminals.get_mut(surface_id) {
            t.set_output_events_enabled(enabled);
        }
        self.terminals.process_surface(surface_id)
    }

    #[cfg(feature = "gui")]
    pub fn flush_all_pty_resizes(&mut self) -> bool {
        self.terminals.flush_pty_resizes()
    }

    pub fn mark_layout_dirty(&mut self) {
        self.layout_dirty.mark_dirty();
    }

    /// 락을 즉시 얻지 못한 Terminal은 이번 수집에서 건너뛴다.
    /// 그 이벤트는 큐에 남지만 여기서 다음 호출 시각이나 전달 성공까지 보장하지는 않는다.
    pub fn collect_events(&mut self) -> Vec<TerminalEvent> {
        let mut all_events = Vec::new();
        for (sid, terminal) in self.terminals.iter_mut() {
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
        self.terminals.iter().map(|(id, _)| id).collect()
    }
}
