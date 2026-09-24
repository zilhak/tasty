//! 외부 HTTP 요청을 등록된 IPC 시퀀스로 전달하는 웹훅.
//! 프로세스가 리스너와 등록 목록을 공유하며 랜덤 ID를 URL 경로로 사용한다.
//! 응답은 고정 ACK이며 실행 결과를 포함하지 않는다.
//!
//! registry는 등록·조회·정리, lifetime은 만료 조건, persist는 설정 저장·복원을 맡는다.
//! listener는 요청을 받고 auth·abuse로 인증과 반복 실패를 검사한다.

pub mod abuse;
pub mod ack;
pub mod auth;
pub mod config;
pub mod lifetime;
pub mod listener;
pub mod persist;
pub mod registry;

pub use auth::{AuthLocation, WebhookAuth, auth_summary};
pub use lifetime::{Lifetime, Limit, Persistence};
pub use registry::{WebhookEntry, info, list, register, sweep, unregister};

use tasty_ipc::host_call::HostIpcInjector;

/// 모든 IPv4 인터페이스에서 수신한다.
const BIND_ADDR: &str = "0.0.0.0";

/// 리스너 초기화 결과. 설정 포트를 사용하며 다른 포트로 재시도하지 않는다.
#[derive(Debug)]
pub enum WebhookInitReport {
    /// bind 표시가 설정됐다. accept 스레드 실행까지 보장하는 결과는 아니다.
    Bound,
    /// 설정 포트가 비어 리스너를 띄우지 않았다.
    PortNotConfigured,
    /// 설정 포트 bind 실패. 세부 값은 GUI 경고에 쓰고 headless에서는 로그만 남긴다.
    BindFailed {
        #[cfg(feature = "gui")]
        port: u16,
        #[cfg(feature = "gui")]
        error: String,
    },
}

impl WebhookInitReport {
    /// GUI에 표시할 경고. Bound면 None이며 headless는 로그를 사용한다.
    #[cfg(feature = "gui")]
    pub fn user_warning(&self) -> Option<String> {
        match self {
            WebhookInitReport::Bound => None,
            WebhookInitReport::PortNotConfigured => {
                Some(crate::i18n::t("webhook.warn.port_not_configured").to_string())
            }
            WebhookInitReport::BindFailed { port, error } => Some(crate::i18n::t_fmt2(
                "webhook.warn.bind_failed",
                &port.to_string(),
                error,
            )),
        }
    }
}

/// 설정과 injector 준비 후 호출하는 GUI·headless 공통 초기화 함수.
/// 포트가 없으면 리스너를 시작하지 않고, 결과 보고서는 호출자가 표시한다.
pub fn init_from_config(injector: HostIpcInjector) -> WebhookInitReport {
    match config::load_or_seed() {
        Some(port) => {
            // 복원 항목의 URL을 만들기 전에 runtime의 주소·포트를 설정한다.
            let report = listener::init(injector, BIND_ADDR, port);
            persist::restore_into_registry();
            report
        }
        None => {
            registry::set_runtime(injector, BIND_ADDR, None);
            tracing::warn!(
                "webhook port not configured; listener not started \
                 (set one via `tasty webhook config --port <N>`)"
            );
            WebhookInitReport::PortNotConfigured
        }
    }
}
