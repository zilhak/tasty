//! 구조 변경 뒤 필요한 창 상태 갱신·자원 정리·이벤트·튜토리얼 처리를 선언한다.
//! 도메인이 AppState 타입에 의존하지 않도록 창 쪽에서 이 trait을 구현한다.

use std::path::PathBuf;

use crate::core::CoreState;
#[cfg(feature = "gui")]
use crate::core::host_event::PendingHostEvent;

pub(crate) trait CascadeWindow {
    /// inherit_cwd 설정과 원래 surface의 cwd를 함께 확인한다.
    fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &CoreState,
        surface_id: u32,
    ) -> Option<PathBuf>;

    fn set_surface_meta(&self, surface_id: u32, key: &str, value: &str) -> std::io::Result<()>;

    /// 창 쪽 surface 자원을 정리하고 소요 시간을 sums에 더한다.
    fn cleanup_surface_traced(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        persist_id: Option<String>,
        sums: &mut crate::close_trace::CleanupSums,
    );

    fn fix_workspace_pointers_after_removal(&mut self, removed_idx: usize, remaining: usize);

    fn set_active_workspace(&mut self, index: usize);

    /// workspace.closed 통지와 해당 workspace의 메모리 정리를 처리한다.
    #[cfg(feature = "gui")]
    fn after_workspace_removed(&mut self, workspace_id: u32, path: &'static str);

    #[cfg(feature = "gui")]
    fn enqueue_surface_closed(
        &mut self,
        surface_id: u32,
        kind: Option<&'static str>,
        is_user_close: bool,
    );

    #[cfg(feature = "gui")]
    fn enqueue_host_event(&mut self, event: PendingHostEvent);

    #[cfg(feature = "gui")]
    fn lifecycle_baseline_insert_tab(
        &mut self,
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        kind: String,
    );

    /// 이전 탭 목록에서 알고 있던 pane. 목록이나 탭 정보가 없으면 None이다.
    #[cfg(feature = "gui")]
    fn lifecycle_baseline_pane_of(&self, tab_id: u32) -> Option<u32>;

    #[cfg(feature = "gui")]
    fn lifecycle_baseline_remove_tab(&mut self, tab_id: u32);

    #[cfg(feature = "gui")]
    fn observe_tutorial_surface_split(
        &mut self,
        engine: &CoreState,
        workspace_index: usize,
        pane_id: u32,
        new_surface_id: u32,
    );

    #[cfg(feature = "gui")]
    fn observe_tutorial_pane_split(&mut self, workspace: u32, original: u32, new_pane: u32);
}
