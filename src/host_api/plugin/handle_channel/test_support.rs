//! 테스트 전용 helper — `HandleListener` 의 `expect_connection` / `cancel_token`.
//!
//! 본 모듈은 `#[cfg(test)]` 로 mod 선언되어 있어 release 빌드에 절대 포함되지
//! 않는다. production 경로는 `register_token` + 자체 polling 으로 충분하고,
//! "토큰 매칭 후 일정 시간 안에 stream 수신 → 시한 초과 시 mailbox cleanup"
//! 시나리오는 `expect_connection` test 가 plugin 채널 spawn race condition
//! 회귀 검증을 한다.

use std::time::Duration;

use super::{HandleListener, HandleStream};

impl HandleListener {
    /// 해당 token으로 connect할 plugin의 stream을 기다린다. `timeout` 안에 안 오면 `None`.
    pub fn expect_connection(&self, token: &str, timeout: Duration) -> Option<HandleStream> {
        let rx = self.register_token(token);
        match rx.recv_timeout(timeout) {
            Ok(stream) => Some(stream),
            Err(_) => {
                self.cancel_token(token);
                None
            }
        }
    }

    /// 미사용 mailbox 명시적 제거. expect_connection 의 timeout cleanup 경로.
    pub fn cancel_token(&self, token: &str) {
        if let Ok(mut p) = self.pending.lock() {
            p.remove(token);
        }
    }
}

