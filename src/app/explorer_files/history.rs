//! explorer 칸 하나의 되돌리기·다시 실행 이력. 끝까지 된 복사·이동만 쌓고, 칸마다 최근
//! [`MAX_STEPS`] 단계를 둔다. 목록 UI 는 없고 맨 위 단계만 메뉴 행과 단축키로 쓴다.
//!
//! 다시 실행은 되돌린 작업의 원래 요청(원본 경로·목적지·이동 여부)을 새 작업으로 다시 보낸다.
//! 이름이 겹치면 처음처럼 충돌 질문을 거친다.

use std::collections::VecDeque;
use std::path::PathBuf;

use super::Operation;
use super::job::{OpKind, Reason, Report, UndoStep};

/// 칸 하나가 기억하는 단계 수.
pub(crate) const MAX_STEPS: usize = 10;
/// 되돌릴 수 없게 됐는지 메뉴를 열 때 확인하는 단계의 최대 수. 이보다 많으면 확인하지 않은
/// 단계가 되돌려질 수 있다고 보고 행을 켜 둔다(되돌리기 결과 카드가 항목별 이유를 보인다).
const STALE_SCAN_LIMIT: usize = 256;

/// 다시 실행할 원래 요청.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Source {
    pub(crate) paths: Vec<PathBuf>,
    pub(crate) destination: PathBuf,
    pub(crate) cut: bool,
}

impl Source {
    pub(crate) fn of(operation: &Operation) -> Option<Self> {
        match operation {
            Operation::Paste {
                paths,
                destination,
                cut,
            } => Some(Self {
                paths: paths.clone(),
                destination: destination.clone(),
                cut: *cut,
            }),
            _ => None,
        }
    }
    pub(crate) fn operation(&self) -> Operation {
        Operation::Paste {
            paths: self.paths.clone(),
            destination: self.destination.clone(),
            cut: self.cut,
        }
    }
}

/// 이력의 한 단계: 끝난 복사·이동과 그것을 되돌리는 방법.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) source: Source,
    pub(crate) undo: Vec<UndoStep>,
}

impl Entry {
    pub(crate) fn kind(&self) -> OpKind {
        if self.source.cut {
            OpKind::Move
        } else {
            OpKind::Copy
        }
    }
    /// 메뉴 행에 적는 항목 수.
    pub(crate) fn count(&self) -> usize {
        self.undo.len()
    }
    /// 되돌릴 단계가 하나도 남지 않았으면 그 이유. 하나라도 되돌릴 수 있으면 None 이다.
    /// 판정은 [`super::job::run_undo`] 가 단계마다 내리는 판정과 같다.
    pub(crate) fn stale_reason(&self) -> Option<Reason> {
        if self.undo.len() > STALE_SCAN_LIMIT {
            return None;
        }
        let mut first = None;
        for step in self.undo.iter().rev() {
            match blocked(step) {
                None => return None,
                Some(reason) => {
                    first.get_or_insert(reason);
                }
            }
        }
        first
    }
}

/// 이 단계를 지금 되돌릴 수 없는 이유. 되돌릴 수 있으면 None.
fn blocked(step: &UndoStep) -> Option<Reason> {
    match step {
        UndoStep::Created(path, made) => match path.symlink_metadata() {
            Err(_) => Some(Reason::Gone),
            Ok(meta) if meta.modified().ok() != *made => Some(Reason::ChangedSince),
            Ok(_) => None,
        },
        UndoStep::Moved { from, to, made } => match to.symlink_metadata() {
            Err(_) => Some(Reason::Gone),
            Ok(_) if from.symlink_metadata().is_ok() => Some(Reason::NewerThere),
            Ok(meta) if meta.modified().ok() != *made => Some(Reason::ChangedAfterMove),
            Ok(_) => None,
        },
        UndoStep::Replaced(_) => Some(Reason::Replaced),
    }
}

/// 작업이 끝났을 때 이력에 반영할 일. 요청과 함께 작업을 따라간다.
#[derive(Debug)]
pub(crate) enum Recorded {
    /// 사용자가 새로 한 복사·이동. 끝까지 되면 되돌리기 맨 위에 쌓는다. 무엇이든 바꿨으면
    /// 다시 실행 목록은 낡았으므로 비운다.
    New(Source),
    /// 다시 실행. 끝까지 되면 되돌리기 맨 위에 쌓고 남은 다시 실행 단계는 그대로 둔다.
    Redo(Entry),
    /// 이력의 한 단계를 되돌린다. 끝까지 되면 다시 실행 목록에 올린다.
    Undo(Entry),
}

/// 칸 하나의 이력.
#[derive(Debug, Default)]
pub(crate) struct History {
    undo: VecDeque<Entry>,
    redo: Vec<Entry>,
    /// 이력이 바뀔 때마다 늘어나는 번호.
    generation: u64,
    /// 메뉴 행을 만들 때의 번호. 고른 행이 그 사이 바뀐 이력을 실행하지 않게 한다.
    shown: Option<u64>,
}

/// 결과 카드가 Undo 를 보이는 조건과 같다: 실패·건너뜀·취소 없이 끝난 복사·이동.
fn completed(report: &Report) -> bool {
    matches!(report.kind, OpKind::Copy | OpKind::Move)
        && !report.cancelled
        && report.failed.is_empty()
        && report.skipped.is_empty()
        && !report.undo.is_empty()
}

impl History {
    pub(crate) fn peek_undo(&self) -> Option<&Entry> {
        self.undo.back()
    }
    pub(crate) fn peek_redo(&self) -> Option<&Entry> {
        self.redo.last()
    }
    pub(crate) fn take_undo(&mut self) -> Option<Entry> {
        let entry = self.undo.pop_back()?;
        self.generation += 1;
        Some(entry)
    }
    pub(crate) fn take_redo(&mut self) -> Option<Entry> {
        let entry = self.redo.pop()?;
        self.generation += 1;
        Some(entry)
    }
    /// 메뉴 행을 지금 이력으로 만든다고 적어 둔다.
    pub(crate) fn show(&mut self) {
        self.shown = Some(self.generation);
    }
    /// 마지막으로 만든 메뉴 행이 지금 이력과 같은가.
    pub(crate) fn still_shown(&self) -> bool {
        self.shown == Some(self.generation)
    }
    /// 꺼낸 단계를 요청하지 못했을 때 제자리에 돌려놓는다.
    pub(crate) fn restore(&mut self, recorded: Recorded) {
        self.generation += 1;
        match recorded {
            Recorded::Undo(entry) => self.push_undo(entry),
            Recorded::Redo(entry) => self.redo.push(entry),
            Recorded::New(_) => {}
        }
    }
    /// 결과 카드의 Undo 가 되돌리는 단계를 이력에서 뺀다. 이력에 없으면(오래돼 밀려났으면) None.
    pub(crate) fn take_matching(&mut self, steps: &[UndoStep]) -> Option<Entry> {
        let at = self.undo.iter().rposition(|e| e.undo == steps)?;
        self.generation += 1;
        self.undo.remove(at)
    }
    fn push_undo(&mut self, entry: Entry) {
        if self.undo.len() == MAX_STEPS {
            self.undo.pop_front();
        }
        self.undo.push_back(entry);
    }
    /// 끝난 작업을 이력에 반영한다.
    pub(crate) fn record(&mut self, recorded: Recorded, report: &Report) {
        self.generation += 1;
        match recorded {
            Recorded::New(source) => {
                if report.done > 0 {
                    self.redo.clear();
                }
                if completed(report) {
                    self.push_undo(Entry {
                        source,
                        undo: report.undo.clone(),
                    });
                }
            }
            Recorded::Redo(entry) => {
                if completed(report) {
                    self.push_undo(Entry {
                        source: entry.source,
                        undo: report.undo.clone(),
                    });
                }
            }
            Recorded::Undo(entry) => {
                let undone = !report.cancelled
                    && report.failed.is_empty()
                    && report.skipped.is_empty()
                    && report.done > 0;
                if undone {
                    if self.redo.len() == MAX_STEPS {
                        self.redo.remove(0);
                    }
                    self.redo.push(entry);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
