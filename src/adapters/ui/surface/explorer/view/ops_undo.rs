//! 이름 변경·새 항목을 되돌린 결과의 문구. 항목이 하나라 제목은 수 대신 결과만 말하고, 복사·이동
//! 문구가 맞지 않는 이유는 그 작업에 맞는 문구로 바꾼다.

use tasty_type_appearance::toast_kind::ToastKind;

use super::reason_text;
use crate::app::explorer_files::history::UndoOf;
use crate::app::explorer_files::job::{Reason, Report};
use crate::i18n::t;

/// 이름 변경·새 항목을 되돌린 카드의 제목. 복사·이동이면 None.
pub(super) fn single_title(report: &Report, of: UndoOf) -> Option<(ToastKind, String)> {
    let (done, failed) = match of {
        UndoOf::Rename => (
            "explorer.result.undo_done_rename",
            "explorer.result.undo_failed_rename",
        ),
        UndoOf::NewFolder => (
            "explorer.result.undo_done_new_folder",
            "explorer.result.undo_failed_new_folder",
        ),
        UndoOf::NewFile => (
            "explorer.result.undo_done_new_file",
            "explorer.result.undo_failed_new_file",
        ),
        UndoOf::Copy | UndoOf::Move => return None,
    };
    Some(if report.failed.is_empty() {
        (ToastKind::Success, t(done).to_owned())
    } else {
        (ToastKind::Warning, t(failed).to_owned())
    })
}

/// 되돌리지 못한 이유. `of` 가 이름 변경·새 항목이면 그 작업에 맞춘 문구를 쓴다.
pub(crate) fn reason_for(reason: &Reason, of: Option<UndoOf>) -> String {
    match (of, reason) {
        (Some(UndoOf::Rename), Reason::NewerThere) => t("explorer.result.name_taken").to_owned(),
        (Some(UndoOf::Rename), Reason::ChangedAfterMove) => {
            t("explorer.result.changed_after_rename").to_owned()
        }
        (Some(UndoOf::NewFolder | UndoOf::NewFile), Reason::ChangedSince) => {
            t("explorer.result.changed_after_create").to_owned()
        }
        _ => reason_text(reason),
    }
}
