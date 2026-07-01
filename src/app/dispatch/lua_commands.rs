//! Lua 워커가 발행한 [`HostCommand`] drain → 메인 스레드에서 적용 (ADR-0031).
//!
//! 워커는 메인 소유 state 를 직접 못 만지므로, mutation/부수효과는 커맨드로
//! 직렬화해 큐에 넣는다. 이 모듈이 프레임 안전지점(`about_to_wait`)에서 적용한다.

use crate::adapters::ipc::handler::build_engine_tree;
use crate::app::App;
use tasty_lua::{HostCommand, LuaSnapshot};

impl App {
    pub(crate) fn dispatch_pending_lua_commands(&mut self) {
        let Some(engine) = self.lua_engine.as_ref() else {
            return;
        };
        for cmd in engine.drain_commands() {
            match cmd {
                HostCommand::RunCli(args) => tasty_lua::run_tasty_cli(&args),
            }
        }
    }

    /// 읽기전용 트리 스냅샷을 워커에 발행한다 (ADR-0031 읽기 = 스냅샷).
    ///
    /// 범위 = **전 View(main) + parked 워크스페이스 통합** — focus 독립 원칙상
    /// `tasty.tree()` 는 활성 창과 무관하게 전체를 반영한다 (list_global 순회 기준과 정합).
    /// per-engine 빌더는 IPC `list tree` 와 공유해 구조 드리프트를 막는다.
    ///
    /// NOTE: 안전지점(`about_to_wait`)마다 트리 JSON 을 재빌드한다. 트리 규모가 커져
    /// 프레임 예산을 침해하면 증분/lazy 발행으로 재검토(ADR-0031 Reconsideration Triggers).
    pub(crate) fn publish_lua_snapshot(&self) {
        let Some(engine) = self.lua_engine.as_ref() else {
            return;
        };
        let mut tree = Vec::new();
        for w in self.view.views.values() {
            if let Some(m) = w.as_main() {
                tree.extend(build_engine_tree(&m.state, &m.core_state));
            }
        }
        for (s, e) in &self.parked_states {
            tree.extend(build_engine_tree(s, e));
        }
        engine.publish_snapshot(LuaSnapshot { tree });
    }
}
