//! explorer 칸 이력의 되돌리기·다시 실행 요청. 메뉴 행, 단축키, 결과 카드의 Undo 가 함께 쓴다.

use crate::app::explorer_files::Operation;
use crate::app::explorer_files::history::{Entry, Recorded, RedoBlock};
use crate::intent::IntentOrigin;
use crate::runtime::engine_read::EngineRead;

/// 되돌릴 수 없게 된 단계의 이유 문구. 메뉴 행의 툴팁과 단축키의 알림이 같은 문구를 쓴다.
pub(crate) fn stale_text(entry: &Entry) -> Option<String> {
    let reason = entry.stale_reason()?;
    Some(crate::i18n::t_fmt(
        "explorer.menu.undo_stale",
        &crate::explorer_ui::view::ops::reason_text(&reason),
    ))
}

/// 다시 실행할 수 없게 된 단계의 이유 문구. 메뉴 행의 툴팁과 단축키의 알림이 같은 문구를 쓴다.
pub(crate) fn redo_stale_text(entry: &Entry) -> Option<String> {
    let reason = match entry.redo_block()? {
        RedoBlock::Gone => crate::i18n::t("explorer.result.gone"),
        RedoBlock::Changed => crate::i18n::t("explorer.menu.changed_after_undo"),
    };
    Some(crate::i18n::t_fmt("explorer.menu.redo_stale", reason))
}

impl super::MainViewState {
    /// 이 칸에 끝나면 이력에 반영할 작업이 기다리거나 실행 중인가. 그동안 되돌리기·다시 실행은
    /// 맨 위 단계가 아직 정해지지 않았으므로 받지 않는다.
    pub(crate) fn explorer_history_busy(&self, sid: u32) -> bool {
        self.explorer_file_requests.has_history_for(sid)
            || self
                .explorer_views
                .get(sid)
                .and_then(|v| v.ops.running.as_ref())
                .is_some_and(|r| r.in_history)
    }

    /// 칸 이력의 맨 위 단계를 되돌리거나(`redo` 가 거짓) 다시 실행한다. 단계가 없으면 아무 일도
    /// 하지 않는다. 되돌리거나 다시 실행할 수 없게 된 단계는 꺼내지 않고 그 칸에 이유를 알린다. 이력에 반영할
    /// 작업이 남아 있거나, 메뉴(`from_menu`)를 연 뒤 이력이 바뀌었으면 아무것도 꺼내지 않고 알린다.
    pub(crate) fn explorer_history_step(
        &mut self,
        engine: &EngineRead<'_>,
        sid: u32,
        redo: bool,
        origin: IntentOrigin,
        from_menu: bool,
    ) {
        let busy = self.explorer_history_busy(sid);
        let Some(history) = self.explorer_views.get_mut(sid).map(|v| &mut v.ops.history) else {
            return;
        };
        let refused = if busy {
            Some("explorer.menu.history_busy")
        } else if from_menu && !history.still_shown() {
            Some("explorer.menu.history_changed")
        } else {
            None
        };
        if let Some(key) = refused {
            self.toasts.push(
                crate::i18n::t(key).to_owned(),
                crate::adapters::ui::ToastKind::Info,
                crate::adapters::ui::ToastScope::Surface(sid),
            );
            return;
        }
        let top = if redo {
            history.peek_redo()
        } else {
            history.peek_undo()
        };
        let Some(top) = top else {
            return;
        };
        let stale = if redo {
            redo_stale_text(top)
        } else {
            stale_text(top)
        };
        if let Some(why) = stale {
            self.toasts.push(
                why,
                crate::adapters::ui::ToastKind::Info,
                crate::adapters::ui::ToastScope::Surface(sid),
            );
            return;
        }
        if redo {
            let Some(entry) = history.take_redo() else {
                return;
            };
            let operation = entry.source.operation();
            self.request_history(engine, sid, operation, origin, Recorded::Redo(entry));
            return;
        }
        let Some(entry) = history.take_undo() else {
            return;
        };
        self.request_history_undo(engine, sid, entry, origin);
    }

    /// 이력의 단계 하나를 되돌린다. 끝까지 되면 다시 실행할 수 있다.
    pub(crate) fn request_history_undo(
        &mut self,
        engine: &EngineRead<'_>,
        sid: u32,
        entry: Entry,
        origin: IntentOrigin,
    ) {
        let operation = Operation::Undo(entry.undo.clone());
        self.request_history(engine, sid, operation, origin, Recorded::Undo(entry));
    }

    fn request_history(
        &mut self,
        engine: &EngineRead<'_>,
        sid: u32,
        operation: Operation,
        origin: IntentOrigin,
        recorded: Recorded,
    ) {
        if let Some(recorded) =
            self.request_explorer_history(engine, sid, operation, origin, recorded)
            && let Some(view) = self.explorer_views.get_mut(sid)
        {
            view.ops.history.restore(recorded);
        }
    }
}
