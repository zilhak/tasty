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
pub mod lifetime;
pub mod listener;
pub mod persist;
pub mod registry;

pub use auth::{AuthLocation, WebhookAuth, auth_summary};
// RegisterOutcome 는 registry::register 반환 타입으로 내부 소비 — 전체 경로로 접근.
pub use lifetime::{Lifetime, Limit, Persistence};
pub use registry::{WebhookEntry, info, list, register, sweep, unregister};

use crate::adapters::ipc::host_call::HostIpcInjector;

/// MVP 기본 웹훅 포트 — 임의 시드값(IANA 미등록, User Ports 범위). 설정 UI(S8)
/// 도입 전까지 이 값 또는 env `TASTY_WEBHOOK_PORT` 로 결정한다.
pub const DEFAULT_WEBHOOK_PORT: u16 = 28429;

/// 부팅 공용 헬퍼 — GUI(`window_lifecycle`) + headless(`boot`) 양쪽에서 호출한다.
///
/// 전제: core config 로드 완료 + 메인루프 IPC 처리 가능(injector 확보 직후). 포트는
/// 설정값 only(자동 폴백 없음). bind 는 `0.0.0.0`(공유기 포워딩 수신). 중복 호출은
/// 리스너 내부 bind 가드로 무해하다.
pub fn init_from_config(injector: HostIpcInjector) {
    let port = std::env::var("TASTY_WEBHOOK_PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(DEFAULT_WEBHOOK_PORT);
    // 리스너 runtime(injector/addr/port) 을 먼저 세팅해야 복원 엔트리의 URL 표기가
    // 정확하다. init 내부의 set_runtime 이 이를 처리하므로 bind 전에 호출된다.
    listener::init(injector, "0.0.0.0", port);
    // 재시작 복원 + 필터 — 이미 만료된 Persistent 웹훅은 등록하지 않고 정리한다.
    persist::restore_into_registry();
}
