//! 워커 스레드 ↔ 메인 스레드 마샬링 타입 (ADR-0031).
//!
//! Lua 워커는 메인 소유 state 를 절대 직접 만지지 않는다. 유일한 통로는:
//! - **읽기** = 메인이 발행한 불변 [`LuaSnapshot`] 를 워커가 읽는다.
//! - **쓰기** = 워커가 [`HostCommand`] 를 큐로 보내고 메인이 안전지점에서 적용한다.
//!
//! 두 타입 모두 GUI 비의존 (egui/wgpu 참조 없음) 이라 크로스스레드로 안전하게 오간다.

use std::sync::{Arc, Mutex};

/// 메인이 발행하는 읽기전용 스냅샷. read API (`tasty.tree` 등) 가 참조한다.
///
/// 프레임 안전지점(`about_to_wait`)에서 메인이 최신 값을 발행하고, 워커는 그
/// 시점 스냅샷을 읽는다 — 실시간이 아니라 프레임 경계 스냅샷이다 (ADR-0031 Consequences).
#[derive(Debug, Default, Clone)]
pub struct LuaSnapshot {
    /// `handle_tree`(IPC/CLI `list tree`) 와 동형인 워크스페이스 트리.
    /// TODO 02 에서 메인이 채우고 `tasty.tree` 가 소비한다.
    pub tree: Vec<serde_json::Value>,
}

/// 메인이 발행하고 워커가 읽는 스냅샷 핸들. 발행 = `Arc` 통째 교체(lock 은 극히 짧게).
pub type SharedSnapshot = Arc<Mutex<Arc<LuaSnapshot>>>;

/// 워커 Lua 가 메인 스레드에 요청하는 mutation 커맨드.
///
/// 메인이 안전지점(`about_to_wait`)에서 drain·적용한다. mutation 호스트 API 가
/// 늘어나면 variant 를 추가한다 (ADR-0031: "새 mutation API 마다 커맨드 variant + 메인 적용 지점").
#[derive(Debug, Clone)]
pub enum HostCommand {
    /// tasty 자기 CLI 를 서브프로세스로 실행. `tasty.run_cli(args)` 가 발행한다.
    ///
    /// 프로세스 spawn 은 부수효과이므로 워커에서 직접 하지 않고 메인 커맨드 큐를
    /// 경유한다 — ADR-0031 "쓰기는 커맨드로 직렬화해 메인 스레드 큐로". 워커 스레드를
    /// 순수 계산 전용으로 유지하는 효과도 있다.
    RunCli(Vec<String>),
}
