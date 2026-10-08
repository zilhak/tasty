//! Lua 워커의 HostCommand를 about_to_wait에서 메인 스레드 상태에 적용한다.

use crate::adapters::ipc::handler::build_engine_tree;
use crate::app::App;
use crate::app::window_access::engines_mut;
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
        for (s, e) in self.engines().sessions() {
            tree.extend(build_engine_tree(
                &s.navigation,
                s.active_workspace_index(e.core),
                &e.read(),
            ));
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
            // 설정은 engine 마다 같은 사본을 둔다. 승인한 윈도우의 사본만 바꾸면 다른 윈도우가 자기 사본을
            // 저장할 때 이 해시를 지우므로 모든 사본에 넣은 뒤 저장한다.
            let copies = self
                .engines
                .all_sessions_mut()
                .map(|session| &mut session.runtime.settings);
            if let Err(e) = record_script_hash(copies, &pending.script_id, &pending.new_hash) {
                tracing::warn!(target: "tasty_lua", "script hash persist failed: {e}");
            }
            if let Some((main, _)) = engines_mut!(self).window_pair(id) {
                main.mark_dirty();
            }
            // 승인한 내용과 실행할 내용이 달라지지 않도록 파일을 다시 읽지 않는다.
            if let Some(engine) = self.lua_engine.as_ref() {
                engine.run_script(&pending.source, Some(&pending.name));
            }
        }
    }
}

/// 승인한 해시를 모든 설정 사본에 넣고 그중 하나를 저장한다. 사본이 없으면 저장하지 않는다.
fn record_script_hash<'a>(
    copies: impl IntoIterator<Item = &'a mut crate::settings::Settings>,
    script_id: &str,
    hash: &str,
) -> anyhow::Result<()> {
    let mut saved = None;
    for settings in copies {
        settings.scripts.update_hash(script_id, hash.to_string());
        saved = Some(settings);
    }
    match saved {
        Some(settings) => settings.save(),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::settings::Settings;

    /// 두 스크립트를 해시 없이 등록한 사본과 그 id.
    fn with_scripts() -> (Settings, [String; 2]) {
        let mut settings = Settings::default();
        let ids = ["one", "two"].map(|name| {
            settings
                .scripts
                .add(name.into(), format!("/{name}.lua").into(), String::new())
        });
        (settings, ids)
    }

    /// 윈도우 둘이 각자 다른 스크립트를 승인해도 두 해시가 모두 저장된다.
    #[test]
    fn script_hashes_approved_in_two_windows_both_survive_a_restart() {
        let _home = crate::test_support::IsolatedHome::new();
        let (mut first, [one, two]) = with_scripts();
        let (mut second, _) = with_scripts();
        super::record_script_hash([&mut first, &mut second], &one, "hash-one").expect("save");
        super::record_script_hash([&mut second, &mut first], &two, "hash-two").expect("save");

        let restarted = Settings::load();
        let hash = |id: &str| restarted.scripts.get(id).map(|entry| entry.sha256.clone());
        assert_eq!(hash(&one).as_deref(), Some("hash-one"));
        assert_eq!(hash(&two).as_deref(), Some("hash-two"));
        assert_eq!(
            first.scripts.get(&two).map(|e| e.sha256.as_str()),
            Some("hash-two")
        );
    }
}
