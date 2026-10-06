//! debug 전용 native 메뉴 응답과 이동 대기 조회. release에는 없다.
//! 다음에 열리는 native 메뉴를 OS 팝업 없이 지정한 항목을 고른 것으로 끝낸다. 메뉴 항목을 만드는 일과
//! 선택 뒤 처리는 사용자 우클릭과 같은 경로를 쓴다. 메뉴를 열려면 `debug.inject_egui_mouse` 우클릭을 함께 쓴다.

#![cfg(debug_assertions)]

use crate::platform::native_menu::MenuItem;
use crate::state::PendingMove;

use super::MainView;

/// `open_native_menu`에서 둔 응답이 있으면 그 선택으로 끝내고 돌아가며, 없으면 `cont`를 되돌려 받는다.
/// 동결 파일(`redraw.rs`)에 남는 debug 줄을 한 문장으로 줄이려고 매크로로 둔다.
macro_rules! answer_or_return {
    ($view:expr, $engine:expr, $items:expr, $cont:ident) => {
        let Err($cont) = $view.debug_answer_menu($engine, $items, $cont) else {
            return;
        };
    };
}
pub(super) use answer_or_return;

/// 다음 native 메뉴에 줄 응답.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MenuAnswer {
    /// 항목 id로 고른다.
    Item(u32),
    /// 표시 문구가 같은 항목을 고른다.
    Label(String),
    /// 항목을 고르지 않고 닫는다.
    Dismiss,
}

impl MainView {
    /// 다음 native 메뉴 하나에 쓸 응답을 둔다. 앞서 둔 응답이 남아 있으면 덮어쓴다.
    pub(crate) fn debug_set_menu_answer(&mut self, answer: MenuAnswer) {
        self.debug_menu_answer = Some(answer);
    }

    /// 둔 응답이 있으면 OS 메뉴 대신 그 선택으로 `cont`를 실행한다(macOS·Windows의 즉시 완료와 같은 순서).
    /// 응답이 없으면 `cont`를 돌려줘 호출부가 실제 메뉴를 연다.
    pub(super) fn debug_answer_menu<F>(
        &mut self,
        engine: &crate::runtime::engine_read::EngineRead<'_>,
        items: &[MenuItem],
        cont: F,
    ) -> Result<(), F>
    where
        F: FnOnce(&mut MainView, &crate::runtime::engine_read::EngineRead<'_>, Option<u32>),
    {
        let Some(answer) = self.debug_menu_answer.take() else {
            return Err(cont);
        };
        let result = resolve_answer(&answer, items);
        if result.is_none() && answer != MenuAnswer::Dismiss {
            tracing::warn!("debug menu answer {answer:?} matched no enabled item; menu dismissed");
        }
        cont(self, engine, result);
        crate::view::ui::View::mark_dirty(self);
        Ok(())
    }

    /// 이 창의 이동 대기 슬롯.
    pub(crate) fn debug_pending_move(&self) -> serde_json::Value {
        pending_move_json(self.state.pending_move)
    }
}

/// 사용자가 고를 수 있는 항목만 고른다. 구분선과 비활성 항목은 고를 수 없다.
fn resolve_answer(answer: &MenuAnswer, items: &[MenuItem]) -> Option<u32> {
    let pickable = |item: &&MenuItem| item.enabled && !item.is_separator();
    match answer {
        MenuAnswer::Item(id) => items.iter().filter(pickable).find(|i| i.id == *id),
        MenuAnswer::Label(label) => items.iter().filter(pickable).find(|i| i.label == *label),
        MenuAnswer::Dismiss => None,
    }
    .map(|item| item.id)
}

fn pending_move_json(pending: Option<PendingMove>) -> serde_json::Value {
    let (kind, id) = match pending {
        None => return serde_json::json!({ "present": false }),
        Some(PendingMove::Surface(id)) => ("surface", id),
        Some(PendingMove::Tab(id)) => ("tab", id),
        Some(PendingMove::Pane(id)) => ("pane", id),
    };
    serde_json::json!({ "present": true, "kind": kind, "id": id })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<MenuItem> {
        vec![
            MenuItem::new(1, "Close Tab"),
            MenuItem::separator(),
            MenuItem::new(7, "Move Tab"),
            MenuItem::disabled(9, "Move Pane"),
        ]
    }

    #[test]
    fn an_answer_picks_an_enabled_item_by_id_or_label() {
        assert_eq!(resolve_answer(&MenuAnswer::Item(7), &items()), Some(7));
        assert_eq!(
            resolve_answer(&MenuAnswer::Label("Move Tab".into()), &items()),
            Some(7)
        );
    }

    #[test]
    fn an_answer_never_picks_what_the_user_cannot_pick() {
        assert_eq!(resolve_answer(&MenuAnswer::Item(9), &items()), None);
        assert_eq!(resolve_answer(&MenuAnswer::Item(0), &items()), None);
        assert_eq!(resolve_answer(&MenuAnswer::Item(42), &items()), None);
        assert_eq!(
            resolve_answer(&MenuAnswer::Label(String::new()), &items()),
            None
        );
        assert_eq!(resolve_answer(&MenuAnswer::Dismiss, &items()), None);
    }

    #[test]
    fn the_pending_move_reads_back_its_kind_and_id() {
        assert_eq!(
            pending_move_json(None),
            serde_json::json!({ "present": false })
        );
        assert_eq!(
            pending_move_json(Some(PendingMove::Tab(5))),
            serde_json::json!({ "present": true, "kind": "tab", "id": 5 })
        );
        assert_eq!(
            pending_move_json(Some(PendingMove::Pane(3)))["kind"],
            "pane"
        );
        assert_eq!(
            pending_move_json(Some(PendingMove::Surface(9)))["kind"],
            "surface"
        );
    }
}
