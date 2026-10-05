//! 이동 대기 대상 표시. 슬롯이 가리키는 서피스·탭·페인을 ID로 찾아 대상에는 대시 링을,
//! 대상이 보이지 않으면 가장 가까운 보이는 컨테이너에 move 글리프를 둔다.
//! 포커스나 활성 탭으로 대상을 고르지 않는다. 명세: docs/features/surface-move/index.md.

use egui::emath::GuiRounding as _;
use tasty_type_geometry::length::PhysicalPx;

use crate::core::CoreState;
use crate::model::PhysicalRect;
use crate::state::PendingMove;

/// 활성 워크스페이스에서 표시할 단서. 대상이 다른 워크스페이스에 있으면 [`workspace_cue`]가 대신한다.
#[derive(Clone, Copy, Debug)]
pub(crate) enum MoveSourceMark {
    /// 서피스 rect 또는 페인 rect(탭 바 포함)에 링을 그린다. 그 페인의 탭 바 레이어에 그린다.
    Ring { pane_id: u32, rect: PhysicalRect },
    /// 탭 칸에 링을 그린다.
    TabRing { pane_id: u32, tab_index: usize },
    /// 대상 서피스가 비활성 탭 안에 있어 그 탭 칸에 글리프를 둔다.
    TabGlyph { pane_id: u32, tab_index: usize },
}

/// 슬롯의 대상이 어느 워크스페이스에도 없으면 슬롯을 비운다. 비웠으면 true다.
/// 대상 종류와 관계없이 닫힌 대상의 표시와 "이곳으로 이동" 메뉴가 남지 않게 한다.
pub(crate) fn clear_if_target_closed(
    engine: &CoreState,
    pending_move: &mut Option<PendingMove>,
) -> bool {
    let Some(pending) = *pending_move else {
        return false;
    };
    if workspace_of(engine, pending).is_some() {
        return false;
    }
    *pending_move = None;
    true
}

fn workspace_of(engine: &CoreState, pending: PendingMove) -> Option<usize> {
    match pending {
        PendingMove::Surface(id) => engine.find_workspace_index_for_surface(id).map(|(i, _)| i),
        PendingMove::Pane(id) => engine.find_workspace_index_for_pane(id),
        PendingMove::Tab(id) => {
            let pane_id = engine.find_pane_for_tab(id)?;
            engine.find_workspace_index_for_pane(pane_id)
        }
    }
}

/// 대상이 활성 워크스페이스 밖에 있으면 그 워크스페이스 인덱스를 반환한다.
/// 사이드바 행과 접힌 레일 아바타가 move 글리프를 둔다.
pub(crate) fn workspace_cue(
    engine: &CoreState,
    active_ws: usize,
    pending: Option<PendingMove>,
) -> Option<usize> {
    let ws_idx = workspace_of(engine, pending?)?;
    (ws_idx != active_ws).then_some(ws_idx)
}

/// 슬롯을 표시할 단서로 바꾼다. `pane_rects`는 활성 워크스페이스에서 지금 보이는 페인이다.
/// 대상이 다른 워크스페이스에 있거나 대상 페인이 `pane_rects`에 없으면 None이다.
/// 활성 워크스페이스의 페인은 모두 `pane_rects`에 있으므로 뒤의 경우는 방어용이다.
/// 탭 칸이 탭 바 스크롤 밖에 있거나 rect가 링보다 좁아 보이지 않는 경우는 여기서 거르지 않는다.
#[allow(clippy::too_many_arguments)] // reason: 활성 워크스페이스 레이아웃을 읽어 표시 단서만 돌려주는 순수 질의다 — 호출부 두 곳이 같은 목록을 넘기므로 묶을 여지는 있고, 구조체화는 동작을 건드리는 별도 작업이다
pub(crate) fn resolve(
    pending: Option<PendingMove>,
    presentation: &dyn crate::model::StructurePresentation,
    engine: &CoreState,
    active_ws: usize,
    pane_rects: &[(u32, PhysicalRect)],
    tab_bar_h: PhysicalPx,
    previews: Option<&crate::state::layout_preview::LayoutPreviews>,
    scale_factor: f32,
) -> Option<MoveSourceMark> {
    let pending = pending?;
    if workspace_of(engine, pending)? != active_ws {
        return None;
    }
    let visible = |pane_id: u32| {
        pane_rects
            .iter()
            .find(|(id, _)| *id == pane_id)
            .map(|&(_, r)| r)
    };
    match pending {
        PendingMove::Pane(pane_id) => Some(MoveSourceMark::Ring {
            pane_id,
            rect: visible(pane_id)?,
        }),
        PendingMove::Tab(tab_id) => {
            let pane_id = engine.find_pane_for_tab(tab_id)?;
            visible(pane_id)?;
            let pane = engine.find_pane_by_id(pane_id)?;
            let tab_index = pane.tabs.iter().position(|t| t.id == tab_id)?;
            Some(MoveSourceMark::TabRing { pane_id, tab_index })
        }
        PendingMove::Surface(surface_id) => {
            let pane_id = engine.find_pane_for_surface(surface_id)?;
            let pane_rect = visible(pane_id)?;
            let pane = engine.find_pane_by_id(pane_id)?;
            let tab_index = pane
                .tabs
                .iter()
                .position(|t| t.contains_surface(surface_id))?;
            if tab_index != presentation.tab_index(pane) {
                return Some(MoveSourceMark::TabGlyph { pane_id, tab_index });
            }
            let content = PhysicalRect {
                x: pane_rect.x,
                y: pane_rect.y + tab_bar_h,
                width: pane_rect.width,
                height: (pane_rect.height - tab_bar_h).max(PhysicalPx(1.0)),
            };
            let rect = crate::state::layout_preview::surface_regions(
                previews,
                engine,
                &pane.tabs[tab_index],
                content,
                scale_factor,
            )
            .into_iter()
            .find(|r| r.id == surface_id)?
            .rect;
            Some(MoveSourceMark::Ring { pane_id, rect })
        }
    }
}

/// 서피스·페인 링을 대상 페인의 탭 바 레이어에 그린다. 탭 바·점유 테두리보다 뒤에 호출해
/// 대상 rect에서 가장 마지막에 그린다. 탭 바 레이어는 Area라 팝업보다 아래에 남는다.
pub(crate) fn draw_move_source_ring(
    ctx: &egui::Context,
    mark: Option<MoveSourceMark>,
    scale_factor: f32,
) {
    let Some(MoveSourceMark::Ring { pane_id, rect }) = mark else {
        return;
    };
    let th = crate::theme::theme();
    let painter = ctx.layer_painter(crate::adapters::ui::tab_bar::pane_tab_bar_layer(pane_id));
    let rect = crate::adapters::ui::to_egui_rect(rect, scale_factor).round_ui();
    tasty_ui_widgets::paint_move_source_ring(&painter, &th, rect);
}

#[cfg(test)]
mod tests;
