use tasty_terminal::{Terminal, TerminalEvent};

use super::CoreState;

impl CoreState {
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
        // tasty 모드의 bashrc source 는 셸 `--rcfile` 인자로 처리한다
        // (effective_shell_args). 더 이상 PTY 입력으로 보내지 않는다 — 그래야
        // 화면 echo / 복원 시 claude 입력창 오염이 없다.
        let startup = self.settings.general.startup_command.trim();
        if !startup.is_empty() {
            let line = format!("{startup}\n");
            if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
                terminal.send_key(&line);
            }
        }
    }

    /// Return `true` if the given surface_id is a deferred placeholder waiting
    /// for lazy PTY spawn (i.e. an `EmptySurface { deferred_spawn: Some(..) }` leaf
    /// in any tab layout).
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

    /// Lazy PTY init for a deferred surface. Returns `true` if a PTY was just
    /// spawned for this surface, `false` otherwise (already initialized or the
    /// surface_id isn't a known deferred placeholder).
    ///
    /// This is the IPC/CLI-facing counterpart to the workspace-switch path
    /// (`ensure_active_workspace_initialized`). It does *not* change focus,
    /// active workspace, or active tab — only the target surface's underlying
    /// PTY is materialized.
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

    /// Deferred terminal 이 spawn 된 직후 호출. layout 복원 시 큐에 적재된
    /// scrollback line 들을 해당 surface 의 terminal 에 inject. PTY 가 실제로
    /// 출력하기 전이라 사용자는 위로 스크롤하면 자연스러운 히스토리를 본다.
    pub fn apply_pending_scrollback_inject(&mut self, surface_id: u32) {
        let Some(lines) = self.pending_scrollback_inject.remove(&surface_id) else {
            return;
        };
        if lines.is_empty() {
            return;
        }
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.inject_scrollback(lines);
            // 새 prompt 가 화면 중간부터 시작하도록 visible 상단 절반에 옛
            // 라인을 미리 그려둔다.
            let prefill = terminal.rows() / 2;
            terminal.prefill_visible_from_scrollback(prefill);
        }
    }

    /// Replace the terminal in a TerminalSurface, keeping the surface/layout intact.
    /// The old terminal's PTY process is dropped (SIGHUP sent).
    /// Returns Ok(()) on success, Err if the surface was not found.
    ///
    /// **D.3.E.4.f** — `TerminalStore::replace` 로 cutover. layout 트리의 옛
    /// Terminal owner 경로는 더 이상 사용 안 함.
    pub fn replace_terminal_by_id(
        &mut self,
        surface_id: u32,
        new_terminal: Terminal,
    ) -> anyhow::Result<()> {
        if let Some(old) = self.terminals.replace(surface_id, new_terminal) {
            drop(old); // SIGHUP
            return Ok(());
        }
        anyhow::bail!("Surface {} not found", surface_id)
    }

    /// terminal 들의 `OutputAppended` emit 게이트를 현재 옵저버 집합과 동기화.
    /// process 직전(lazy) + observer register/unregister 직후(eager) 호출 —
    /// terminal 생성 콜사이트가 게이트 초기화를 신경 쓸 필요가 없다.
    pub(crate) fn sync_output_event_gates(&mut self) {
        let router = &self.observer_router;
        for (sid, t) in self.terminals.iter_mut() {
            t.set_output_events_enabled(router.wants(sid));
        }
    }

    /// Process all terminals (read PTY output). **D.3.E.4.f** — store 가 owner.
    pub fn process_all(&mut self) -> bool {
        self.sync_output_event_gates();
        self.terminals.process_all()
    }

    /// OS 절전 복귀 후 헬스 패스 (Windows, ADR-0017). 살아있는 PTY 자식을 wake
    /// nudge 해 hang 에서 깨어나도록 유도하고, **자식 TUI 가 실행 중인(foreground
    /// 가 셸이 아닌) 살아있는 surface 들의 ID** 를 의심 목록으로 반환한다. 죽은
    /// 자식은 여기서 건드리지 않고 곧이은 `process_all` 의 `ProcessExited` cascade
    /// 가 정리한다. 호출자는 의심 목록으로 사용자 알림을 발행한다.
    #[cfg(windows)]
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

    /// Process a single terminal by surface ID (read PTY output).
    /// Returns true if data was processed.
    pub fn process_surface(&mut self, surface_id: u32) -> bool {
        let enabled = self.observer_router.wants(surface_id);
        if let Some(t) = self.terminals.get_mut(surface_id) {
            t.set_output_events_enabled(enabled);
        }
        self.terminals.process_surface(surface_id)
    }

    /// Flush deferred PTY resizes (throttled). Returns true if any terminal still has pending resize.
    pub fn flush_all_pty_resizes(&mut self) -> bool {
        self.terminals.flush_pty_resizes()
    }

    /// Mark layout as dirty for persistence.
    pub fn mark_layout_dirty(&mut self) {
        self.layout_dirty.mark_dirty();
    }

    /// Collect events from all terminals. **D.3.E.4.f** — store iter.
    ///
    /// Uses a non-blocking take: a terminal whose parser thread currently holds
    /// the state lock (mid-chunk ingest) is skipped this round, so the input
    /// thread never serializes against busy parser threads (ADR-0002). Skipped
    /// events are not lost — the parser wakes the loop again after each ingest.
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

    /// Collect all terminal surface IDs across all workspaces.
    /// 현재는 CWD 폴링(macOS/Linux)에서만 사용된다.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn all_terminal_surface_ids(&mut self) -> Vec<u32> {
        self.terminals.iter().map(|(id, _)| id).collect()
    }
}
