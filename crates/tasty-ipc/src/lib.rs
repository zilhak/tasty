#![forbid(unsafe_code)]

//! 호스트와 CLI·에이전트·플러그인이 공유하는 IPC 전송 형식과 호출자 모델.
//! 도메인 핸들러와 실제 서버 adapter는 호스트 본체가 구현한다.

pub mod admission;
pub mod alias;
pub mod caller;
pub mod capability;
pub mod client;
pub mod dispatch;
pub mod host_call;
pub mod host_port;
pub mod ipc_namespace;
pub mod mesh_stream;
pub mod method_meta;
pub mod output_cursor;
pub mod port_file;
pub mod protocol;
pub mod server;
pub mod session;
pub mod stream;
pub mod stream_hub;

pub use host_port::{AuditCallerMarker, AuditDecision, IpcHostFacade, SessionResolution};
