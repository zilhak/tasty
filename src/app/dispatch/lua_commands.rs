//! Lua 워커의 HostCommand를 about_to_wait에서 메인 스레드 상태에 적용한다.

use crate::adapters::ipc::handler::build_engine_tree;
use crate::app::App;
use crate::view::ui::View;
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

    /// 모든 MainView·parked workspace의 읽기 스냅샷을 발행한다.
    /// 매번 전체 트리를 다시 만들므로 큰 트리에서는 비용을 확인해야 한다.
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

    /// 스크립트 변경을 승인하면 해시 저장을 시도하고 이미 읽은 소스를 실행한다.
    /// 저장 실패는 경고만 남기며 실행을 막지 않는다.
    pub(crate) fn dispatch_pending_script_confirm(&mut self) {
        use winit::window::WindowId;
        let ids: Vec<WindowId> = self
            .view
            .views
            .iter()
            .filter_map(|(id, w)| {
                let main = w.as_main()?;
                let data = main.state.dialogs.pending_script_confirm.as_ref()?;
                data.result.map(|_| *id)
            })
            .collect();
        for id in ids {
            let Some(pending) = self
                .view
                .views
                .get_mut(&id)
                .and_then(|w| w.as_main_mut())
                .and_then(|m| m.state.dialogs.pending_script_confirm.take())
            else {
                continue;
            };
            if pending.result != Some(true) {
                continue; // 취소 — 폐기(이미 take 됨).
            }
            if let Some(main) = self.view.views.get_mut(&id).and_then(|w| w.as_main_mut()) {
                main.core_state
                    .settings
                    .scripts
                    .update_hash(&pending.script_id, pending.new_hash.clone());
                if let Err(e) = main.core_state.settings.save() {
                    tracing::warn!(target: "tasty_lua", "script hash persist failed: {e}");
                }
                main.mark_dirty();
            }
            // 승인한 내용과 실행할 내용이 달라지지 않도록 파일을 다시 읽지 않는다.
            if let Some(engine) = self.lua_engine.as_ref() {
                engine.run_script(&pending.source, Some(&pending.name));
            }
        }
    }
}
