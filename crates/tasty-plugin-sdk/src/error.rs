//! SDK 도메인 에러.
//!
//! [`PluginError`]는 SDK 함수에서 발생할 수 있는 모든 실패 모드를 표현한다.
//! Plugin 작성자는 `?`로 흘려 전파하거나, 분기가 필요할 때 variant로 매칭한다.
//!
//! IPC 응답 표면에서는 [`crate::plugin::IpcMethodError`]를 그대로 쓴다. SDK
//! 내부 에러를 IPC 응답으로 흘릴 때는 `From<PluginError> for IpcMethodError`
//! 변환이 자동으로 동작한다.

use std::time::Duration;

/// SDK 함수의 표준 결과 타입.
pub type Result<T, E = PluginError> = std::result::Result<T, E>;

/// SDK 작업 중 발생할 수 있는 에러.
#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    /// 필수 환경변수가 비어 있다.
    #[error("missing env var: {0}")]
    EnvMissing(&'static str),

    /// 환경변수 파싱 실패 (예: 포트 번호가 숫자 아님).
    #[error("invalid env var {var}: {message}")]
    EnvParse { var: &'static str, message: String },

    /// 호스트 TCP 포트에 connect 실패.
    #[error("failed to connect to host on port {port}: {source}")]
    Connect {
        port: u16,
        #[source]
        source: std::io::Error,
    },

    /// 호스트가 connection을 닫음.
    #[error("host closed connection")]
    HostClosed,

    /// 호스트 호출 응답에 호스트가 명시한 에러.
    #[error("host call '{method}' failed: {message}")]
    HostCall { method: String, message: String },

    /// 호스트 호출이 timeout 안에 응답을 받지 못함.
    #[error("host call '{method}' timed out after {timeout:?}")]
    HostCallTimeout { method: String, timeout: Duration },

    /// 일반 IO 에러.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// JSON 인코딩/디코딩 실패.
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    /// Mutex가 poison된 상태에서 lock 시도.
    #[error("mutex poisoned: {0}")]
    LockPoisoned(&'static str),
}
