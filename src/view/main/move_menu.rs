//! 우클릭 메뉴의 "이동" / "이곳으로 이동" 2단계 조작(서피스·탭). 대기 슬롯은 종류를 담는 하나뿐이다.
//! 사용자 우클릭 조작이라 GUI에서만 연다. 명세: docs/features/surface-move/index.md.

use super::MainView;
use crate::core::state::PendingMove;
use crate::platform::native_menu::MenuItem;

/// surface 메뉴 항목 id. 같은 메뉴의 다른 항목 id와 겹치지 않는다.
pub(super) const ITEM_MOVE_SURFACE: u32 = 10;
pub(super) const ITEM_MOVE_SURFACE_HERE: u32 = 11;

/// 탭 메뉴 항목 id. 탭 메뉴의 기존 항목(1~6)과 겹치지 않는다.
const ITEM_MOVE_TAB: u32 = 7;
const ITEM_MOVE_TAB_HERE: u32 = 8;

impl MainView {
    /// "서피스 이동"과, 서피스가 대기 중일 때만 "서피스를 이곳으로 이동"을 붙인다.
    pub(super) fn push_surface_move_items(&self, items: &mut Vec<MenuItem>) {
        items.push(MenuItem::new(
            ITEM_MOVE_SURFACE,
            crate::i18n::t("surface_context_menu.move"),
        ));
        if matches!(self.core_state.pending_move, Some(PendingMove::Surface(_))) {
            items.push(MenuItem::new(
                ITEM_MOVE_SURFACE_HERE,
                crate::i18n::t("surface_context_menu.move_here"),
            ));
        }
    }

    /// surface 메뉴의 이동 항목을 처리한다. 이동 항목이 아니면 false를 반환한다.
    pub(super) fn apply_surface_move_selection(&mut self, surface_id: u32, item: u32) -> bool {
        match item {
            ITEM_MOVE_SURFACE => {
                // 도메인 구조는 바꾸지 않고 대기 슬롯만 덮어쓴다.
                self.core_state.pending_move = Some(PendingMove::Surface(surface_id));
                self.state.toasts.push_info(
                    crate::i18n::t("toast.surface_cut"),
                    crate::adapters::ui::ToastScope::Surface(surface_id),
                );
                true
            }
            ITEM_MOVE_SURFACE_HERE => {
                if let Some(PendingMove::Surface(source)) = self.core_state.pending_move {
                    self.core_state.pending_move = None;
                    self.state.dispatch_intent(
                        crate::core::intent::DomainIntent::MoveSurface {
                            source_surface_id: source,
                            target_surface_id: surface_id,
                        }
                        .from_user_context_menu(),
                    );
                }
                true
            }
            _ => false,
        }
    }

    /// 탭 메뉴 끝에 구분선과 "탭 이동"을 붙인다. 다른 탭이 대기 중일 때만 "탭을 이곳으로 이동"도 붙인다.
    pub(super) fn push_tab_move_items(&self, items: &mut Vec<MenuItem>, tab_id: u32) {
        items.push(MenuItem::separator());
        items.push(MenuItem::new(
            ITEM_MOVE_TAB,
            crate::i18n::t("tab_context_menu.move_tab"),
        ));
        if matches!(self.core_state.pending_move, Some(PendingMove::Tab(id)) if id != tab_id) {
            items.push(MenuItem::new(
                ITEM_MOVE_TAB_HERE,
                crate::i18n::t("tab_context_menu.move_tab_here"),
            ));
        }
    }

    /// 탭 메뉴의 이동 항목을 처리한다. 이동 항목이 아니면 false를 반환한다.
    /// 메뉴가 열린 동안 탭이 닫혔으면 아무것도 하지 않는다.
    pub(super) fn apply_tab_move_selection(&mut self, tab_id: u32, item: u32) -> bool {
        match item {
            ITEM_MOVE_TAB => {
                if let Some(pane_id) = self.core_state.find_pane_for_tab(tab_id) {
                    self.core_state.pending_move = Some(PendingMove::Tab(tab_id));
                    self.state.toasts.push_info(
                        crate::i18n::t("toast.tab_cut"),
                        crate::adapters::ui::ToastScope::Pane(pane_id),
                    );
                }
                true
            }
            ITEM_MOVE_TAB_HERE => {
                if self.core_state.find_pane_for_tab(tab_id).is_some()
                    && let Some(PendingMove::Tab(source)) = self.core_state.pending_move
                {
                    self.core_state.pending_move = None;
                    self.state.dispatch_intent(
                        crate::core::intent::DomainIntent::ReplaceTabWithTab {
                            source_tab_id: source,
                            target_tab_id: tab_id,
                        }
                        .from_user_context_menu(),
                    );
                }
                true
            }
            _ => false,
        }
    }
}
