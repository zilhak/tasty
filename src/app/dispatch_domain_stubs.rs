//! 헤드리스에서 사용하는 창별 후속 처리 대체 함수.
//! 생성·복원·메타 변경의 GUI 처리는 생략하지만 workspace 이동의 활성 인덱스는 보정한다.
//! 구조 변경과 자원 정리는 두 빌드가 공유하는 core::structural_cascade가 담당한다.

#![cfg(not(feature = "gui"))]

use crate::core::CoreState;
use crate::core::intent::RestoredKind;
use crate::intent::IntentOrigin;
use crate::state::AppState;

/// 생성 결과는 두 빌드가 공유하지만 이 필드를 읽는 후속 처리는 GUI에만 있다.
#[expect(
    dead_code,
    reason = "headless cascade is a no-op; the fields are read only by the gui cascade"
)]
pub(crate) struct WorkspaceCreatedCascade {
    pub(crate) workspace_id: u32,
    pub(crate) index: usize,
    pub(crate) surface_id: Option<u32>,
    pub(crate) renamed_name: Option<String>,
    pub(crate) renamed_subtitle: Option<String>,
    pub(crate) renamed_description: Option<String>,
}

pub(crate) fn cascade_workspace_created(
    _state: &mut AppState,
    _engine: &mut CoreState,
    _origin: &IntentOrigin,
    _window_id: u64,
    _c: WorkspaceCreatedCascade,
) {
}

pub(crate) fn cascade_closed_item_restored(
    _state: &mut AppState,
    _engine: &mut CoreState,
    _origin: &IntentOrigin,
    _kind: RestoredKind,
) {
}

/// 헤드리스도 활성 workspace 인덱스가 실제 위치를 가리켜야 한다.
pub(crate) fn cascade_workspace_moved(state: &mut AppState, from_index: usize, to_index: usize) {
    state.fix_workspace_pointers_after_move(from_index, to_index);
}

pub(crate) fn cascade_workspace_meta_updated(
    _state: &mut AppState,
    _workspace_id: u32,
    _name: Option<String>,
    _subtitle: Option<String>,
    _description: Option<String>,
) {
}
