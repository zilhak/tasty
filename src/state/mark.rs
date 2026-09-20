use super::AppState;
use crate::core::CoreState;

impl AppState {
    /// Read since mark on the focused terminal (or a specific surface).
    pub fn read_since_mark(
        &mut self,
        engine: &mut CoreState,
        surface_id: Option<u32>,
        strip_ansi: bool,
    ) -> String {
        if let Some(target_sid) = surface_id {
            engine
                .terminals
                .get_mut(target_sid)
                .map(|t| t.read_since_mark(strip_ansi))
                .unwrap_or_default()
        } else if let Some(terminal) = self.focused_terminal_mut(engine) {
            terminal.read_since_mark(strip_ansi)
        } else {
            String::new()
        }
    }

    /// Read what the output scanner has not seen yet on a specific surface and
    /// advance its cursor past it. Serves `surface.read_since_scan_mark`.
    ///
    /// Unlike [`Self::read_since_mark`] there is no focused-surface fallback:
    /// the caller is a scanner polling a surface it named, and a cursor that
    /// silently follows the focus would hand it another surface's output
    /// (`docs/adr/0307-the-output-scanner-reads-its-own-cursor.md`).
    pub fn take_since_output_scan_mark(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        strip_ansi: bool,
    ) -> String {
        engine
            .terminals
            .get_mut(surface_id)
            .map(|t| t.take_since_output_scan_mark(strip_ansi))
            .unwrap_or_default()
    }
}
