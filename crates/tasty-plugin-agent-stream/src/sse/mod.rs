//! 수집한 스트림 이벤트를 외부 구독자에게 흘리는 **SSE 서버**.
//!
//! 방향이 "plugin 이 서버, 소비자가 구독하러 온다" 인 이유는 인바운드(웹훅)가 이미
//! **소비자 → tasty** 방향이기 때문이다. 소비자가 tasty 에 도달 가능하다는 것이 이미
//! 전제이므로, 아웃바운드도 같은 방향으로 두면 방화벽/NAT 요구가 새로 생기지 않는다.
//! 근거·대안·재검토 조건은 `docs/adr/0100-agent-stream-sse-endpoint-exposure.md`.
//!
//! 구성:
//!
//! - [`frame`] — SSE 프레이밍(`id:`/`event:`/`data:` + 빈 줄, 개행 처리).
//! - [`hub`] — 구독자 레지스트리 + 구독자별 bounded 큐(생산자 무블로킹).
//! - [`request`] — 경로·쿼리·인증·구독 옵션 해석(HTTP 레이어 비의존).
//! - [`server`] — tiny_http bind + accept 스레드 + 연결당 스레드.

pub mod frame;
pub mod hub;
pub mod request;
pub mod server;

use std::net::IpAddr;

use serde_json::{Value, json};

/// SSE 서버 기동 설정.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ServeConfig {
    pub bind: String,
    pub port: u16,
    /// 구독 토큰. `None` 이면 무인증(loopback 에서만 허용).
    pub token: Option<String>,
}

/// 설정이 거부되는 이유. 사람이 읽는 문자열은 호출자가 i18n 키로 만든다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// 포트 0 = "아무 포트나" 는 받지 않는다.
    PortRequired,
    /// bind 주소가 IP 로 해석되지 않는다.
    InvalidBind,
    /// loopback 이 아닌 주소에 토큰 없이 열려 했다.
    RemoteBindNeedsToken,
}

impl ServeConfig {
    /// 기본 bind 주소 — loopback. 대화 전문이 나가는 채널이라 명시적으로 넓히기 전에는
    /// 같은 기기 밖으로 나가지 않는다.
    pub const DEFAULT_BIND: &'static str = "127.0.0.1";

    /// 설정을 검증한다.
    ///
    /// **포트는 필수이고 자동 폴백이 없다** — 본체 웹훅 리스너의 "설정값 only" 정책과
    /// 같다(`src/webhook/listener.rs`). 임의 포트로 조용히 옮겨 뜨면 소비자가 어디에
    /// 붙어야 하는지 알 수 없고, 재시작마다 주소가 바뀐다.
    ///
    /// **loopback 이 아닌 주소는 토큰 없이는 거부한다** — 실수로 `0.0.0.0` 에 무인증으로
    /// 열려 대화 전문이 네트워크에 공개되는 조합을 구조적으로 막는다.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port == 0 {
            return Err(ConfigError::PortRequired);
        }
        let loopback = is_loopback(&self.bind).ok_or(ConfigError::InvalidBind)?;
        if !loopback && self.token.is_none() {
            return Err(ConfigError::RemoteBindNeedsToken);
        }
        Ok(())
    }

    /// 영속화/조회용 JSON. **토큰은 절대 싣지 않는다** — 설정돼 있는지 여부만 노출한다.
    pub fn to_public_json(&self) -> Value {
        json!({
            "bind": self.bind,
            "port": self.port,
            "auth": self.token.is_some(),
        })
    }
}

/// bind 주소가 loopback 인가. IP 로 해석되지 않으면 `None`.
fn is_loopback(bind: &str) -> Option<bool> {
    if bind.eq_ignore_ascii_case("localhost") {
        return Some(true);
    }
    let addr: IpAddr = bind.parse().ok()?;
    Some(addr.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(bind: &str, port: u16, token: Option<&str>) -> ServeConfig {
        ServeConfig {
            bind: bind.to_string(),
            port,
            token: token.map(str::to_string),
        }
    }

    #[test]
    fn a_port_is_required_there_is_no_automatic_allocation() {
        assert_eq!(
            config("127.0.0.1", 0, None).validate(),
            Err(ConfigError::PortRequired)
        );
        assert!(config("127.0.0.1", 8123, None).validate().is_ok());
    }

    #[test]
    fn loopback_may_run_without_a_token_but_a_wider_bind_may_not() {
        assert!(config("127.0.0.1", 1, None).validate().is_ok());
        assert!(config("localhost", 1, None).validate().is_ok());
        assert!(config("::1", 1, None).validate().is_ok());
        assert_eq!(
            config("0.0.0.0", 1, None).validate(),
            Err(ConfigError::RemoteBindNeedsToken)
        );
        assert!(config("0.0.0.0", 1, Some("t")).validate().is_ok());
    }

    #[test]
    fn a_bind_address_that_is_not_an_ip_is_rejected() {
        assert_eq!(
            config("example.com", 1, Some("t")).validate(),
            Err(ConfigError::InvalidBind)
        );
    }

    #[test]
    fn the_public_view_never_carries_the_token() {
        let view = config("127.0.0.1", 9, Some("super-secret")).to_public_json();
        assert!(!view.to_string().contains("super-secret"));
        assert_eq!(view["auth"], json!(true));
    }
}
