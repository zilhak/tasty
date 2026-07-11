//! 인바운드 웹훅 리스너 (외부 HTTP 트리거).
//!
//! **싱글턴 라우터** — 프로세스당 단 하나의 리스너가 다수의 웹훅 등록을 opaque
//! path 로 멀티플렉싱한다. 개별 웹훅은 port/path 를 지정하지 못하고 리스너가
//! 짧은해시 id 를 발급·은닉한다. 응답은 **단방향 ACK 전용**([`ack`]).
//!
//! - [`registry`] — 등록 상태(전역 싱글턴) + register/list/info/unregister/sweep.
//! - [`lifetime`] — {영속성}×{제한} = 6종 lifetime + lazy 만료 판정.
//! - [`persist`] — `Persistent` 웹훅의 `~/.tasty/webhooks.toml` 영속화·재시작 복원.
//! - [`listener`] — tiny_http bind + accept + 요청 라우팅.
//! - [`ack`] — 단방향 ACK 빌더(실행 결과 미접근, 타입 강제).
//! - [`auth`] — 웹훅별 **선택적** 인증(고정 토큰, 미설정 시 무인증 통과).
//!
//! 현재 범위: 단일 포트 + opaque path + **lifetime 6종 + 영속화 + lazy 만료 +
//! sweep** + **선택적 인증(S6)** + 기본 IpcSequence 실행. 남용차단·포트설정 UI 는
//! 후속(S7~S8).

pub mod abuse;
pub mod ack;
pub mod auth;
pub mod config;
pub mod lifetime;
pub mod listener;
pub mod persist;
pub mod registry;

pub use auth::{AuthLocation, WebhookAuth, auth_summary};
// RegisterOutcome 는 registry::register 반환 타입으로 내부 소비 — 전체 경로로 접근.
pub use lifetime::{Lifetime, Limit, Persistence};
pub use registry::{WebhookEntry, info, list, register, sweep, unregister};

use crate::adapters::ipc::host_call::HostIpcInjector;

/// bind 주소 — `0.0.0.0`(모든 인터페이스, 공유기 포워딩 수신). OS 무관.
const BIND_ADDR: &str = "0.0.0.0";

/// 웹훅 리스너 부팅 결과 — caller 가 경고를 UI(toast)/로그로 노출하는 데 쓴다.
///
/// 포트는 **설정값 only**(자동 폴백 bind 없음, S8). 비었거나 bind 실패면 리스너를
/// 띄우지 않고 그 사실을 사용자에게 알린다.
#[derive(Debug)]
pub enum WebhookInitReport {
    /// 리스너가 설정 포트에 bind 됨(또는 이미 bind 되어 있었음). 경고 없음.
    Bound,
    /// 설정 포트가 비어 리스너를 띄우지 않았다.
    PortNotConfigured,
    /// 설정된 포트가 충돌/권한 등으로 bind 실패했다(자동 회피 없음).
    BindFailed { port: u16, error: String },
}

impl WebhookInitReport {
    /// 사용자에게 보여줄 경고 문자열(i18n). `Bound` 면 경고 없음(`None`).
    ///
    /// GUI 는 이 값을 toast(`ToastManager`)로, headless 는 이미 `tracing::warn!`
    /// 으로 노출한다(중복 로그 방지 위해 headless 는 이 문자열을 다시 찍지 않음).
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

/// 부팅 공용 헬퍼 — GUI(`window_lifecycle`) + headless(`boot`) 양쪽에서 호출한다.
///
/// 전제: core config 로드 완료 + 메인루프 IPC 처리 가능(injector 확보 직후). 포트는
/// **설정값 only**([`config::load_or_seed`], 자동 폴백 없음). 파일이 처음 없으면
/// 시드 [`config::SEED_PORT`] 를 기록하고, 사용자가 포트를 비우면 리스너를 띄우지
/// 않는다. 중복 호출은 리스너 내부 bind 가드로 무해하다.
///
/// 반환 [`WebhookInitReport`] 를 caller 가 toast/로그로 노출한다.
pub fn init_from_config(injector: HostIpcInjector) -> WebhookInitReport {
    match config::load_or_seed() {
        Some(port) => {
            // 리스너 runtime(injector/addr/port) 을 먼저 세팅해야 복원 엔트리의 URL
            // 표기가 정확하다. init 내부의 set_runtime 이 이를 처리하므로 복원 전에
            // 호출된다.
            let report = listener::init(injector, BIND_ADDR, port);
            // 재시작 복원 + 필터 — 이미 만료된 Persistent 웹훅은 등록하지 않고 정리한다.
            persist::restore_into_registry();
            report
        }
        None => {
            // 포트 미설정 — bind 하지 않되 URL 표기용 상태는 잡아 둔다(port None).
            registry::set_runtime(injector, BIND_ADDR, None);
            tracing::warn!(
                "webhook port not configured; listener not started \
                 (set one via `tasty webhook config --port <N>`)"
            );
            WebhookInitReport::PortNotConfigured
        }
    }
}
