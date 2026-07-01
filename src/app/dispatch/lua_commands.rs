//! Lua 워커가 발행한 [`HostCommand`] drain → 메인 스레드에서 적용 (ADR-0031).
//!
//! 워커는 메인 소유 state 를 직접 못 만지므로, mutation/부수효과는 커맨드로
//! 직렬화해 큐에 넣는다. 이 모듈이 프레임 안전지점(`about_to_wait`)에서 적용한다.

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

    /// 스크립트 TOFU 변경 확인 팝업(06)의 결정 슬롯 drain.
    ///
    /// popup wrapper 가 `pending_script_confirm.result` 를 채우면 frame begin 에 검사 —
    /// `true` 면 레지스트리 해시를 `new_hash` 로 갱신·영속(config.toml)하고 워커에서 실행,
    /// `false`/Esc 면 폐기. md_open drain 과 동일 패턴.
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
            // 승인: 레지스트리 해시 갱신 + 영속.
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
            // 워커에서 실행(이미 읽은 source 그대로 — 재읽기 없음).
            if let Some(engine) = self.lua_engine.as_ref() {
                engine.run_script(&pending.source, Some(&pending.name));
            }
        }
    }
}
