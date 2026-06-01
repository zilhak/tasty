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
        if let Some(cmd) = self.settings.general.tasty_mode_init_command() {
            if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
                terminal.send_key(&cmd);
            }
        }
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
        let mut spawned = false;
        for ws in &mut self.workspaces {
            let pane_ids: Vec<u32> = ws.pane_layout().all_pane_ids();
            for pane_id in pane_ids {
                if let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id) {
                    for tab in &mut pane.tabs {
                        if tab.ensure_initialized(surface_id) {
                            spawned = true;
                            break;
                        }
                    }
                }
                if spawned {
                    break;
                }
            }
            if spawned {
                break;
            }
        }
        if spawned {
            self.send_fast_init(surface_id);
            self.apply_pending_scrollback_inject(surface_id);
        }
        spawned
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
    pub fn replace_terminal_by_id(
        &mut self,
        surface_id: u32,
        mut new_terminal: Terminal,
    ) -> anyhow::Result<()> {
        if let Some(old) = self.find_terminal_by_id_mut(surface_id) {
            std::mem::swap(old, &mut new_terminal);
            // old terminal (now in new_terminal) is dropped here, sending SIGHUP
            drop(new_terminal);
            Ok(())
        } else {
            anyhow::bail!("Surface {} not found", surface_id)
        }
    }

    /// Process all terminals (read PTY output).
    pub fn process_all(&mut self) -> bool {
        let mut any = false;
        for ws in &mut self.workspaces {
            if ws.pane_layout_mut().process_all() {
                any = true;
            }
        }
        any
    }

    /// Process a single terminal by surface ID (read PTY output).
    /// Returns true if data was processed.
    pub fn process_surface(&mut self, surface_id: u32) -> bool {
        if let Some(terminal) = self.find_terminal_by_id_mut(surface_id) {
            terminal.process()
        } else {
            false
        }
    }

    /// Flush deferred PTY resizes (throttled). Returns true if any terminal still has pending resize.
    pub fn flush_all_pty_resizes(&mut self) -> bool {
        let mut any_pending = false;
        for workspace in &mut self.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |_sid, terminal| {
                    terminal.flush_pty_resize();
                    if terminal.has_pending_pty_resize() {
                        any_pending = true;
                    }
                });
        }
        any_pending
    }

    /// Mark layout as dirty for persistence.
    pub fn mark_layout_dirty(&mut self) {
        self.layout_dirty.mark_dirty();
    }

    /// Force flush deferred PTY resizes (ignores throttle).
    /// Used after discrete events like pane split/close.
    pub fn force_flush_all_pty_resizes(&mut self) {
        for workspace in &mut self.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |_sid, terminal| {
                    terminal.force_flush_pty_resize();
                });
        }
    }

    /// Collect events from all terminals.
    pub fn collect_events(&mut self) -> Vec<TerminalEvent> {
        let mut all_events = Vec::new();
        for workspace in &mut self.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |sid, terminal| {
                    let mut events = terminal.take_events();
                    for event in &mut events {
                        event.surface_id = sid;
                    }
                    all_events.extend(events);
                });
        }
        all_events
    }

    /// Collect all terminal surface IDs across all workspaces.
    /// 현재는 CWD 폴링(macOS/Linux)에서만 사용된다.
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    pub fn all_terminal_surface_ids(&mut self) -> Vec<u32> {
        let mut ids = Vec::new();
        for workspace in &mut self.workspaces {
            workspace
                .pane_layout_mut()
                .for_each_terminal_mut(&mut |sid, _terminal| {
                    ids.push(sid);
                });
        }
        ids
    }
}
