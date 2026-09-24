//! 시각을 제공하는 인터페이스. 시험에서 시간을 직접 정할 수 있다.

use std::time::{Instant, SystemTime};

#[allow(dead_code)] // 이유: 빌더와 어댑터는 있지만 제품 코드의 호출부는 없다
pub trait Clock: Send + Sync {
    fn now_instant(&self) -> Instant;
    fn now_system(&self) -> SystemTime;
    /// Unix ms (telemetry / audit / memory entry timestamp 용).
    fn now_unix_millis(&self) -> i64;
}
