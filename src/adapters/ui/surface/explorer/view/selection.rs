//! 전체 선택의 집합 구성과 교체 구간. 경로 내용은 성능 로그에 남기지 않는다.

mod paths;

pub use paths::SelectedPaths;
use std::time::Instant;

use super::ExplorerView;

impl ExplorerView {
    /// 보이는 엔트리를 모두 선택. 앵커는 마지막 엔트리로 둔다. Find 로 숨긴 항목은 고르지 않는다.
    pub fn select_all(&mut self) {
        let timer = SelectionTimer::start(self.shown_count());
        let mut selected = std::mem::take(&mut self.selected);
        let mut last = None;
        selected.refill(
            self.shown_count(),
            self.shown().map(|entry| {
                last = Some(&entry.path);
                entry.path.as_path()
            }),
        );
        let anchor = last.cloned();
        timer.built(selected.packed_bytes());
        self.replace_selection(selected, anchor);
        timer.applied(self.selected.len());
    }
}

struct SelectionTimer(Option<Instant>);

impl SelectionTimer {
    fn start(shown: usize) -> Self {
        let started = tracing::enabled!(target: "tasty::explorer::perf", tracing::Level::TRACE)
            .then(Instant::now);
        tracing::trace!(target: "tasty::explorer::perf", shown, "selection_started");
        Self(started)
    }

    fn built(&self, packed_bytes: usize) {
        if let Some(started) = self.0 {
            tracing::trace!(target: "tasty::explorer::perf", packed_bytes, elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, "selection_built");
        }
    }

    fn applied(&self, selected: usize) {
        if let Some(started) = self.0 {
            tracing::trace!(target: "tasty::explorer::perf", selected, elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, "selection_applied");
        }
    }
}
