//! Lua 워커는 호스트 상태를 직접 바꾸지 않는다. 메인이 발행한 LuaSnapshot을 읽고,
//! 변경 요청은 HostCommand 큐에 넣어 메인이 처리한다. 두 타입은 GUI에 의존하지 않는다.

use std::sync::{Arc, Mutex};

/// 메인이 발행하는 읽기 전용 스냅샷. tasty.tree는 발행 시점의 값을 읽는다.
#[derive(Debug, Default, Clone)]
pub struct LuaSnapshot {
    /// handle_tree와 같은 형식의 workspace 트리.
    pub tree: Vec<serde_json::Value>,
}

/// 메인이 발행하고 워커가 읽는 스냅샷 핸들. 발행 = `Arc` 통째 교체(lock 은 극히 짧게).
pub type SharedSnapshot = Arc<Mutex<Arc<LuaSnapshot>>>;

/// 워커의 변경 요청. 메인 스레드가 큐에서 꺼내 처리한다.
#[derive(Debug, Clone)]
pub enum HostCommand {
    /// tasty.run_cli의 CLI 실행 요청. 실제 spawn은 메인 스레드가 맡는다.
    RunCli(Vec<String>),
}
