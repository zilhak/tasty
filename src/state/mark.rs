use super::AppState;

impl AppState {
    /// Set a read mark on the focused terminal (or a specific surface).
    pub fn set_mark(&mut self, surface_id: Option<u32>) {
        if let Some(target_sid) = surface_id {
            for workspace in &mut self.engine.workspaces {
                let mut found = false;
                workspace.pane_layout_mut().for_each_terminal_mut(&mut |sid, terminal| {
                    if sid == target_sid {
                        terminal.set_mark();
                        found = true;
                    }
                });
                if found {
                    return;
                }
            }
        } else if let Some(terminal) = self.focused_terminal_mut() {
            terminal.set_mark();
        }
    }

    /// Read since mark on the focused terminal (or a specific surface).
    pub fn read_since_mark(&mut self, surface_id: Option<u32>, strip_ansi: bool) -> String {
        if let Some(target_sid) = surface_id {
            let mut result = None;
            for workspace in &mut self.engine.workspaces {
                workspace.pane_layout_mut().for_each_terminal_mut(&mut |sid, terminal| {
                    if sid == target_sid && result.is_none() {
                        result = Some(terminal.read_since_mark(strip_ansi));
                    }
                });
                if result.is_some() {
                    break;
                }
            }
            result.unwrap_or_default()
        } else if let Some(terminal) = self.focused_terminal_mut() {
            terminal.read_since_mark(strip_ansi)
        } else {
            String::new()
        }
    }
}
