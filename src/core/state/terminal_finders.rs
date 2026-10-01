use tasty_terminal::Terminal;

use super::CoreState;
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

    /// 화면과 같은 Terminal을 사용해야 선택 좌표와 복사 내용이 맞는다.
    /// hard 점유 중에는 readonly 사본만 반환하며 없다고 원본으로 대체하지 않는다.
    #[cfg(feature = "gui")]
    pub fn visible_terminal(&self, surface_id: u32) -> Option<&'a Terminal> {
        if self.live.occupancy.is_hard_occupied(surface_id) {
            self.readonly_view(surface_id)
        } else {
            self.runtime.terminals.get(surface_id)
        }
    }
}

impl EngineMut<'_> {
    pub fn find_terminal_by_id(&self, surface_id: u32) -> Option<&Terminal> {
        self.as_ref().find_terminal_by_id(surface_id)
    }
}
