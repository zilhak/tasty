//! 요청 게이트 시험용 저장 실패 주입. 세션 키 읽기와 cap 목록 읽기를 대상으로 정한 공용 시험 저장소다.

use std::sync::Arc;

pub(crate) use tasty_memory::testing::faults::{Faults, FaultyStore};

/// 세션 키 읽기(`key_reads`)와 cap 목록 읽기(`prefix_lists`)를 대상으로 삼는다.
pub(crate) fn gate_faults() -> Arc<Faults> {
    Arc::new(
        Faults::default()
            .reading(tasty_ipc::session::SESSION_KEY_PREFIX)
            .listing(tasty_telemetry::CAP_KEY_PREFIX),
    )
}
