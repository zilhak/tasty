//! Core의 메모리 저장소에서 에이전트별 사용 한도를 관리한다.

use tasty_agent::{AgentError, ConsumeOutcome, RateLimit, RateLimitStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;

impl Core {
    pub(crate) fn rate_limit_set(
        &self,
        agent: String,
        metric: String,
        limit: u32,
        per_ms: u64,
        burst: Option<u32>,
        now_ms: u64,
    ) -> Result<RateLimit, AgentError> {
        self.with_memory(|mem| {
            let mut store = RateLimitStore::new(mem, HOST_OWNER);
            store.set(agent, metric, limit, per_ms, burst, now_ms)
        })
    }

    pub(crate) fn rate_limit_remove(&self, id: &str) -> Result<(), AgentError> {
        self.with_memory(|mem| {
            let mut store = RateLimitStore::new(mem, HOST_OWNER);
            store.remove(id)
        })
    }

    /// 토큰을 보충한 전체 목록을 반환하며 agent·metric 필터는 핸들러가 적용한다.
    pub(crate) fn rate_limit_status(&self, now_ms: u64) -> Result<Vec<RateLimit>, AgentError> {
        self.with_memory(|mem| {
            let mut store = RateLimitStore::new(mem, HOST_OWNER);
            store.status(now_ms)
        })
    }

    /// 등록되지 않은 agent·metric 조합은 제한 없이 허용한다.
    pub(crate) fn rate_limit_try_consume(
        &self,
        agent: &str,
        metric: &str,
        cost: u32,
        now_ms: u64,
    ) -> Result<ConsumeOutcome, AgentError> {
        self.with_memory(|mem| {
            let mut store = RateLimitStore::new(mem, HOST_OWNER);
            store.try_consume(agent, metric, cost, now_ms)
        })
    }
}
