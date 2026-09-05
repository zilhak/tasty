// 이유: IPC 인프라 (auth/audit/세션 토큰/JSON-RPC framing) 은 handler 트리와 연동.
// headless 빌드에선 handler 호출자가 cfg(gui) 차단되어 일부 인프라 함수도
// 미사용. library standard — *headless 한정* dead_code/unused_imports 침묵.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

pub mod audit;
pub mod handler;
pub mod host_call;
pub mod log_retention;

pub use tasty_ipc::{alias, caller, client, port_file, protocol, server, session, stream};
