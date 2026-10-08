//! journal 시험의 정체 감지([`StallBudget`])를 저널 애플리케이션 단위로 쓴다.

pub(super) use crate::runtime::journal_product::stall_budget::StallBudget;

use super::JournalApplication;

impl StallBudget {
    pub(crate) fn new(journal: &JournalApplication) -> Self {
        Self::for_worker(&journal.worker)
    }

    /// worker를 새로 띄운 journal로 바꾼다. 쌓인 정체 시간은 유지한다.
    pub(crate) fn watch(&mut self, journal: &JournalApplication) {
        self.watch_worker(&journal.worker);
    }
}
