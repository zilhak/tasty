//! `Terminal` 의 surface / cols / rows / process info / mark accessors.

use termwiz::surface::Surface;

use crate::{cwd, foreground_process, Terminal, TerminalEvent, BUSY_OUTPUT_WINDOW, INPUT_ECHO_WINDOW};

impl Terminal {

    pub fn surface(&self) -> &Surface {
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

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn rows(&self) -> usize {
        self.rows
    }

    /// Get the PID of the child process.
    pub fn process_id(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Get the foreground process info (name, PID) for this terminal.
    pub fn foreground_process_info(&self) -> Option<foreground_process::ForegroundProcessInfo> {
        let shell_pid = self.child.process_id()?;
        foreground_process::get_foreground_process(shell_pid)
    }

    /// Whether the terminal is currently considered "active" — that is, a
    /// non-shell foreground program is running AND the PTY has produced output
    /// within the last `BUSY_OUTPUT_WINDOW`. The output-window check lets idle
    /// TUIs (claude waiting for input, vim sitting still) drop out of the busy
    /// set while bursty programs (token streams, builds, tails) stay marked.
    ///
    /// Output that arrives within `INPUT_ECHO_WINDOW` after the last user
    /// keystroke is treated as echo and ignored, so typing into a waiting
    /// TUI (e.g. Claude prompt) does not trigger the busy indicator.
    ///
    /// Returns false when the shell is at its prompt, when foreground info
    /// cannot be resolved, or when the foreground program has been quiet long
    /// enough to look idle.
    pub fn is_busy(&self) -> bool {
        let Some(shell_pid) = self.child.process_id() else {
            return false;
        };
        let Some(info) = foreground_process::get_foreground_process(shell_pid) else {
            return false;
        };
        if info.pid == shell_pid {
            return false;
        }
        if foreground_process::is_known_shell_name(&info.name) {
            return false;
        }
        // Ignore output that looks like echo of recent user input.
        if self.last_output_at <= self.last_input_at + INPUT_ECHO_WINDOW {
            return false;
        }
        self.last_output_at.elapsed() < BUSY_OUTPUT_WINDOW
    }

    /// Get the current working directory of the child process.
    /// Returns the CWD cached from OSC 7 sequences. If no OSC 7 has been
    /// received yet, falls back to an OS-level query (proc_pidinfo on macOS,
    /// /proc on Linux). The fallback result is NOT cached to avoid stale data
    /// — OSC 7 remains the authoritative source when available.
    pub fn get_cwd(&self) -> Option<std::path::PathBuf> {
        if let Some(ref cwd) = self.cached_cwd {
            return Some(cwd.clone());
        }
        // On-demand fallback: query OS for CWD (microseconds on macOS/Linux, None on Windows)
        if let Some(pid) = self.process_id() {
            return cwd::get_cwd_of_pid(pid);
        }
        None
    }

    /// Set the cached CWD. Used by the OS-level CWD polling mechanism.
    pub fn set_cached_cwd(&mut self, cwd: std::path::PathBuf) {
        self.cached_cwd = Some(cwd);
    }

    /// Check if the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }

    /// Check if the child process has exited. Returns false if exited.
    pub fn check_process_alive(&mut self) -> bool {
        match self.child.try_wait() {
            Ok(Some(_status)) => false, // exited
            _ => true,
        }
    }

    /// Take all accumulated events, leaving the internal buffer empty.
    pub fn take_events(&mut self) -> Vec<TerminalEvent> {
        std::mem::take(&mut self.events)
    }

    /// Set a read mark at the current end of the output buffer.
    pub fn set_mark(&mut self) {
        self.output.set_mark();
    }

    /// Return raw bytes accumulated since the last `set_output_scan_mark()` call.
    /// Used by the ClaudeError scanner; independent of `read_since_mark`'s mark.
    pub fn output_since_scan_mark(&self, strip_ansi: bool) -> String {
        self.output.output_since_scan_mark(strip_ansi)
    }

    /// Advance the scan mark to the current end of the output buffer.
    pub fn set_output_scan_mark(&mut self) {
        self.output.set_scan_mark();
    }

    /// Read output since the last mark. If no mark was set, reads from the beginning.
    pub fn read_since_mark(&self, strip_ansi: bool) -> String {
        self.output.read_since_mark(strip_ansi)
    }

    // ---- Public getters for terminal state ----

}
