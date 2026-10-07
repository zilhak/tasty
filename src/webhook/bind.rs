//! 리스너 포트를 정하고 소켓을 선점한다. 부팅 초기에 이벤트 루프·저널보다 먼저 부른다.
//!
//! - 실행 인자 또는 설정 파일의 포트는 명시 지정이다. 그 포트를 bind하지 못하면 실행을 막는다.
//!   인자가 설정보다 우선한다.
//! - 명시 지정이 없으면 기본 포트부터 하나씩 올리며 bind를 시도한다. 상한 안에서 모두 막히면
//!   리스너 없이 실행한다.
//! - 점유 여부는 미리 조회하지 않고 실제 bind 결과로 판단한다. 조회와 bind 사이에 다른 프로세스가
//!   포트를 가져가는 경우가 생기지 않는다.

use std::net::{IpAddr, SocketAddr};

use crate::runtime_ports::{BoundPort, PortSource};

use super::config::DEFAULT_PORT;

/// 명시 지정이 없을 때 시도하는 포트 수. 기본 포트부터 `DEFAULT_PORT + PROBE_COUNT - 1`까지다.
pub const PROBE_COUNT: u16 = 64;

/// 이번 실행의 포트 요청.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortRequest {
    Explicit { port: u16, source: PortSource },
    Probe,
}

/// 실행 인자가 설정 파일보다 우선한다.
pub fn request(argument: Option<u16>, config: Option<u16>) -> PortRequest {
    match (argument, config) {
        (Some(port), _) => PortRequest::Explicit {
            port,
            source: PortSource::Argument,
        },
        (None, Some(port)) => PortRequest::Explicit {
            port,
            source: PortSource::Config,
        },
        (None, None) => PortRequest::Probe,
    }
}

/// 명시 지정한 포트를 bind하지 못했다. 실행을 막는다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExplicitBindError {
    pub addr: SocketAddr,
    pub source: PortSource,
    pub error: String,
}

impl ExplicitBindError {
    /// 실행을 막을 때 보이는 제목·본문·안내. GUI 오류 화면과 headless 표준 오류가 같이 쓴다.
    pub fn message(&self) -> (String, String, String) {
        let title = crate::i18n::t("boot.webhook_port.title").to_string();
        let body = crate::i18n::t_fmt2(
            "boot.webhook_port.body",
            &self.addr.port().to_string(),
            &self.error,
        );
        let hint = match self.source {
            PortSource::Argument => crate::i18n::t("boot.webhook_port.hint_argument").to_string(),
            _ => crate::i18n::t_fmt(
                "boot.webhook_port.hint_config",
                &super::config::config_path().display().to_string(),
            ),
        };
        (title, body, hint)
    }
}

/// 선점 결과. `listener`가 None이면 탐색 범위가 모두 막혀 리스너 없이 실행한다.
pub struct Reservation {
    pub(super) listener: Option<(tiny_http::Server, BoundPort)>,
}

impl Reservation {
    /// bind한 주소. 리스너가 없으면 None이다.
    pub fn bound(&self) -> Option<BoundPort> {
        self.listener.as_ref().map(|(_, bound)| *bound)
    }
}

/// 요청대로 bind를 시도한다.
pub fn reserve(request: PortRequest, ip: IpAddr) -> Result<Reservation, ExplicitBindError> {
    let listener = reserve_with(request, ip, |addr| {
        tiny_http::Server::http(addr).map_err(|e| e.to_string())
    })?;
    Ok(Reservation { listener })
}

/// bind 함수를 받아 순서만 정한다. 시험은 가짜 bind를 넘긴다.
fn reserve_with<S>(
    request: PortRequest,
    ip: IpAddr,
    bind: impl FnMut(SocketAddr) -> Result<S, String>,
) -> Result<Option<(S, BoundPort)>, ExplicitBindError> {
    match request {
        PortRequest::Explicit { port, source } => {
            reserve_explicit(SocketAddr::new(ip, port), source, bind).map(Some)
        }
        PortRequest::Probe => Ok(probe(ip, bind)),
    }
}

fn reserve_explicit<S>(
    addr: SocketAddr,
    source: PortSource,
    mut bind: impl FnMut(SocketAddr) -> Result<S, String>,
) -> Result<(S, BoundPort), ExplicitBindError> {
    match bind(addr) {
        Ok(server) => Ok((server, BoundPort { addr, source })),
        Err(error) => {
            // 리스너 로그와 같은 대상·문형으로 남겨 실행 로그에서 한 표지로 찾게 한다.
            tracing::error!(
                target: "tasty::webhook::listener",
                "webhook listener bind {addr} failed: {error} — explicit {source:?} port; not starting"
            );
            Err(ExplicitBindError {
                addr,
                source,
                error,
            })
        }
    }
}

fn probe<S>(
    ip: IpAddr,
    mut bind: impl FnMut(SocketAddr) -> Result<S, String>,
) -> Option<(S, BoundPort)> {
    let found = (0..PROBE_COUNT).find_map(|offset| {
        let addr = SocketAddr::new(ip, DEFAULT_PORT + offset);
        bind(addr)
            .inspect_err(|error| tracing::debug!("webhook bind {addr} failed: {error}"))
            .ok()
            .map(|server| (server, addr))
    });
    let Some((server, addr)) = found else {
        tracing::warn!(
            "webhook ports {DEFAULT_PORT}..={} are all taken; running without the webhook listener",
            DEFAULT_PORT + PROBE_COUNT - 1
        );
        return None;
    };
    if addr.port() != DEFAULT_PORT {
        tracing::info!(
            "webhook port {DEFAULT_PORT} is taken; using {}",
            addr.port()
        );
    }
    let bound = BoundPort {
        addr,
        source: PortSource::Probe,
    };
    Some((server, bound))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOOPBACK: IpAddr = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);

    /// `busy`에 든 포트는 실패하는 가짜 bind. 시도한 순서를 남긴다.
    fn fake(busy: &[u16], tried: &mut Vec<u16>) -> impl FnMut(SocketAddr) -> Result<(), String> {
        let busy = busy.to_vec();
        move |addr| {
            tried.push(addr.port());
            if busy.contains(&addr.port()) {
                Err("Address in use".into())
            } else {
                Ok(())
            }
        }
    }

    #[test]
    fn the_argument_wins_over_the_config() {
        assert_eq!(
            request(Some(30000), Some(40000)),
            PortRequest::Explicit {
                port: 30000,
                source: PortSource::Argument
            }
        );
        assert_eq!(
            request(None, Some(40000)),
            PortRequest::Explicit {
                port: 40000,
                source: PortSource::Config
            }
        );
        assert_eq!(request(None, None), PortRequest::Probe);
    }

    #[test]
    fn probing_skips_busy_ports_from_the_default() {
        let mut tried = Vec::new();
        let got = reserve_with(
            PortRequest::Probe,
            LOOPBACK,
            fake(&[28429, 28430], &mut tried),
        )
        .unwrap()
        .unwrap();
        assert_eq!(got.1.addr.port(), 28431);
        assert_eq!(got.1.source, PortSource::Probe);
        assert_eq!(tried, vec![28429, 28430, 28431]);
    }

    #[test]
    fn probing_stops_after_the_limit_without_a_listener() {
        let all: Vec<u16> = (DEFAULT_PORT..DEFAULT_PORT + PROBE_COUNT).collect();
        let mut tried = Vec::new();
        let got = reserve_with(PortRequest::Probe, LOOPBACK, fake(&all, &mut tried)).unwrap();
        assert!(got.is_none());
        assert_eq!(tried.len(), 64);
        assert_eq!(tried.last(), Some(&28492));
    }

    #[test]
    fn a_busy_explicit_port_is_an_error_and_is_not_probed_past() {
        let mut tried = Vec::new();
        let request = PortRequest::Explicit {
            port: 28429,
            source: PortSource::Argument,
        };
        let Err(error) = reserve_with(request, LOOPBACK, fake(&[28429], &mut tried)) else {
            panic!("a busy explicit port must refuse");
        };
        assert_eq!(error.addr.port(), 28429);
        assert_eq!(error.source, PortSource::Argument);
        assert_eq!(tried, vec![28429]);
    }

    #[test]
    fn a_real_socket_held_by_someone_else_is_skipped() {
        // 실제 bind 결과로 판단하는지 본다. 다른 시험과 겹치지 않게 OS가 고른 포트를 점유한다.
        let held = std::net::TcpListener::bind((LOOPBACK, 0)).unwrap();
        let port = held.local_addr().unwrap().port();
        let request = PortRequest::Explicit {
            port,
            source: PortSource::Config,
        };
        assert!(reserve(request, LOOPBACK).is_err());
        drop(held);
    }
}
