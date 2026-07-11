//! 인바운드 웹훅 리스너 (외부 HTTP 트리거).
//!
//! **싱글턴 라우터** — 프로세스당 단 하나의 리스너가 다수의 웹훅 등록을 opaque
//! path 로 멀티플렉싱한다. 개별 웹훅은 port/path 를 지정하지 못하고 리스너가
//! 짧은해시 id 를 발급·은닉한다. 응답은 **단방향 ACK 전용**([`ack`]).
//!
//! - [`registry`] — 등록 상태(전역 싱글턴) + register/list/info/unregister.
//! - [`listener`] — tiny_http bind + accept + 요청 라우팅.
//! - [`ack`] — 단방향 ACK 빌더(실행 결과 미접근, 타입 강제).
//!
//! MVP 범위: 단일 포트 + opaque path + Temporary/Unlimited lifetime + 기본
//! IpcSequence 실행. lifetime 6종·영속화·인증·남용차단·포트설정 UI 는 후속.

pub mod ack;
pub mod listener;
pub mod registry;

pub use registry::{RegisterOutcome, WebhookEntry, info, list, register, unregister};

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
    listener::init(injector, "0.0.0.0", port);
}
