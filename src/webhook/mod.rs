//! 외부 HTTP 요청을 등록된 IPC 시퀀스로 전달하는 웹훅.
//! 프로세스가 리스너와 등록 목록을 공유하며 랜덤 ID를 URL 경로로 사용한다.
//! 응답은 고정 ACK이며 실행 결과를 포함하지 않는다.
//!
//! registry는 등록·조회·정리, lifetime은 만료 조건, persist는 설정 저장·복원을 맡는다.
//! listener는 요청을 받고 auth·abuse로 인증과 반복 실패를 검사한다.

pub mod abuse;
pub mod ack;
pub mod auth;
pub mod bind;
pub mod config;
pub mod lifetime;
pub mod listener;
pub mod persist;
pub mod registry;

pub use auth::{AuthLocation, WebhookAuth, auth_summary};
pub use bind::{ExplicitBindError, Reservation};
pub use lifetime::{Lifetime, Limit, Persistence};
pub use registry::{WebhookEntry, info, list, register, sweep, unregister};

use tasty_ipc::host_call::HostIpcInjector;

/// 리스너 시작 결과. 명시 지정 포트의 bind 실패는 부팅 초기의 [`prepare`]가 실행을 막으므로 여기 없다.
#[derive(Debug, PartialEq, Eq)]
pub enum WebhookInitReport {
    /// 기본 포트나 명시 지정 포트에 bind했다.
    Bound,
    /// 기본 포트가 막혀 다른 포트에 bind했고 복원한 Persistent 웹훅이 있다. URL의 포트가 바뀌었다.
    MovedWithPersistent { port: u16 },
    /// 탐색 범위가 모두 막혀 리스너 없이 실행한다.
    Unavailable,
}

impl WebhookInitReport {
    fn from_bound(bound: Option<crate::runtime_ports::BoundPort>, has_persistent: bool) -> Self {
        match bound {
            None => Self::Unavailable,
            Some(b) if !b.source.is_explicit() && b.addr.port() != config::DEFAULT_PORT => {
                if has_persistent {
                    Self::MovedWithPersistent {
                        port: b.addr.port(),
                    }
                } else {
                    Self::Bound
                }
            }
            Some(_) => Self::Bound,
        }
    }

    /// 사용자에게 알릴 경고. GUI는 toast로, headless는 로그로 낸다.
    pub fn warning(&self) -> Option<String> {
        let last = config::DEFAULT_PORT + bind::PROBE_COUNT - 1;
        match self {
            Self::Bound => None,
            Self::MovedWithPersistent { port } => Some(crate::i18n::t_fmt2(
                "webhook.warn.moved",
                &config::DEFAULT_PORT.to_string(),
                &port.to_string(),
            )),
            Self::Unavailable => Some(crate::i18n::t_fmt2(
                "webhook.warn.unavailable",
                &config::DEFAULT_PORT.to_string(),
                &last.to_string(),
            )),
        }
    }
}

/// 모든 IPv4 인터페이스에서 수신한다.
const BIND_ADDR: std::net::IpAddr = std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED);

/// 부팅 초기에 GUI·headless가 부른다. 실행 인자(`--webhook-port`)가 설정 파일보다 우선하며,
/// 명시 지정 포트를 bind하지 못하면 오류를 돌려줘 호출자가 실행을 막는다.
pub fn prepare(argument: Option<u16>) -> Result<Reservation, ExplicitBindError> {
    bind::reserve(bind::request(argument, config::read_port()), BIND_ADDR)
}

/// IPC와 injector가 준비된 뒤 GUI·headless가 부른다. 선점한 소켓으로 리스너를 시작하고
/// Persistent 웹훅을 복원한다. 결과의 경고는 호출자가 표시한다.
pub fn start(
    reservation: Option<Reservation>,
    injector: HostIpcInjector,
    ports: std::sync::Arc<crate::runtime_ports::RuntimePorts>,
) -> WebhookInitReport {
    // 복원 항목의 URL을 만들기 전에 포트 기록을 연결한다.
    registry::set_runtime(injector, ports.clone());
    let bound = reservation.as_ref().and_then(Reservation::bound);
    if let Some((server, bound)) = reservation.and_then(|r| r.listener) {
        listener::start(server, bound, &ports);
    }
    let restored = persist::restore_into_registry();
    let report = WebhookInitReport::from_bound(bound, restored > 0);
    if let Some(message) = report.warning() {
        tracing::warn!("{message}");
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_ports::{BoundPort, PortSource};

    fn at(port: u16, source: PortSource) -> Option<BoundPort> {
        Some(BoundPort {
            addr: std::net::SocketAddr::from(([127, 0, 0, 1], port)),
            source,
        })
    }

    #[test]
    fn only_a_probed_move_with_saved_webhooks_warns() {
        use WebhookInitReport as R;
        assert_eq!(R::from_bound(at(28429, PortSource::Probe), true), R::Bound);
        assert_eq!(
            R::from_bound(at(28430, PortSource::Probe), true),
            R::MovedWithPersistent { port: 28430 }
        );
        assert_eq!(R::from_bound(at(28430, PortSource::Probe), false), R::Bound);
        assert_eq!(R::from_bound(at(40000, PortSource::Config), true), R::Bound);
        assert_eq!(R::from_bound(None, true), R::Unavailable);
    }
}
