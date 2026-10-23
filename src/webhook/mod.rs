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
pub use registry::WebhookEntry;

use tasty_ipc::host_call::HostIpcInjector;

/// 리스너 시작 결과.
#[derive(Debug, PartialEq, Eq)]
pub enum WebhookInitReport {
    /// 기본 포트나 명시 지정 포트에 bind했다.
    Bound,
    /// The restored registry rewrite must settle before the listener accepts.
    Starting,
    /// 기본 포트가 막혀 다른 포트에 bind했고 복원한 Persistent 웹훅이 있다. URL의 포트가 바뀌었다.
    MovedWithPersistent { port: u16 },
    /// 탐색 범위가 모두 막혀 리스너 없이 실행한다.
    Unavailable,
    /// 명시 지정 포트를 열지 못해 다른 포트로 옮기지 않고 리스너 없이 실행한다.
    ExplicitUnavailable(ExplicitBindError),
    /// 포트는 열었지만 요청을 받을 스레드를 띄우지 못해 소켓을 닫고 리스너 없이 실행한다.
    ListenerFailed(ExplicitBindError),
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

    /// 사용자에게 알릴 경고 알림. GUI·headless 모두 알림 저장소에 경고로 남긴다.
    /// 시작 단계의 경고이며 웹훅 실행 실패와는 출처 값으로 구분한다.
    pub fn notice(&self) -> Option<crate::system_notice::SystemNotice> {
        let last = config::DEFAULT_PORT + bind::PROBE_COUNT - 1;
        let body = match self {
            Self::Bound | Self::Starting => return None,
            Self::MovedWithPersistent { port } => crate::i18n::t_fmt2(
                "webhook.warn.moved",
                &config::DEFAULT_PORT.to_string(),
                &port.to_string(),
            ),
            Self::Unavailable => crate::i18n::t_fmt2(
                "webhook.warn.unavailable",
                &config::DEFAULT_PORT.to_string(),
                &last.to_string(),
            ),
            Self::ExplicitUnavailable(failure) => failure.notice_body(),
            Self::ListenerFailed(failure) => crate::i18n::t_fmt2(
                "webhook.warn.listener_failed",
                &failure.addr.port().to_string(),
                &failure.error,
            ),
        };
        // 포트만 바뀐 경우는 리스너가 동작하므로 "꺼졌다"는 제목을 쓰지 않는다.
        let title = match self {
            Self::MovedWithPersistent { .. } => "webhook.warn.moved_title",
            _ => "webhook.warn.title",
        };
        Some(crate::system_notice::SystemNotice::warning(
            NOTICE_SOURCE_START,
            crate::i18n::t(title).to_string(),
            body,
        ))
    }
}

/// 리스너 시작 경고의 알림 source.
pub const NOTICE_SOURCE_START: &str = "webhook.start";

/// 리스너 주소. 기본은 이 컴퓨터에서만 받는 loopback이고, 설정으로 켜면 모든 IPv4 인터페이스에서 받는다.
pub fn bind_ip(settings: &tasty_settings::WebhookSettings) -> std::net::IpAddr {
    if settings.allow_external {
        std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED)
    } else {
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)
    }
}

/// 부팅 초기에 GUI·headless가 부른다. 실행 인자(`--webhook-port`)가 설정 파일보다 우선한다.
/// 명시 지정 포트를 bind하지 못해도 실행을 막지 않고 원인을 선점 결과에 남긴다.
pub fn prepare(argument: Option<u16>, settings: &tasty_settings::WebhookSettings) -> Reservation {
    bind::reserve(
        bind::request(argument, config::read_port()),
        bind_ip(settings),
    )
}

/// IPC와 injector가 준비된 뒤 GUI·headless가 부른다. 선점한 소켓으로 리스너를 시작하고
/// Persistent 웹훅을 복원한다. 리스너를 열지 못했으면 등록은 남지만 URL이 응답하지 않는다.
/// 경고는 알림 채널로 남기며 첫 workspace가 생긴 뒤 저장된다.
pub(crate) fn start(
    reservation: Option<Reservation>,
    injector: HostIpcInjector,
    ports: std::sync::Arc<crate::runtime_ports::RuntimePorts>,
    registry: std::sync::Arc<registry::Registry>,
) -> WebhookInitReport {
    if registry.is_bound() {
        return WebhookInitReport::Bound;
    }
    registry.set_runtime(injector, ports.clone());
    registry.begin_restore();
    let restored = persist::restore_into_registry(&registry);
    let next = registry.clone();
    registry.after_restore(move || {
        let report = start_restored(reservation, ports, next, restored);
        if let Some(notice) = report.notice() {
            tracing::warn!("{}", notice.body);
            crate::system_notice::post(notice);
        }
    });
    WebhookInitReport::Starting
}
fn start_restored(
    reservation: Option<Reservation>,
    ports: std::sync::Arc<crate::runtime_ports::RuntimePorts>,
    registry: std::sync::Arc<registry::Registry>,
    restored: usize,
) -> WebhookInitReport {
    let explicit = reservation
        .as_ref()
        .and_then(Reservation::explicit_failure)
        .cloned();
    let mut bound = reservation.as_ref().and_then(Reservation::bound);
    let mut thread_failure = None;
    if let Some((server, at)) = reservation.and_then(|r| r.listener) {
        match listener::start(server, at, &ports, &registry) {
            Ok(listener) => registry.install_listener(listener),
            Err(error) => {
                bound = None;
                thread_failure = Some(ExplicitBindError {
                    addr: at.addr,
                    source: at.source,
                    error,
                });
            }
        }
    }
    if let Some(failure) = explicit.as_ref().or(thread_failure.as_ref()) {
        ports.record_webhook_failure(failure.clone());
    }
    match (explicit, thread_failure) {
        (Some(failure), _) => WebhookInitReport::ExplicitUnavailable(failure),
        (None, Some(failure)) => WebhookInitReport::ListenerFailed(failure),
        (None, None) => WebhookInitReport::from_bound(bound, restored > 0),
    }
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
    fn the_listener_is_loopback_unless_external_calls_are_allowed() {
        let mut settings = tasty_settings::WebhookSettings::default();
        assert!(bind_ip(&settings).is_loopback());
        settings.allow_external = true;
        assert!(bind_ip(&settings).is_unspecified());
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

    #[test]
    fn every_listener_problem_becomes_one_start_warning() {
        use WebhookInitReport as R;
        assert!(R::Bound.notice().is_none());
        let failure = ExplicitBindError {
            addr: std::net::SocketAddr::from(([127, 0, 0, 1], 41000)),
            source: PortSource::Config,
            error: "Address in use".into(),
        };
        for report in [
            R::MovedWithPersistent { port: 28430 },
            R::Unavailable,
            R::ListenerFailed(failure.clone()),
            R::ExplicitUnavailable(failure),
        ] {
            let off = !matches!(report, R::MovedWithPersistent { .. });
            let notice = report.notice().expect("a warning");
            let off_title = crate::i18n::t("webhook.warn.title");
            assert_eq!(
                notice.title == off_title,
                off,
                "{report:?}: {}",
                notice.title
            );
            assert_eq!(
                notice.level,
                crate::notification::NotificationLevel::Warning
            );
            assert_eq!(notice.source, NOTICE_SOURCE_START);
        }
        let failure = ExplicitBindError {
            addr: std::net::SocketAddr::from(([127, 0, 0, 1], 41000)),
            source: PortSource::Argument,
            error: "Address in use".into(),
        };
        let body = failure.notice_body();
        assert!(
            body.contains("41000") && body.contains("Address in use"),
            "{body}"
        );
    }
}
