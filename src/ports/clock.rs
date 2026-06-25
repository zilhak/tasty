//! Clock port — 시각 source. test 시 deterministic.

use std::time::{Instant, SystemTime};

#[allow(dead_code)] // 이유: Clock port — DI 빌더 배선·std_clock 어댑터 존재, 호출 경로 배선 대기
pub trait Clock: Send + Sync {
    fn now_instant(&self) -> Instant;
    fn now_system(&self) -> SystemTime;
    /// Unix ms (telemetry / audit / memory entry timestamp 용).
    fn now_unix_millis(&self) -> i64;
}
