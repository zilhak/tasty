//! explorer 파일 작업 표시에서 고른 일을 요청으로 바꾼다. 실행은 App 의 파일 작업 대기열이 맡는다.

use crate::app::explorer_files::Operation;
use crate::app::explorer_files::job::OpKind;
use crate::explorer_ui::view::ops::OpsAction;
use crate::intent::{IntentOrigin, UserSource};
use crate::runtime::engine_read::EngineRead;

/// 다시 시도하거나 끌어 놓은 작업. 목적지를 모르는 복사·이동이나 되돌리기는 다시 시도하지 않는다.
fn retry_operation(
    kind: OpKind,
    paths: Vec<std::path::PathBuf>,
    dest: Option<std::path::PathBuf>,
) -> Option<Operation> {
    match (kind, dest) {
        (OpKind::Copy, Some(destination)) => Some(Operation::Paste {
            paths,
            destination,
            cut: false,
        }),
        // 원본 지우기에서 사본이 없거나 원본이 바뀌어 남긴 항목은 이동 자체를 다시 요청한다.
        (OpKind::Move | OpKind::RemoveOriginals, Some(destination)) => Some(Operation::Paste {
            paths,
            destination,
            cut: true,
        }),
        (OpKind::Trash, _) => Some(Operation::Trash(paths)),
        _ => None,
    }
}

impl super::MainViewState {
    /// 사용자가 진행 표시·결과 카드에서 고른 일을 처리한다.
    pub(crate) fn apply_explorer_ops(&mut self, engine: &EngineRead<'_>, sid: u32, op: OpsAction) {
        let origin = IntentOrigin::User {
            source: UserSource::Menu("explorer_ops"),
        };
        match op {
            OpsAction::RemoveQueued(id) => {
                self.explorer_file_requests.remove(id);
                if let Some(view) = self.explorer_views.get_mut(sid) {
                    view.ops.queued.retain(|q| q.id != id);
                }
            }
            OpsAction::Retry {
                kind,
                paths,
                dest,
                leftovers,
            } => {
                // 원본이 남은 이동 항목은 다시 옮기지 않고 원본 삭제만 다시 한다. 결과 카드는 따로 뜬다.
                if !leftovers.is_empty() {
                    let operation = Operation::RemoveLeftovers {
                        dest: dest.clone(),
                        leftovers,
                    };
                    self.request_explorer_file_direct(engine, sid, operation, origin.clone());
                }
                if !paths.is_empty()
                    && let Some(operation) = retry_operation(kind, paths, dest)
                {
                    self.request_explorer_file_direct(engine, sid, operation, origin);
                }
            }
            OpsAction::Undo(steps) => {
                self.request_explorer_file_direct(engine, sid, Operation::Undo(steps), origin);
            }
            OpsAction::Drop { kind, paths, dest } => {
                let origin = IntentOrigin::User {
                    source: UserSource::Menu("explorer_drop"),
                };
                if let Some(operation) = retry_operation(kind, paths, Some(dest)) {
                    self.request_explorer_file_direct(engine, sid, operation, origin);
                }
            }
            OpsAction::ShowConflict => {
                crate::app::explorer_files::open_conflict(self, sid, "explorer_conflict_show");
            }
        }
    }
}
