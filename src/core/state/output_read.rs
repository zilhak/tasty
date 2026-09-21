use super::CoreState;

impl CoreState {
    /// Read a specific surface's output since its mark (`surface.set_mark`).
    /// Empty when that surface has no terminal.
    ///
    /// The focused-surface fallback that used to sit on `AppState` had no
    /// caller and was removed
    /// (`docs/adr/0471-ipc-engine-handlers-reach-the-window-through-a-port.md`);
    /// every caller names the surface.
    pub fn read_since_mark_of(&mut self, surface_id: u32, strip_ansi: bool) -> String {
        self.terminals
            .get_mut(surface_id)
            .map(|t| t.read_since_mark(strip_ansi))
            .unwrap_or_default()
    }

    /// Read what the output scanner has not seen yet on a specific surface and
    /// advance its cursor past it. Serves `surface.read_since_scan_mark`.
    ///
    /// Unlike `surface.read_since_mark` there is no focused-surface fallback:
    /// the caller is a scanner polling a surface it named, and a cursor that
    /// silently follows the focus would hand it another surface's output
    /// (`docs/adr/0307-the-output-scanner-reads-its-own-cursor.md`).
    pub fn take_since_output_scan_mark(&mut self, surface_id: u32, strip_ansi: bool) -> String {
        self.terminals
            .get_mut(surface_id)
            .map(|t| t.take_since_output_scan_mark(strip_ansi))
            .unwrap_or_default()
    }

    /// Answer a read of a specific surface's raw output from a position the
    /// caller holds. `None` means that surface has no terminal.
    ///
    /// There is no focused-surface fallback, for the same reason
    /// [`Self::take_since_output_scan_mark`] has none: the caller named a
    /// surface and a read that silently followed the focus would hand it
    /// another surface's output.
    pub fn read_output(
        &mut self,
        surface_id: u32,
        req: &tasty_terminal::OutputReadRequest,
    ) -> Option<Result<tasty_terminal::OutputRead, tasty_terminal::OutputReadError>> {
        self.terminals
            .get_mut(surface_id)
            .map(|t| t.read_output(req))
    }
}
