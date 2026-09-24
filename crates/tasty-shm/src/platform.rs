//! 플랫폼별 매핑과 핸들 수명 관리. create/prepare_send/receive를 공통 API에 제공한다.

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

/// 모든 플랫폼에 적용하는 크기 상한(8 GiB).
pub(crate) const MAX_SIZE: usize = 8 * 1024 * 1024 * 1024;
