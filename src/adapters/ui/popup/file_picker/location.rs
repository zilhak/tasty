//! 호출자가 피커를 열기 전에 둘러볼 위치를 정하는 도우미. 피커는 받은 위치만 둘러본다.

use crate::core::state::SurfaceCwd;
use crate::runtime::engine_read::EngineRead;
use crate::state::{FilePickerLocation, MainViewState};

/// `origin_surface_id`가 속한 workspace, 찾지 못하면 호출자가 고른 `fallback_ws_index`로 로컬·원격을 정한다.
/// `dir`이 없으면 출발 surface의 cwd에서 출발한다. 원격이면 결과를 열 때 쓸 surface도 함께 정한다.
/// 출발 surface가 그 mirror workspace에 없으면 그 workspace에서 이 창이 선택한 surface를 쓴다.
pub(crate) fn locate(
    state: &MainViewState,
    engine: &EngineRead<'_>,
    origin_surface_id: Option<u32>,
    fallback_ws_index: usize,
    dir: Option<String>,
) -> FilePickerLocation {
    let origin_ws = origin_surface_id
        .and_then(|sid| engine.find_workspace_index_for_surface(sid))
        .map(|(idx, _)| idx);
    let dir = dir.or_else(|| {
        origin_surface_id
            .and_then(|sid| engine.surface_cwd(sid))
            .map(|cwd| match cwd {
                SurfaceCwd::Local(p) => p.to_string_lossy().into_owned(),
                SurfaceCwd::Remote(r) => r.as_str().to_string(),
            })
    });
    let Some(ws) = engine
        .workspace_at(origin_ws.unwrap_or(fallback_ws_index))
        .filter(|ws| ws.mirror)
    else {
        return FilePickerLocation::Local { dir };
    };
    let surface_id = match origin_ws {
        Some(_) => origin_surface_id,
        None => selected_surface(state, engine, ws),
    };
    FilePickerLocation::Remote {
        mirror_ws_id: ws.id,
        surface_id,
        dir,
    }
}

/// workspace에서 이 창이 선택한 pane의 선택 탭 surface. 없으면 workspace의 첫 surface다.
fn selected_surface(
    state: &MainViewState,
    engine: &EngineRead<'_>,
    ws: &crate::model::Workspace,
) -> Option<u32> {
    state
        .navigation
        .pane_id(ws)
        .and_then(|pane_id| engine.find_pane_by_id(pane_id))
        .and_then(|pane| pane.tabs.get(state.navigation.tab_index(pane)))
        .and_then(|tab| state.navigation.surface_id(tab))
        .or_else(|| ws.all_surface_ids().into_iter().next())
}
