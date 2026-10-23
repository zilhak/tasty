//! 헤드리스 메모리 저장소의 부팅 초기화와 기존 대체 저장소 연결.

use super::maintain_memory_at_boot;

/// 메모리 저장소 초기화 실패는 로그를 남기고 None으로 반환해 계속 실행한다.
#[cfg(not(feature = "gui"))]
pub(super) fn boot_memory(
    boot_settings: &crate::settings::Settings,
) -> Option<std::sync::Arc<std::sync::Mutex<tasty_memory::MemoryStore>>> {
    let memory_config = tasty_memory::MemoryConfig {
        entry_max_bytes: boot_settings
            .memory
            .entry_max_mb
            .saturating_mul(1024 * 1024),
        secret_quota_per_owner_bytes: boot_settings
            .memory
            .secret_quota_mb_per_plugin
            .saturating_mul(1024 * 1024),
        regular_quota_total_bytes: boot_settings
            .memory
            .regular_quota_mb_total
            .saturating_mul(1024 * 1024),
    };
    match tasty_memory::init_with_config(memory_config) {
        Ok(arc) => {
            maintain_memory_at_boot(&arc);
            Some(arc)
        }
        Err(e) => {
            tracing::warn!(target: "tasty::boot", "memory.db init at boot failed: {e}");
            crate::boot::wiring::memory_fallback_after(&e)
        }
    }
}
