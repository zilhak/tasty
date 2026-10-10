//! explorer 빈 영역 메뉴의 Undo · Redo 행. 칸의 이력에서 맨 위 단계만 보인다.
//! 되돌릴 수 없게 된 단계는 행을 끄고 이유를 툴팁으로 보인다. 목록 UI 는 없다.

use super::MainView;
use crate::app::explorer_files::history::Entry;
use crate::app::explorer_files::job::OpKind;
use crate::platform::native_menu::MenuItem;
use crate::runtime::engine_read::EngineRead;
use crate::state::explorer_history::{redo_stale_text, stale_text};

const UNDO: u32 = 110;
const REDO: u32 = 111;

/// 메뉴 행의 작업 이름(예: "move 3 items").
fn op_label(entry: &Entry) -> String {
    let key = match entry.kind() {
        OpKind::Move => "explorer.menu.op_move",
        _ => "explorer.menu.op_copy",
    };
    let n = entry.count();
    crate::i18n::t_count(key, n as u64, &[&n.to_string()])
}

pub(crate) fn undo_label(entry: &Entry) -> String {
    crate::i18n::t_fmt("explorer.menu.undo", &op_label(entry))
}

pub(crate) fn redo_label(entry: &Entry) -> String {
    crate::i18n::t_fmt("explorer.menu.redo", &op_label(entry))
}

/// 이력의 맨 위 단계로 만든 Undo · Redo 행. 둘 다 없으면 빈 목록이다. 되돌리거나 다시 실행할 수
/// 없게 된 단계는 행을 끄고 이유를 툴팁으로 보인다. `busy` 면 이력에 반영할
/// 작업이 남아 있다는 뜻이라 두 행을 모두 끄고 그 이유를 툴팁으로 보인다.
pub(crate) fn history_items(
    undo: Option<&Entry>,
    redo: Option<&Entry>,
    busy: bool,
) -> Vec<MenuItem> {
    let busy_text = || crate::i18n::t("explorer.menu.history_busy").to_owned();
    let mut items = Vec::new();
    if let Some(entry) = undo {
        let why = if busy {
            Some(busy_text())
        } else {
            stale_text(entry)
        };
        items.push(match why {
            Some(why) => MenuItem::disabled(UNDO, undo_label(entry)).with_tooltip(why),
            None => MenuItem::new(UNDO, undo_label(entry)),
        });
    }
    if let Some(entry) = redo {
        let why = if busy {
            Some(busy_text())
        } else {
            redo_stale_text(entry)
        };
        items.push(match why {
            Some(why) => MenuItem::disabled(REDO, redo_label(entry)).with_tooltip(why),
            None => MenuItem::new(REDO, redo_label(entry)),
        });
    }
    items
}

/// 칸 이력으로 메뉴 행을 만들고, 고른 행이 이 이력을 실행하도록 지금 이력을 적어 둔다.
pub(super) fn history_rows(state: &mut crate::state::MainViewState, sid: u32) -> Vec<MenuItem> {
    let busy = state.explorer_history_busy(sid);
    let Some(history) = state
        .explorer_views
        .get_mut(sid)
        .map(|v| &mut v.ops.history)
    else {
        return Vec::new();
    };
    history.show();
    history_items(history.peek_undo(), history.peek_redo(), busy)
}

/// 메뉴는 늘 "구분선 · Properties" 로 끝난다. 그 구분선 앞에 구분선과 행 묶음을 넣어
/// 묶음이 앞뒤 구분선 사이에 놓이게 한다.
fn insert_rows(items: &mut Vec<MenuItem>, rows: Vec<MenuItem>) {
    if rows.is_empty() {
        return;
    }
    let at = items.len().saturating_sub(2);
    items.splice(at..at, std::iter::once(MenuItem::separator()).chain(rows));
}

impl MainView {
    /// 빈 영역 메뉴의 Properties 묶음 앞에 Undo · Redo 묶음을 넣는다. `shown` 이 거짓이면
    /// (항목 메뉴, mirror explorer) 넣지 않는다.
    pub(super) fn push_explorer_history_items(
        &mut self,
        surface_id: u32,
        shown: bool,
        items: &mut Vec<MenuItem>,
    ) {
        if shown {
            insert_rows(items, history_rows(&mut self.state, surface_id));
        }
    }

    /// 생성 행과 이력 행을 처리한다. 둘 다 아니면 아무것도 하지 않는다.
    pub(super) fn explorer_menu_other(
        &mut self,
        engine: &EngineRead<'_>,
        surface_id: u32,
        id: u32,
        paths: &[std::path::PathBuf],
        cwd: &std::path::Path,
    ) {
        match id {
            UNDO | REDO => {
                let origin = crate::intent::IntentOrigin::User {
                    source: crate::intent::UserSource::ContextMenu,
                };
                self.state
                    .explorer_history_step(engine, surface_id, id == REDO, origin, true);
            }
            _ => self.explorer_menu_create(engine, surface_id, id, paths, cwd),
        }
    }
}

#[cfg(test)]
#[path = "explorer_history_tests.rs"]
mod tests;
