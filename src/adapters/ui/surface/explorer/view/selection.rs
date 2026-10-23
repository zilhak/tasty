//! 전체 선택의 집합 구성과 교체 구간. 경로 내용은 성능 로그에 남기지 않는다.

use std::collections::HashSet;
use std::time::Instant;

use super::ExplorerView;

impl ExplorerView {
    /// 보이는 엔트리를 모두 선택. 앵커는 마지막 엔트리로 둔다. Find 로 숨긴 항목은 고르지 않는다.
    pub fn select_all(&mut self) {
        let started = tracing::enabled!(target: "tasty::explorer::perf", tracing::Level::TRACE)
            .then(Instant::now);
        tracing::trace!(target: "tasty::explorer::perf", shown = self.shown_count(), "selection_started");
        let mut selected = HashSet::with_capacity(self.shown_count());
        let mut last = None;
        for entry in self.shown() {
            selected.insert(entry.path.clone());
            last = Some(&entry.path);
        }
        let anchor = last.cloned();
        if let Some(started) = started {
            tracing::trace!(target: "tasty::explorer::perf", elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, "selection_built");
        }
        self.replace_selection(selected, anchor);
        if let Some(started) = started {
            tracing::trace!(target: "tasty::explorer::perf", selected = self.selected.len(), elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, "selection_applied");
        }
    }
}
