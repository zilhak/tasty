//! split 이 탐색기 칸을 하한(`explorer-min-height`) 아래로 만들지 않게 새 분할의 비율을 정한다.
//!
//! 엔진의 split 은 비율 0.5 로 정해지고 칸의 픽셀 크기를 모른다. 픽셀 크기는 그 workspace 를 보여 주는
//! 창의 터미널 영역에서 나오므로, 창이 journal 을 돌릴 때마다 창이 보여 주는 workspace 와
//! [`SplitGeometry`] 를 넘긴다([`ShownWorkspace`]). 같은 workspace 를 여러 창이 보여 주면 가장 큰
//! 창으로 판정한다. 어느 창도 그 workspace 를 보여 주지 않으면(헤드리스 포함) 하한을 적용하지 않는다.
//!
//! 비율은 하한을 지키는 범위에서 0.5 에 가장 가까운 값으로 고친다. 하한은 탐색기 칸에만 있고 다른 종류의
//! 칸은 최소가 0 이다. 그래서 탐색기 칸을 하한에 맞추면 탐색기인 형제 칸이 하한 아래가 될 때만 split 을
//! 거절한다. 판정 대상은 split 으로 높이가 바뀌는 칸뿐이다. 다른 칸과 창 크기 변경은 보지 않는다.

use crate::model::{
    PANE_BORDER_WIDTH, PhysicalPx, PhysicalRect, SURFACE_BORDER_WIDTH, SplitDirection, WorkspaceId,
};
use tasty_core::CreationDestination;
use tasty_ipc::protocol::JsonRpcResponse;

/// 이 거절을 다른 `-32602` 와 구별하는 `error.data.reason`.
pub(crate) const REFUSAL_REASON: &str = "explorer_min_height";

/// 하한을 지키는 비율 경계를 찾는 이분 탐색 횟수. 2^-16 비율은 4K 화면에서도 1px 보다 작다.
const SEARCH_STEPS: usize = 16;

/// 창의 터미널 영역. 그 창이 보여 주는 workspace 가 이 영역에 그려진다.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SplitGeometry {
    pub terminal_rect: PhysicalRect,
    pub tab_bar_height: PhysicalPx,
    pub scale: f32,
}

impl SplitGeometry {
    /// 배율을 걷어 낸 터미널 영역의 넓이. 배율이 다른 창끼리 크기를 비교할 때 쓴다.
    fn logical_area(&self) -> f32 {
        let scale = self.scale.max(f32::EPSILON);
        (self.terminal_rect.width.value() / scale) * (self.terminal_rect.height.value() / scale)
    }
}

/// 창 하나가 지금 보여 주는 workspace 와 그 창의 터미널 영역.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ShownWorkspace {
    pub workspace: WorkspaceId,
    pub geometry: SplitGeometry,
}

/// `workspace` 를 보여 주는 창 가운데 가장 큰 창의 터미널 영역.
fn largest_view(views: &[ShownWorkspace], workspace: WorkspaceId) -> Option<&SplitGeometry> {
    views
        .iter()
        .filter(|view| view.workspace == workspace)
        .map(|view| &view.geometry)
        .max_by(|a, b| a.logical_area().total_cmp(&b.logical_area()))
}

/// split 비율에 따라 높이가 바뀌는 탐색기 칸. 첫째 칸 쪽(`first`)은 비율이 클수록, 새로 생기는
/// 둘째 칸 쪽(`second`)은 비율이 작을수록 높아진다. `first_before` 는 split 전 첫째 쪽 칸의 높이로,
/// `first` 와 같은 순서다.
struct Affected<'a> {
    first: Box<dyn Fn(f32) -> Vec<PhysicalPx> + 'a>,
    first_before: Vec<PhysicalPx>,
    second: Box<dyn Fn(f32) -> Vec<PhysicalPx> + 'a>,
}

/// 새 분할의 비율을 하한에 맞게 고친다. 맞출 수 없으면 거절 응답을 돌려준다.
pub(super) fn hold(
    destination: &mut CreationDestination,
    new_kind: &str,
    core: &crate::core::CoreState,
    views: &[ShownWorkspace],
) -> Result<(), JsonRpcResponse> {
    let Some(geometry) =
        target_workspace(destination, core).and_then(|workspace| largest_view(views, workspace))
    else {
        return Ok(());
    };
    let floor_logical = crate::theme::theme().explorer_min_height();
    let floor = floor_logical.to_physical(geometry.scale);
    let new_is_explorer = new_kind == EXPLORER;
    let (split, affected) = match destination {
        CreationDestination::Pane { target, split, .. } => {
            let Some(affected) =
                pane_split(core, geometry, *target, split.direction, new_is_explorer)
            else {
                return Ok(());
            };
            (split, affected)
        }
        CreationDestination::Split { target, split } => {
            let Some(affected) =
                surface_split(core, geometry, *target, split.direction, new_is_explorer)
            else {
                return Ok(());
            };
            (split, affected)
        }
        _ => return Ok(()),
    };
    // 창이 작아 이미 하한보다 낮은 칸은 지금 높이 아래로만 줄지 않으면 된다(드래그 하한과 같은 규칙).
    let first_ok = |r: f32| {
        (affected.first)(r)
            .into_iter()
            .zip(&affected.first_before)
            .all(|(h, before)| h >= floor.min(*before))
    };
    let second_ok = |r: f32| (affected.second)(r).into_iter().all(|h| h >= floor);
    let current = split.ratio.to_f32();
    if first_ok(current) && second_ok(current) {
        return Ok(());
    }
    match feasible_ratio(current, &first_ok, &second_ok) {
        Some(ratio) => {
            split.ratio = tasty_core::Ratio::from_f32(ratio);
            Ok(())
        }
        None => Err(refusal(floor_logical.value())),
    }
}

const EXPLORER: &str = "explorer";

/// split 대상 칸이 속한 workspace.
fn target_workspace(
    destination: &CreationDestination,
    core: &crate::core::CoreState,
) -> Option<WorkspaceId> {
    let pane = match destination {
        CreationDestination::Pane { target, .. } => *target,
        CreationDestination::Split { target, .. } => {
            core.find_pane_for_tab(core.find_tab_for_surface(*target)?)?
        }
        _ => return None,
    };
    core.find_workspace_index_for_pane(pane)
        .and_then(|index| core.workspace_at(index))
        .map(|workspace| workspace.id)
}

/// 두 조건을 함께 만족하는 비율 중 `preferred` 에 가장 가까운 값. 첫째 조건은 비율이 커질수록,
/// 둘째 조건은 작아질수록 성립한다(단조). 범위 안에 그런 비율이 없으면 `None`.
fn feasible_ratio(
    preferred: f32,
    first_ok: &dyn Fn(f32) -> bool,
    second_ok: &dyn Fn(f32) -> bool,
) -> Option<f32> {
    // 탐색기가 아닌 칸의 최소는 0 이므로 분할선 드래그의 비율 범위로 좁히지 않는다.
    let (min, max) = (0.0, 1.0);
    if !first_ok(max) || !second_ok(min) {
        return None;
    }
    let low = if first_ok(min) {
        min
    } else {
        boundary(min, max, first_ok)
    };
    let high = if second_ok(max) {
        max
    } else {
        boundary(max, min, second_ok)
    };
    (low <= high && first_ok(low) && second_ok(high)).then(|| preferred.clamp(low, high))
}

/// `bad` 쪽에서 성립하지 않고 `good` 쪽에서 성립하는 조건의 경계를 찾아, 성립하는 쪽 값을 돌려준다.
fn boundary(bad: f32, good: f32, ok: &dyn Fn(f32) -> bool) -> f32 {
    let (mut bad, mut good) = (bad, good);
    for _ in 0..SEARCH_STEPS {
        let mid = (bad + good) * 0.5;
        if ok(mid) {
            good = mid;
        } else {
            bad = mid;
        }
    }
    good
}

/// 하한 거절 응답. 토스트 판정 시험(gui 전용)이 같은 응답을 쓴다.
#[cfg(all(test, feature = "gui"))]
pub(super) fn refusal_for_tests() -> JsonRpcResponse {
    refusal(crate::theme::theme().explorer_min_height().value())
}

fn refusal(floor: f32) -> JsonRpcResponse {
    JsonRpcResponse::error_with_data(
        serde_json::Value::Null,
        -32602,
        "split refused: not enough room",
        serde_json::json!({ "reason": REFUSAL_REASON, "explorer_min_height": floor }),
    )
}

/// pane 분할. 대상 pane 의 모든 탭이 첫째 칸 높이로 줄고, 새 pane 의 칸이 둘째 칸이 된다.
fn pane_split<'a>(
    core: &'a crate::core::CoreState,
    geometry: &'a SplitGeometry,
    target: u32,
    direction: SplitDirection,
    new_is_explorer: bool,
) -> Option<Affected<'a>> {
    let workspace = core
        .find_workspace_index_for_pane(target)
        .and_then(|index| core.workspace_at(index))?;
    let pane = core.find_pane_by_id(target)?;
    let (_, rect) = workspace
        .pane_layout()
        .compute_rects(geometry.terminal_rect, geometry.scale)
        .into_iter()
        .find(|(id, _)| *id == target)?;
    let gap = PANE_BORDER_WIDTH.to_physical(geometry.scale);
    let tab_bar = geometry.tab_bar_height;
    let pane_heights = move |pane_rect: PhysicalRect| -> Vec<PhysicalPx> {
        pane.tabs
            .iter()
            .filter_map(|tab| tab.layout_if_initialized())
            .flat_map(|layout| {
                explorer_heights(layout.surface_regions(content(pane_rect, tab_bar)))
            })
            .collect()
    };
    Some(Affected {
        first_before: pane_heights(rect),
        first: Box::new(move |r| pane_heights(rect.split_with_gap(direction, r, gap).0)),
        second: Box::new(move |r| {
            let (_, second) = rect.split_with_gap(direction, r, gap);
            new_is_explorer
                .then(|| content(second, tab_bar).height)
                .into_iter()
                .collect()
        }),
    })
}

/// surface 분할. 대상 surface 가 첫째 칸, 새 surface 가 둘째 칸이 된다.
fn surface_split<'a>(
    core: &'a crate::core::CoreState,
    geometry: &'a SplitGeometry,
    target: u32,
    direction: SplitDirection,
    new_is_explorer: bool,
) -> Option<Affected<'a>> {
    let tab_id = core.find_tab_for_surface(target)?;
    let pane_id = core.find_pane_for_tab(tab_id)?;
    let workspace = core
        .find_workspace_index_for_pane(pane_id)
        .and_then(|index| core.workspace_at(index))?;
    let tab = core
        .find_pane_by_id(pane_id)?
        .tabs
        .iter()
        .find(|tab| tab.id == tab_id)?;
    let (_, pane_rect) = workspace
        .pane_layout()
        .compute_rects(geometry.terminal_rect, geometry.scale)
        .into_iter()
        .find(|(id, _)| *id == pane_id)?;
    let region = tab
        .layout_if_initialized()?
        .surface_regions(content(pane_rect, geometry.tab_bar_height))
        .into_iter()
        .find(|region| region.id == target)?;
    let (rect, target_is_explorer) = (region.rect, region.surface.kind == EXPLORER);
    Some(Affected {
        first_before: target_is_explorer
            .then_some(rect.height)
            .into_iter()
            .collect(),
        first: Box::new(move |r| {
            let (first, _) = rect.split_with_gap(direction, r, SURFACE_BORDER_WIDTH);
            target_is_explorer
                .then_some(first.height)
                .into_iter()
                .collect()
        }),
        second: Box::new(move |r| {
            let (_, second) = rect.split_with_gap(direction, r, SURFACE_BORDER_WIDTH);
            new_is_explorer
                .then_some(second.height)
                .into_iter()
                .collect()
        }),
    })
}

/// pane 에서 탭 바를 뺀 내용 영역. 화면의 탐색기 하한 판정(`explorer_floor`)과 같은 계산이다.
fn content(pane_rect: PhysicalRect, tab_bar: PhysicalPx) -> PhysicalRect {
    PhysicalRect {
        y: pane_rect.y + tab_bar,
        height: (pane_rect.height - tab_bar).max(PhysicalPx(1.0)),
        ..pane_rect
    }
}

fn explorer_heights(regions: Vec<crate::model::SurfaceRegion<'_>>) -> Vec<PhysicalPx> {
    regions
        .into_iter()
        .filter(|region| region.surface.kind == EXPLORER)
        .map(|region| region.rect.height)
        .collect()
}

#[cfg(test)]
mod tests;
