//! explorer 칸 하나의 되돌리기·다시 실행 이력. 복사·이동·이름 변경·새 항목을 쌓고, 칸마다 최근
//! [`MAX_STEPS`] 단계를 둔다. 일부만 된 복사·이동은 된 항목만 쌓는다. 목록 UI 는 없고 맨 위 단계만
//! 메뉴 행과 단축키로 쓴다.
//!
//! 다시 실행은 되돌린 작업의 원래 요청을 새 작업으로 다시 보낸다. 붙여넣기는 이름이 겹치면 처음처럼
//! 충돌 질문을 거치고, 이름 변경·새 항목은 같은 이름이 있으면 덮어쓰지 않고 실패한다.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::Operation;
use super::job::{OpKind, Reason, Report, UndoStep};

/// 칸 하나가 기억하는 단계 수.
pub(crate) const MAX_STEPS: usize = 10;
/// 되돌릴 수 없게 됐는지 메뉴를 열 때 확인하는 단계의 최대 수. 이보다 많으면 확인하지 않은
/// 단계가 되돌려질 수 있다고 보고 행을 켜 둔다(되돌리기 결과 카드가 항목별 이유를 보인다).
const STALE_SCAN_LIMIT: usize = 256;

/// 다시 실행할 원래 요청.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Source {
    Paste {
        paths: Vec<PathBuf>,
        destination: PathBuf,
        cut: bool,
    },
    Rename {
        path: PathBuf,
        name: String,
    },
    Create {
        dir: PathBuf,
        name: String,
        folder: bool,
    },
}

impl Source {
    pub(crate) fn of(operation: &Operation) -> Option<Self> {
        match operation {
            Operation::Paste {
                paths,
                destination,
                cut,
            } => Some(Self::Paste {
                paths: paths.clone(),
                destination: destination.clone(),
                cut: *cut,
            }),
            Operation::Rename { path, name } => Some(Self::Rename {
                path: path.clone(),
                name: name.clone(),
            }),
            Operation::Create { dir, name, folder } => Some(Self::Create {
                dir: dir.clone(),
                name: name.clone(),
                folder: *folder,
            }),
            _ => None,
        }
    }
    pub(crate) fn operation(&self) -> Operation {
        match self {
            Self::Paste {
                paths,
                destination,
                cut,
            } => Operation::Paste {
                paths: paths.clone(),
                destination: destination.clone(),
                cut: *cut,
            },
            Self::Rename { path, name } => Operation::Rename {
                path: path.clone(),
                name: name.clone(),
            },
            Self::Create { dir, name, folder } => Operation::Create {
                dir: dir.clone(),
                name: name.clone(),
                folder: *folder,
            },
        }
    }
    /// 되돌리기가 끝났을 때 모습을 적어 두고 다시 실행 전에 비교할 경로. 붙여넣기와 이름 변경은
    /// 원래 항목, 새 항목은 비어 있어야 할 그 이름이다.
    fn watched(&self) -> Vec<PathBuf> {
        match self {
            Self::Paste { paths, .. } => paths.clone(),
            Self::Rename { path, .. } => vec![path.clone()],
            Self::Create { dir, name, .. } => vec![dir.join(name)],
        }
    }
    /// 일부만 된 붙여넣기에서 된 항목만 남긴다. 실패·건너뜀·취소로 하지 않은 원본은 다시 실행에서 뺀다.
    fn kept(self, report: &Report) -> Self {
        let Self::Paste {
            paths,
            destination,
            cut,
        } = self
        else {
            return self;
        };
        let missed = |p: &PathBuf| {
            report.failed.iter().any(|f| &f.path == p)
                || report.skipped.contains(p)
                || report.unprocessed.contains(p)
        };
        Self::Paste {
            paths: paths.into_iter().filter(|p| !missed(p)).collect(),
            destination,
            cut,
        }
    }
    /// 되돌리기 결과 카드와 메뉴 행에 쓰는 작업 종류.
    pub(crate) fn undo_of(&self) -> UndoOf {
        match self {
            Self::Paste { cut: true, .. } => UndoOf::Move,
            Self::Paste { cut: false, .. } => UndoOf::Copy,
            Self::Rename { .. } => UndoOf::Rename,
            Self::Create { folder: true, .. } => UndoOf::NewFolder,
            Self::Create { folder: false, .. } => UndoOf::NewFile,
        }
    }
}

/// 되돌리는 작업의 종류. 결과 카드의 제목과 메뉴 행의 이름을 정한다.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum UndoOf {
    Copy,
    Move,
    Rename,
    NewFolder,
    NewFile,
}

/// 이력의 한 단계: 끝난 복사·이동·이름 변경·새 항목과 그것을 되돌리는 방법.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Entry {
    pub(crate) source: Source,
    pub(crate) undo: Vec<UndoStep>,
    /// 되돌리기가 끝났을 때 원본 경로마다 본 모습. 다시 실행 전에 바깥에서 바뀌었는지 비교한다.
    seen: Vec<(PathBuf, Option<Look>)>,
    /// 되돌리려고 꺼냈을 때 [`History::cleared`] 값. 되돌리기가 끝날 때 값이 다르면 그 사이 새 작업이
    /// 다시 실행 목록을 비운 것이라 이 단계도 올리지 않는다.
    taken_at: u64,
}

/// 경로에 있는 항목의 종류와 수정 시각.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Look {
    kind: LookKind,
    modified: Option<SystemTime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LookKind {
    File,
    Dir,
    Link,
}

fn look(path: &Path) -> Option<Look> {
    let meta = path.symlink_metadata().ok()?;
    let kind = if meta.file_type().is_symlink() {
        LookKind::Link
    } else if meta.is_dir() {
        LookKind::Dir
    } else {
        LookKind::File
    };
    Some(Look {
        kind,
        modified: meta.modified().ok(),
    })
}

/// 다시 실행할 수 없는 이유.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RedoBlock {
    /// 원본 자리의 항목이 없어졌다.
    Gone,
    /// 되돌린 뒤 원본 자리의 항목이 바뀌었다(종류나 수정 시각).
    Changed,
}

impl Entry {
    pub(crate) fn new(source: Source, undo: Vec<UndoStep>) -> Self {
        Self {
            source,
            undo,
            seen: Vec::new(),
            taken_at: 0,
        }
    }
    /// 다시 실행하면 되돌린 뒤 바깥에서 바뀐 항목을 옮기거나 복사하게 되는가. 바뀐 원본이 하나라도
    /// 있으면 그 이유. 다시 실행은 붙여넣기를 다시 보내 항목별로 거르지 않으므로 하나만 바뀌어도 막는다.
    pub(crate) fn redo_block(&self) -> Option<RedoBlock> {
        self.seen.iter().find_map(|(path, then)| match look(path) {
            now if now == *then => None,
            None => Some(RedoBlock::Gone),
            Some(_) => Some(RedoBlock::Changed),
        })
    }
    pub(crate) fn undo_of(&self) -> UndoOf {
        self.source.undo_of()
    }
    /// 끝난 복사·이동에서 되돌릴 단계. 일부만 됐으면 된 항목만 담는다. 된 항목이 없으면 None.
    fn done(source: Source, report: &Report) -> Option<Self> {
        (matches!(report.kind, OpKind::Copy | OpKind::Move) && !report.undo.is_empty())
            .then(|| Self::new(source.kept(report), report.undo.clone()))
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

/// 이 단계를 지금 되돌릴 수 없는 이유. 되돌릴 수 있으면 None. 메뉴 행과 되돌리기 작업이 함께 쓴다.
pub(super) fn blocked(step: &UndoStep) -> Option<Reason> {
    match step {
        UndoStep::Created(path, made) => match path.symlink_metadata() {
            Err(_) => Some(Reason::Gone),
            Ok(meta) if meta.modified().ok() != *made => Some(Reason::ChangedSince),
            Ok(_) => None,
        },
        UndoStep::Made(path, made) => super::named::made_blocked(path, *made),
        UndoStep::Moved { from, to, made } => match to.symlink_metadata() {
            Err(_) => Some(Reason::Gone),
            Ok(_) if super::named::taken(from, to) => Some(Reason::NewerThere),
            Ok(meta) if meta.modified().ok() != *made => Some(Reason::ChangedAfterMove),
            Ok(_) => None,
        },
        UndoStep::Replaced(_) => Some(Reason::Replaced),
    }
}

/// 작업이 끝났을 때 이력에 반영할 일. 요청과 함께 작업을 따라간다.
#[derive(Debug)]
pub(crate) enum Recorded {
    /// 사용자가 새로 한 작업. 된 항목이 있으면 그 항목만 되돌리기 맨 위에 쌓는다. 무엇이든 바꿨으면
    /// 다시 실행 목록은 낡았으므로 비운다.
    New(Source),
    /// 다시 실행. 된 항목이 있으면 되돌리기 맨 위에 쌓고 남은 다시 실행 단계는 그대로 둔다.
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
    /// 새 작업이 다시 실행 목록을 비운 횟수.
    cleared: u64,
}

impl History {
    pub(crate) fn peek_undo(&self) -> Option<&Entry> {
        self.undo.back()
    }
    pub(crate) fn peek_redo(&self) -> Option<&Entry> {
        self.redo.last()
    }
    pub(crate) fn take_undo(&mut self) -> Option<Entry> {
        let mut entry = self.undo.pop_back()?;
        self.generation += 1;
        entry.taken_at = self.cleared;
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
        let mut entry = self.undo.remove(at)?;
        entry.taken_at = self.cleared;
        Some(entry)
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
                    self.clear_redo();
                }
                if let Some(entry) = Entry::done(source, report) {
                    self.push_undo(entry);
                }
            }
            Recorded::Redo(entry) => {
                if let Some(entry) = Entry::done(entry.source, report) {
                    self.push_undo(entry);
                }
            }
            Recorded::Undo(entry) => {
                // 결과 카드의 Undo 는 대기 중인 새 작업 뒤에 들어갈 수 있다. 그 작업이 먼저 끝나 다시 실행
                // 목록을 비웠다면, 더 오래된 이 단계를 올리면 시간 순서가 뒤바뀐다.
                let undone = entry.taken_at == self.cleared
                    && !report.cancelled
                    && report.failed.is_empty()
                    && report.skipped.is_empty()
                    && report.done > 0;
                if undone {
                    if self.redo.len() == MAX_STEPS {
                        self.redo.remove(0);
                    }
                    let mut entry = entry;
                    entry.seen = entry
                        .source
                        .watched()
                        .into_iter()
                        .map(|p| {
                            let now = look(&p);
                            (p, now)
                        })
                        .collect();
                    self.redo.push(entry);
                }
            }
        }
    }
    /// 끝난 이름 변경·새 항목을 이력에 반영한다. `step` 은 성공해 무엇이 바뀌었을 때 그것을 되돌리는
    /// 단계이고, 실패했거나 바뀐 것이 없으면 None 이다. 다시 실행한 단계가 실패하면 그 단계는 버린다.
    pub(crate) fn record_step(&mut self, recorded: Recorded, step: Option<UndoStep>) {
        self.generation += 1;
        match (recorded, step) {
            (Recorded::New(source), Some(step)) => {
                self.clear_redo();
                self.push_undo(Entry::new(source, vec![step]));
            }
            (Recorded::Redo(entry), Some(step)) => {
                self.push_undo(Entry::new(entry.source, vec![step]));
            }
            // 되돌리기는 늘 항목별 보고를 낸다. 여기 오지 않지만 오면 단계를 잃지 않게 제자리에 둔다.
            (Recorded::Undo(entry), _) => self.push_undo(entry),
            (Recorded::New(_) | Recorded::Redo(_), None) => {}
        }
    }
    fn clear_redo(&mut self) {
        self.redo.clear();
        self.cleared += 1;
    }
}

#[cfg(test)]
#[path = "history_tests.rs"]
mod tests;
