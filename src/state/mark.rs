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
}
