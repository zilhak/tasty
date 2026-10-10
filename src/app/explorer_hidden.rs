//! 탐색기 숨김 파일 표시 여부를 model 에 남긴다. View 가 토글을 바로 바꿔 그리고, 렌더 뒤 이 경로로
//! `ExplorerPanel::show_hidden` 에 기록해 레이아웃 스냅샷에 싣는다.

use crate::runtime::engine_access::EngineMut;
use crate::runtime::surface_binding::SurfaceBinding;

/// 대상이 아직 같은 explorer 이고 값이 다를 때만 바꾸고 레이아웃을 더럽힌다.
pub(crate) fn apply(engine: &mut EngineMut<'_>, target: &SurfaceBinding, show: bool) {
    if !target.current(&engine.as_ref()) {
        return;
    }
    let Some(panel) = engine
        .runtime
        .surfaces
        .get_mut(&target.surface_id())
        .and_then(|surface| {
            surface
                .as_any_mut()
                .downcast_mut::<crate::model::ExplorerPanel>()
        })
    else {
        return;
    };
    if panel.show_hidden != show {
        panel.show_hidden = show;
        engine.mark_layout_dirty();
    }
}
