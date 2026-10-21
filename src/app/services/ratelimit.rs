//! Core의 메모리 저장소에서 에이전트별 사용 한도를 관리한다.

use tasty_agent::{AgentError, ConsumeOutcome, RateLimit, RateLimitStore};
use tasty_memory::HOST_OWNER;

pub(crate) fn set(
    mem: &mut dyn tasty_memory::MemoryStorage,
    agent: String,
    metric: String,
    limit: u32,
    per_ms: u64,
    burst: Option<u32>,
    now_ms: u64,
) -> Result<RateLimit, AgentError> {
    let mut store = RateLimitStore::new(mem, HOST_OWNER);
    store.set(agent, metric, limit, per_ms, burst, now_ms)
}

pub(crate) fn remove(
    mem: &mut dyn tasty_memory::MemoryStorage,
    id: &str,
) -> Result<(), AgentError> {
    let mut store = RateLimitStore::new(mem, HOST_OWNER);
    store.remove(id)
}

/// 토큰을 보충한 전체 목록을 반환하며 agent·metric 필터는 핸들러가 적용한다.
pub(crate) fn status(
    mem: &mut dyn tasty_memory::MemoryStorage,
    now_ms: u64,
) -> Result<Vec<RateLimit>, AgentError> {
    let mut store = RateLimitStore::new(mem, HOST_OWNER);
    store.status(now_ms)
}

/// 등록되지 않은 agent·metric 조합은 제한 없이 허용한다.
pub(crate) fn try_consume(
    mem: &mut dyn tasty_memory::MemoryStorage,
    agent: &str,
    metric: &str,
    cost: u32,
    now_ms: u64,
) -> Result<ConsumeOutcome, AgentError> {
    let mut store = RateLimitStore::new(mem, HOST_OWNER);
    store.try_consume(agent, metric, cost, now_ms)
}

use crate::app::services::AppServices;

impl AppServices {
    /// 시험이 게이트 한도를 미리 채울 때 쓴다. 결과를 기다리므로 메인 루프는 쓰지 않는다.
    #[cfg(test)]
    pub(crate) fn rate_limit_set(
        &self,
        agent: String,
        metric: String,
        limit: u32,
        per_ms: u64,
        burst: Option<u32>,
        now_ms: u64,
    ) -> Result<RateLimit, AgentError> {
        self.memory_access()
            .run_blocking(0, move |mem| {
                set(
                    mem,
                    agent.clone(),
                    metric.clone(),
                    limit,
                    per_ms,
                    burst,
                    now_ms,
                )
            })
            .map_err(|blocked| AgentError::from(blocked.to_memory_error()))?
    }
}
