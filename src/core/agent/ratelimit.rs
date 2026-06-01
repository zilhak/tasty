//! Rate limit store wrapper. handler 의 `core.with_memory + RateLimitStore::new`
//! 조립을 본 모듈로 흡수.

use tasty_agent::{AgentError, RateLimit, RateLimitStore};
use tasty_memory::HOST_OWNER;

use crate::core::Core;

impl Core {
    /// (agent, metric) 키로 rate limit upsert.
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

    /// Rate limit 삭제.
    pub(crate) fn rate_limit_remove(&self, id: &str) -> Result<(), AgentError> {
        self.with_memory(|mem| {
            let mut store = RateLimitStore::new(mem, HOST_OWNER);
            store.remove(id)
        })
    }

    /// 모든 rate_limit refill 후 반환. handler 가 agent/metric 필터를 적용.
    pub(crate) fn rate_limit_status(&self, now_ms: u64) -> Result<Vec<RateLimit>, AgentError> {
        self.with_memory(|mem| {
            let mut store = RateLimitStore::new(mem, HOST_OWNER);
            store.status(now_ms)
        })
    }
}
