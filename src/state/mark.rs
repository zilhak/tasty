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
            let mut result = None;
            for workspace in &mut engine.workspaces {
                workspace
                    .pane_layout_mut()
                    .for_each_terminal_mut(&mut |sid, terminal| {
                        if sid == target_sid && result.is_none() {
                            result = Some(terminal.read_since_mark(strip_ansi));
                        }
                    });
                if result.is_some() {
                    break;
                }
            }
            result.unwrap_or_default()
        } else if let Some(terminal) = self.focused_terminal_mut(engine) {
            terminal.read_since_mark(strip_ansi)
        } else {
            String::new()
        }
    }
}
