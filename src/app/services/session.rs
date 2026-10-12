//! 요청 게이트가 세션을 판정할 때 Core의 공유 저장소에서 바로 읽는다. 세션 IPC는 memory writer를 쓴다.

use tasty_memory::HOST_OWNER;

use crate::app::services::AppServices;
use tasty_ipc::caller::SessionToken;
use tasty_ipc::session::{AgentSession, SessionError, SessionStore};

impl AppServices {
    /// 만료·폐기된 token은 Ok(None)이다.
    pub(crate) fn session_resolve(
        &self,
        token: &SessionToken,
        now_ms: u64,
    ) -> Result<Option<AgentSession>, SessionError> {
        self.with_memory(|mem| {
            let mut store = SessionStore::new(mem, HOST_OWNER);
            store.resolve(token, now_ms)
        })
    }
}
