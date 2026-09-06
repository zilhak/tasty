#![forbid(unsafe_code)]

//! Tasty 호스트와 외부 caller (CLI / agent / plugin) 간 IPC wire framing 과
//! caller-context 모델.
//!
//! 본 바이너리 `src/adapters/ipc/` 의 wire/framing/method 모듈을 이 crate 로
//! 이동했다. handler (`crate::adapters::ipc::handler`) 는 본 바이너리에 잔존
//! (AppState/Core 결합 깊음).

pub mod alias;
pub mod caller;
pub mod client;
pub mod host_port;
pub mod ipc_namespace;
pub mod mesh_stream;
pub mod method_meta;
pub mod port_file;
pub mod protocol;
pub mod server;
pub mod session;
pub mod stream;

// 테스트는 method_meta.rs / session.rs 에서 각각 *_tests.rs 를 로드 (co-located).

pub use host_port::{AuditCallerMarker, AuditDecision, IpcHostFacade, SessionResolution};
