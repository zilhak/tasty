//! Platform-specific implementations.
//!
//! 각 모듈은 `create / prepare_send / receive`를 노출하고 상위 lib.rs가 cfg-gated로
//! 재수출한다. 모듈 내에선 platform-specific RAII 타입을 정의해 `SharedMemory` /
//! `SendableHandle` / `TransportPayload`의 `inner` 필드로 들어간다.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::{
    PlatformMapping, PlatformPayload, PlatformSendable, create, prepare_send, receive,
};
#[cfg(target_os = "macos")]
pub(crate) use macos::{
    PlatformMapping, PlatformPayload, PlatformSendable, create, prepare_send, receive,
};
#[cfg(windows)]
pub(crate) use windows::{
    PlatformMapping, PlatformPayload, PlatformSendable, create, prepare_send, receive,
};

/// 모든 플랫폼이 따르는 최대 크기 상한 (8 GB).
pub(crate) const MAX_SIZE: usize = 8 * 1024 * 1024 * 1024;
