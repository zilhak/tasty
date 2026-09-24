//! workspace 닫기 단계의 시간을 tasty::close에 기록한다.
//! surface마다 로그를 쓰지 않고 합산해 계측 자체의 쓰기 비용을 줄인다.

use std::sync::Mutex;
use std::time::{Duration, Instant};

pub(crate) fn elapsed_ms(since: Instant) -> f64 {
    since.elapsed().as_secs_f64() * 1000.0
}

pub(crate) fn duration_ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// 여러 함수에 걸친 닫기 작업의 시작 시각. take로 소비하며 남은 값은 다음 arm이 덮어쓴다.
static CASCADE_T0: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

static CASCADE_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const CASCADE_WHAT: &str = "close-trace cascade cell";

/// snapshot은 해당 닫기에서 스냅샷 단계를 실행했는지를 뜻한다.
pub(crate) fn arm_cascade(t0: Instant, snapshot: bool) {
    let mut g = crate::poison::recover_mutex(CASCADE_T0.lock(), CASCADE_WHAT, &CASCADE_POISONED);
    *g = Some((t0, snapshot));
}

/// 저장된 시작 시각을 가져오며 값이 없으면 전체 닫기 시간도 기록하지 않는다.
#[cfg(feature = "gui")]
pub(crate) fn take_cascade() -> Option<(Instant, bool)> {
    let mut g = crate::poison::recover_mutex(CASCADE_T0.lock(), CASCADE_WHAT, &CASCADE_POISONED);
    g.take()
}

/// 스크롤백을 파일로 옮기기 전에 호출해야 인라인 줄 수를 함께 측정할 수 있다.
pub(crate) fn log_snapshot(t: Instant, item: &crate::model::ClosedItem, path: &'static str) {
    let extent = tasty_model::closed_item::snapshot_extent(item);
    tracing::info!(
        target: "tasty::close",
        ms = elapsed_ms(t),
        surfaces = extent.surfaces,
        lines = extent.scrollback_lines,
        path,
        "C1 snapshot (capture_workspace_snapshot)"
    );
}

pub(crate) fn log_collect(t: Instant, surfaces: usize, path: &'static str) {
    tracing::info!(
        target: "tasty::close",
        ms = elapsed_ms(t),
        surfaces,
        path,
        "C3 collect_targets (pane x tab x leaf walk)"
    );
}

pub(crate) fn log_ws_purge(t: Instant, path: &'static str) {
    tracing::info!(
        target: "tasty::close",
        ms = elapsed_ms(t),
        path,
        "C4 ws_memory_purge (purge_scope(Workspace))"
    );
}

/// close 전체와 세부 단계의 합이 다르면 계측하지 않은 구간도 살펴본다.
pub(crate) fn log_total(t0: Instant, surfaces: usize, snapshot: bool, path: &'static str) {
    tracing::info!(
        target: "tasty::close",
        ms = elapsed_ms(t0),
        surfaces,
        snapshot,
        path,
        "close_total (workspace close)"
    );
}

/// surface 정리를 반복하는 호출자가 누적기를 소유하고 각 정리가 시간을 더한다.
#[derive(Default, Clone, Copy)]
pub(crate) struct CleanupSums {
    /// 0도 기록해 정리 대상이 없는 경우와 계측 누락을 구분한다.
    pub(crate) surfaces: u64,
    pub(crate) scrollback_delete: Duration,
    /// Terminal의 필드 Drop까지 포함한 시간.
    pub(crate) terminal_drop: Duration,
    /// 호스트 인덱스 해제 시간. observer sender는 놓지만 워커 join은 미룬다.
    pub(crate) indices_drop: Duration,
    /// surface 범위 메모리 정리 시간.
    pub(crate) memory_purge: Duration,
}

impl CleanupSums {
    /// total은 이 로그를 쓰기 전 cleanup 루프 전체 시간이다.
    pub(crate) fn log(&self, total: Duration, path: &'static str) {
        tracing::info!(
            target: "tasty::close",
            ms = duration_ms(total),
            surfaces = self.surfaces,
            path,
            scrollback_delete_ms = duration_ms(self.scrollback_delete),
            terminal_drop_ms = duration_ms(self.terminal_drop),
            indices_drop_ms = duration_ms(self.indices_drop),
            memory_purge_ms = duration_ms(self.memory_purge),
            "C5 cleanup_targets (C5a scrollback_delete / C5b terminal_drop / C5c indices_drop / C5d memory_purge)"
        );
    }
}

/// push_closed_item의 세부 시간. workspace 닫기에서만 로그를 남긴다.
#[derive(Default, Clone, Copy)]
pub(crate) struct PushClosedItemTimings {
    pub(crate) restore_inject: Duration,
    pub(crate) scrollback_persist: Duration,
    pub(crate) evict: Duration,
}

impl PushClosedItemTimings {
    pub(crate) fn log(&self, total: Duration, path: &'static str) {
        tracing::info!(
            target: "tasty::close",
            ms = duration_ms(total),
            path,
            restore_inject_ms = duration_ms(self.restore_inject),
            scrollback_persist_ms = duration_ms(self.scrollback_persist),
            evict_ms = duration_ms(self.evict),
            "C2 push_closed_item (C2a restore_inject / C2b scrollback_persist / C2c evict)"
        );
    }
}
