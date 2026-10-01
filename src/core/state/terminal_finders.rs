use tasty_terminal::Terminal;

use crate::runtime::engine_access::{EngineMut, EngineRef};

impl EngineMut<'_> {
    pub fn find_terminal_by_id_mut(&mut self, surface_id: u32) -> Option<&mut Terminal> {
        self.runtime.terminals.get_mut(surface_id)
    }
}

impl<'a> EngineRef<'a> {
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&'a Terminal> {
        self.runtime.terminals.get(surface_id)
    }
}

impl EngineMut<'_> {
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        self.as_ref().find_terminal_by_id(surface_id)
    }
}
