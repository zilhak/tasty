//! 구조 도메인 실행(분할·탭·닫기)이 **창 쪽에서 갱신해야 하는 것** — 도메인이 소유하는 포트.
//!
//! 분할·닫기 같은 구조 op 은 `Core`/`CoreState` 만으로 끝나지 않는다. 활성 워크스페이스
//! 포인터 보정, 닫힌 surface 의 창 쪽 자원 회수, plugin 에 나갈 호스트 이벤트 큐잉,
//! 튜토리얼 관찰은 창 상태(`AppState`)의 일이다. 도메인 실행(`core::structural_exec`)과
//! cascade(`core::structural_cascade`)가 그 일을 `AppState` 라는 **타입 이름으로** 부르면
//! 도메인이 창 모듈에 컴파일 의존한다. 그래서 도메인이 필요한 연산만 이 trait 으로 선언하고,
//! 창 쪽(`state::cascade_window`)이 구현한다.
//!
//! 메서드 하나하나는 `AppState` 의 같은 이름 메서드로 그대로 위임된다 — 이 trait 은 의존의
//! 방향을 뒤집을 뿐 동작을 바꾸지 않는다. gui 로 가린 메서드는 호출 자리가 이미 gui 로
//! 가려진 것들이다(`structural_cascade` 모듈 문서의 "갈리는 지점").

use std::path::PathBuf;

use crate::core::CoreState;
#[cfg(feature = "gui")]
use crate::core::host_event::PendingHostEvent;

pub(crate) trait CascadeWindow {
    /// 새 surface 의 cwd 상속 — 설정(`inherit_cwd`)과 원 surface 의 cwd 를 본다.
    fn resolve_inherit_cwd_from_surface(
        &self,
        engine: &CoreState,
        surface_id: u32,
    ) -> Option<PathBuf>;

    /// 분할이 실어 온 surface 메타데이터 한 칸을 메모리 저장소에 쓴다.
    fn set_surface_meta(&self, surface_id: u32, key: &str, value: &str) -> std::io::Result<()>;

    /// 닫힌 surface 의 창 쪽 자원 회수(계측 합산 포함).
    fn cleanup_surface_traced(
        &mut self,
        engine: &mut CoreState,
        surface_id: u32,
        persist_id: Option<String>,
        sums: &mut crate::close_trace::CleanupSums,
    );

    /// 워크스페이스 하나가 빠진 뒤 활성·직전 포인터를 보정한다.
    fn fix_workspace_pointers_after_removal(&mut self, removed_idx: usize, remaining: usize);

    /// 빈 창을 채우려 새로 만든 워크스페이스를 활성으로 둔다.
    fn set_active_workspace(&mut self, index: usize);

    /// 워크스페이스 제거 통지(`workspace.closed`) + 그 워크스페이스의 메모리 scope 정리.
    #[cfg(feature = "gui")]
    fn after_workspace_removed(&mut self, workspace_id: u32, path: &'static str);

    /// `surface.closed` lifecycle 통지를 큐에 넣는다.
    #[cfg(feature = "gui")]
    fn enqueue_surface_closed(
        &mut self,
        surface_id: u32,
        kind: Option<&'static str>,
        is_user_close: bool,
    );

    /// 호스트 이벤트(plugin event bus · Lua hook)를 큐에 넣는다.
    #[cfg(feature = "gui")]
    fn enqueue_host_event(&mut self, event: PendingHostEvent);

    /// 탭 lifecycle polling 기준선에 새 탭을 넣는다.
    #[cfg(feature = "gui")]
    fn lifecycle_baseline_insert_tab(
        &mut self,
        tab_id: u32,
        pane_id: u32,
        workspace_id: u32,
        kind: String,
    );

    /// 탭 lifecycle polling 기준선이 기억하는 그 탭의 pane. 기준선이 아직 없거나 그 탭을
    /// 모르면 `None`.
    #[cfg(feature = "gui")]
    fn lifecycle_baseline_pane_of(&self, tab_id: u32) -> Option<u32>;

    /// 탭 lifecycle polling 기준선에서 탭을 뺀다.
    #[cfg(feature = "gui")]
    fn lifecycle_baseline_remove_tab(&mut self, tab_id: u32);

    /// 사용자 surface 분할을 튜토리얼이 관찰한다.
    #[cfg(feature = "gui")]
    fn observe_tutorial_surface_split(
        &mut self,
        engine: &CoreState,
        workspace_index: usize,
        pane_id: u32,
        new_surface_id: u32,
    );

    /// 사용자 pane 분할을 튜토리얼이 관찰한다.
    #[cfg(feature = "gui")]
    fn observe_tutorial_pane_split(&mut self, workspace: u32, original: u32, new_pane: u32);
}
