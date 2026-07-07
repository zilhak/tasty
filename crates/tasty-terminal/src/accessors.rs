//! Surface / cols / rows / process info / mark accessors.
//!
//! Grid/VTE 상태를 읽는 접근자는 `impl TerminalState` (락 안에서 동작), child/PTY
//! 를 만지는 접근자는 `impl Terminal` (핸들). 핸들 쪽 메서드는 필요 시 짧게 락을
//! 잡아 상태 필드를 읽는다 (ADR-0002).

use termwiz::surface::Surface;

use crate::{
    BUSY_OUTPUT_WINDOW, INPUT_ECHO_WINDOW, Terminal, TerminalEvent, TerminalState, cwd,
    foreground_process,
};

impl TerminalState {
    pub(crate) fn surface(&self) -> &Surface {
        if self.use_alternate {
            self.alternate_surface
                .as_ref()
                .unwrap_or(&self.primary_surface)
        } else {
            &self.primary_surface
        }
    }

    pub(crate) fn surface_mut(&mut self) -> &mut Surface {
        if self.use_alternate {
            self.alternate_surface
                .as_mut()
                .unwrap_or(&mut self.primary_surface)
        } else {
            &mut self.primary_surface
        }
    }

    /// Take all accumulated events, leaving the internal buffer empty.
    pub(crate) fn take_events(&mut self) -> Vec<TerminalEvent> {
        std::mem::take(&mut self.events)
    }

    pub(crate) fn set_output_events_enabled(&mut self, enabled: bool) {
        self.emit_output_events = enabled;
    }

    pub(crate) fn set_mark(&mut self) {
        self.output.set_mark();
    }

    pub(crate) fn output_since_scan_mark(&self, strip_ansi: bool) -> String {
        self.output.output_since_scan_mark(strip_ansi)
    }

    pub(crate) fn set_output_scan_mark(&mut self) {
        self.output.set_scan_mark();
    }

    pub(crate) fn read_since_mark(&self, strip_ansi: bool) -> String {
        self.output.read_since_mark(strip_ansi)
    }

    pub(crate) fn set_cached_cwd(&mut self, cwd: std::path::PathBuf) {
        self.cached_cwd = Some(cwd);
    }
}

impl Terminal {
    /// Grid columns. Served from the handle-side cache (lock-free) — kept in sync
    /// by `resize()`.
    pub fn cols(&self) -> usize {
        self.cached_dims.0
    }

    /// Grid rows. Served from the handle-side cache (lock-free).
    pub fn rows(&self) -> usize {
        self.cached_dims.1
    }

    /// Get the PID of the child process. `None` for a detached mirror (no child).
    pub fn process_id(&self) -> Option<u32> {
        self.pty.as_ref()?.child.process_id()
    }

    /// Whether this terminal is a detached mirror (no PTY/child). Its grid is
    /// authoritative from the remote handshake/resize and must NOT be overwritten
    /// by the local layout resize sweep — the local resize path skips these.
    pub fn is_detached(&self) -> bool {
        self.pty.is_none()
    }

    /// Get the foreground process info (name, PID) for this terminal.
    pub fn foreground_process_info(&self) -> Option<foreground_process::ForegroundProcessInfo> {
        let shell_pid = self.process_id()?;
        foreground_process::get_foreground_process(shell_pid)
    }

    /// Whether the terminal is currently considered "active" — a non-shell
    /// foreground program is running AND the PTY produced output within the last
    /// `BUSY_OUTPUT_WINDOW`. Output within `INPUT_ECHO_WINDOW` after the last
    /// keystroke is treated as echo and ignored.
    pub fn is_busy(&self) -> bool {
        let Some(shell_pid) = self.process_id() else {
            return false;
        };
        let info = foreground_process::get_foreground_process(shell_pid);
        self.busy_with_foreground(shell_pid, info.as_ref())
    }

    /// Same busy decision as [`is_busy`](Self::is_busy), but the (expensive on
    /// Windows) foreground lookup is supplied by the caller. The batch poll in
    /// `refresh_busy_surfaces` resolves every surface's foreground from a single
    /// system snapshot and then calls this per terminal, turning a per-surface
    /// snapshot into one snapshot per tick. `shell_pid` must be this terminal's
    /// own child PID and `foreground` the result of resolving it.
    pub fn busy_with_foreground(
        &self,
        shell_pid: u32,
        foreground: Option<&foreground_process::ForegroundProcessInfo>,
    ) -> bool {
        let Some(info) = foreground else {
            return false;
        };
        if info.pid == shell_pid {
            return false;
        }
        if foreground_process::is_known_shell_name(&info.name) {
            return false;
        }
        // Non-blocking: `refresh_busy_surfaces` polls every terminal at 1Hz, and a
        // blocking lock here would wait on each busy parser thread mid-ingest,
        // spiking the input thread's tail latency (ADR-0002). A contended lock
        // means the parser is actively ingesting output → that is "busy".
        let st = match self.state.try_lock() {
            Ok(st) => st,
            Err(std::sync::TryLockError::WouldBlock) => return true,
            Err(std::sync::TryLockError::Poisoned(p)) => p.into_inner(),
        };
        // Ignore output that looks like echo of recent user input.
        if st.last_output_at <= st.last_input_at + INPUT_ECHO_WINDOW {
            return false;
        }
        st.last_output_at.elapsed() < BUSY_OUTPUT_WINDOW
    }

    /// Get the current working directory of the child process. Prefers the CWD
    /// cached from OSC 7; falls back to an OS-level query (not cached).
    pub fn get_cwd(&self) -> Option<std::path::PathBuf> {
        if let Some(cwd) = self.lock_state().cached_cwd.clone() {
            return Some(cwd);
        }
        if let Some(pid) = self.process_id() {
            return cwd::get_cwd_of_pid(pid);
        }
        None
    }

    /// Set the cached CWD. Used by the OS-level CWD polling mechanism.
    pub fn set_cached_cwd(&mut self, cwd: std::path::PathBuf) {
        self.lock_state().set_cached_cwd(cwd);
    }

    /// Current window title (last value emitted via OSC 0/2), if any. The host
    /// projects the focused surface's title onto its tab name — mirrors the
    /// `get_cwd` lock pattern (short lock to clone the field).
    pub fn current_title(&self) -> Option<String> {
        self.lock_state().current_title.clone()
    }

    /// Check if the child process is still running. A detached mirror has no
    /// child; reported as alive.
    pub fn is_alive(&mut self) -> bool {
        match self.pty.as_mut() {
            Some(pty) => pty.child.try_wait().ok().flatten().is_none(),
            None => true,
        }
    }

    /// Check if the child process has exited. Returns false if exited. A detached
    /// mirror (no child) is always considered alive.
    pub fn check_process_alive(&mut self) -> bool {
        match self.pty.as_mut() {
            Some(pty) => !matches!(pty.child.try_wait(), Ok(Some(_status))),
            None => true,
        }
    }

    /// Take all accumulated events, leaving the internal buffer empty.
    pub fn take_events(&mut self) -> Vec<TerminalEvent> {
        self.lock_state().take_events()
    }

    /// Like [`take_events`](Self::take_events) but never blocks: if the parser
    /// thread currently holds the state lock (mid-chunk ingest), returns `None`
    /// and leaves the events buffered for a later poll. The host's per-wake event
    /// drain iterates *every* terminal, so a blocking take would re-serialize the
    /// input thread against all busy parser threads — defeating ADR-0002. Events
    /// are never lost: the parser wakes the loop again after each ingest.
    pub fn try_take_events(&mut self) -> Option<Vec<TerminalEvent>> {
        match self.state.try_lock() {
            Ok(mut st) => Some(st.take_events()),
            Err(std::sync::TryLockError::WouldBlock) => None,
            Err(std::sync::TryLockError::Poisoned(p)) => Some(p.into_inner().take_events()),
        }
    }

    /// Enable/disable `OutputAppended` emission. Defaults to off. Lock-free no-op
    /// when the gate is unchanged (the common per-wake case).
    pub fn set_output_events_enabled(&mut self, enabled: bool) {
        if self.cached_emit_events == enabled {
            return;
        }
        self.cached_emit_events = enabled;
        self.lock_state().set_output_events_enabled(enabled);
    }

    /// Set a read mark at the current end of the output buffer.
    pub fn set_mark(&mut self) {
        self.lock_state().set_mark();
    }

    /// Return raw bytes accumulated since the last `set_output_scan_mark()` call.
    pub fn output_since_scan_mark(&self, strip_ansi: bool) -> String {
        self.lock_state().output_since_scan_mark(strip_ansi)
    }

    /// Advance the scan mark to the current end of the output buffer.
    pub fn set_output_scan_mark(&mut self) {
        self.lock_state().set_output_scan_mark();
    }

    /// Read output since the last mark. If no mark was set, reads from the beginning.
    pub fn read_since_mark(&self, strip_ansi: bool) -> String {
        self.lock_state().read_since_mark(strip_ansi)
    }
}
