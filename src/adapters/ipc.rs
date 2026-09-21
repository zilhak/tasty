pub mod audit;
pub mod handler;
pub(crate) mod window_port;

pub use tasty_ipc::{alias, caller, port_file, protocol, server, session, stream};
// IPC 클라이언트 — 본체 안에서 부르는 자리(GUI 의 원격 attach·도구)가 gui 뿐이다.
#[cfg(feature = "gui")]
pub use tasty_ipc::client;
