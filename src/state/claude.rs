use super::{AppState, ClaudeChildEntry};

impl AppState {
    /// Get the next child index for a parent, incrementing the counter.
    pub fn next_child_index(&mut self, parent_id: u32) -> u32 {
        let idx = self.engine.claude_next_child_index.entry(parent_id).or_insert(0);
        *idx += 1;
        *idx
    }

    /// Register a child entry under a parent surface.
    pub fn register_child(&mut self, parent_id: u32, entry: ClaudeChildEntry) {
        self.engine.claude_child_parent.insert(entry.child_surface_id, parent_id);
        self.engine.claude_parent_children.entry(parent_id).or_default().push(entry);
    }

    /// Unregister a child surface. Cleans up parent tracking if parent is closed and has no more children.
    pub fn unregister_child(&mut self, child_surface_id: u32) {
        self.engine.claude_idle_state.remove(&child_surface_id);
        self.engine.claude_needs_input_state.remove(&child_surface_id);
        if let Some(parent_id) = self.engine.claude_child_parent.remove(&child_surface_id) {
            if let Some(children) = self.engine.claude_parent_children.get_mut(&parent_id) {
                children.retain(|c| c.child_surface_id != child_surface_id);
                if children.is_empty() && self.engine.claude_closed_parents.contains(&parent_id) {
                    self.engine.claude_parent_children.remove(&parent_id);
                    self.engine.claude_closed_parents.remove(&parent_id);
                    self.engine.claude_next_child_index.remove(&parent_id);
                }
            }
        }
    }

    /// Mark a parent surface as closed. If it has no children, clean up immediately.
    pub fn mark_parent_closed(&mut self, parent_surface_id: u32) {
        self.engine.claude_idle_state.remove(&parent_surface_id);
        self.engine.claude_needs_input_state.remove(&parent_surface_id);
        if self.engine.claude_parent_children.contains_key(&parent_surface_id) {
            let children_empty = self.engine.claude_parent_children
                .get(&parent_surface_id)
                .map(|c| c.is_empty())
                .unwrap_or(true);
            if children_empty {
                self.engine.claude_parent_children.remove(&parent_surface_id);
                self.engine.claude_next_child_index.remove(&parent_surface_id);
            } else {
                self.engine.claude_closed_parents.insert(parent_surface_id);
            }
        }
    }

    /// Get all children of a parent surface.
    pub fn children_of(&self, parent_id: u32) -> &[ClaudeChildEntry] {
        self.engine.claude_parent_children.get(&parent_id).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get the parent of a child surface.
    pub fn parent_of(&self, child_id: u32) -> Option<u32> {
        self.engine.claude_child_parent.get(&child_id).copied()
    }

    /// Set the Claude idle state for a surface. When becoming non-idle, also clears needs_input.
    pub fn set_claude_idle(&mut self, surface_id: u32, idle: bool) {
        self.engine.claude_idle_state.insert(surface_id, idle);
        if !idle {
            self.engine.claude_needs_input_state.remove(&surface_id);
        }
    }

    /// Set the Claude needs-input state for a surface.
    pub fn set_claude_needs_input(&mut self, surface_id: u32, needs_input: bool) {
        self.engine.claude_needs_input_state.insert(surface_id, needs_input);
    }

    /// Get the Claude state string for a surface: "needs_input", "idle", or "active".
    pub fn claude_state_of(&self, surface_id: u32) -> &str {
        if self.engine.claude_needs_input_state.get(&surface_id).copied().unwrap_or(false) {
            "needs_input"
        } else if self.engine.claude_idle_state.get(&surface_id).copied().unwrap_or(false) {
            "idle"
        } else {
            "active"
        }
    }
}
