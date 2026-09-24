#![forbid(unsafe_code)]

//! 등록된 사용자 Lua 스크립트를 전용 워커에서 실행한다. init.lua를 자동으로 읽지는 않는다.
//! 호스트 API는 정해진 함수만 제공하며 이벤트 hook의 반환값은 호스트 동작을 취소하지 못한다.
//!
//! 스크립트는 사용자의 OS 권한으로 실행되며 io/os.execute를 제한하지 않는다.
//! 신뢰할 수 없는 코드를 격리하는 보안 sandbox가 아니다. 메모리 상한과 Lua 명령 hook의
//! 실행 기한을 적용하고 debug·bytecode 로더·package.loadlib를 제거한다.
//! 외부 작업에는 tasty.run_cli 같은 호스트 API를 우선 사용한다.

mod bridge;
mod engine;
mod host_api;
mod sandbox;

pub use bridge::{HostCommand, LuaSnapshot};
pub use engine::{CompletionToken, LuaEngine, LuaEngineError};
pub use host_api::run_tasty_cli;
