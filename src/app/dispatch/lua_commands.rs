//! Lua 워커가 발행한 [`HostCommand`] drain → 메인 스레드에서 적용 (ADR-0031).
//!
//! 워커는 메인 소유 state 를 직접 못 만지므로, mutation/부수효과는 커맨드로
//! 직렬화해 큐에 넣는다. 이 모듈이 프레임 안전지점(`about_to_wait`)에서 적용한다.

use crate::app::App;
use tasty_lua::HostCommand;

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
}
