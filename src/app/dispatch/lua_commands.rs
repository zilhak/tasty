//! Lua 워커의 HostCommand를 about_to_wait에서 메인 스레드 상태에 적용한다.

use crate::adapters::ipc::handler::build_engine_tree;
use crate::app::App;
use crate::app::window_access::engines_mut;
use crate::view::ui::View;
use tasty_lua::{HostCommand, LuaSnapshot};
use winit::window::WindowId;

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
        for (window, mut wait, source, name) in std::mem::take(&mut self.scripts_after_save) {
            match wait.poll() {
                crate::app::settings_change::SettingsPoll::Pending => {
                    self.scripts_after_save.push((window, wait, source, name));
                }
                _ => self.submit_shortcut_script(window, &source, name),
            }
        }
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
            // 저장할 때 이 해시를 지우므로 모든 사본에 넣은 뒤 승인한 윈도우의 사본을 저장한다.
            let approver = self.engines.of_window(id);
            let (mut mine, mut others) = (None, Vec::new());
            for session in self.engines.all_sessions_mut() {
                if Some(session.id) == approver {
                    mine = Some(&mut session.runtime.settings);
                } else {
                    others.push(&mut session.runtime.settings);
                }
            }
            let snapshot = record_script_hash(mine, others, &pending.script_id, &pending.new_hash);
            let (done, wait) = crate::app::settings_change::pair(self.services.reply_wake());
            if let Some(snapshot) = snapshot {
                self.services.submit_settings_save(
                    snapshot,
                    Some(crate::intent::IntentOrigin::System),
                    Some(done),
                );
                self.scripts_after_save.push((
                    id,
                    wait,
                    pending.source.clone(),
                    pending.name.clone(),
                ));
            } else {
                self.submit_shortcut_script(id, &pending.source, pending.name.clone());
            }
            if let Some((main, _)) = engines_mut!(self).window_pair(id) {
                main.mark_dirty();
            }
            // Keep the approved source in the continuation; never re-read it after saving.
        }
    }

    /// 사용자가 시작한 스크립트 실행을 요청한다. 실행하지 못하면 요청한 창에 실패 토스트를 띄운다.
    pub(crate) fn submit_shortcut_script(&mut self, window: WindowId, source: &str, name: String) {
        use crate::hooks::lua_runs::{RunOrigin, ShortcutFailure};
        let submitted = match self.lua_engine.as_ref() {
            Some(engine) => engine.submit_script(source, Some(&name), None),
            None => Err(tasty_lua::RunRejected::WorkerGone),
        };
        match submitted {
            Ok(ticket) => self
                .lua_runs
                .record(ticket, RunOrigin::Shortcut { window, name }),
            Err(rejected) => {
                tracing::warn!(target: "tasty_lua", "script '{name}' not queued: {rejected:?}");
                self.toast_script_failure(window, &name, &ShortcutFailure::Rejected(rejected));
            }
        }
    }

    /// 워커가 돌려준 실행 결과를 시작 경로로 보낸다. 자동실행 실패는 알림 채널, 단축키 실패는 원 창의 토스트다.
    pub(crate) fn settle_lua_runs(&mut self) {
        let Some(engine) = self.lua_engine.as_ref() else {
            return;
        };
        let outcomes = engine.drain_run_outcomes();
        if outcomes.is_empty() {
            return;
        }
        let settled = self.lua_runs.settle(outcomes);
        for notice in settled.notices {
            crate::system_notice::post(notice);
        }
        for (origin, failure) in settled.for_views {
            if let crate::hooks::lua_runs::RunOrigin::Shortcut { window, name } = origin {
                self.toast_script_failure(
                    window,
                    &name,
                    &crate::hooks::lua_runs::ShortcutFailure::Run(&failure),
                );
            }
        }
    }

    /// 원 창이 닫혔으면 다른 창에 옮기지 않고 버린다.
    fn toast_script_failure(
        &mut self,
        window: WindowId,
        name: &str,
        failure: &crate::hooks::lua_runs::ShortcutFailure<'_>,
    ) {
        let Some(main) = self
            .view
            .views
            .get_mut(&window)
            .and_then(|w| w.as_main_mut())
        else {
            tracing::debug!(target: "tasty_lua", "script '{name}' result arrived after its window closed");
            return;
        };
        main.state.toasts.push(
            crate::hooks::lua_runs::shortcut_failure_text(name, failure),
            crate::adapters::ui::ToastKind::Error,
            crate::adapters::ui::ToastScope::Window,
        );
        main.mark_dirty();
    }
}

/// 승인한 해시를 모든 설정 사본에 넣고 승인한 윈도우의 사본(`approver`)을 저장한다.
/// 승인한 윈도우의 engine 을 찾지 못하면 저장하지 않는다.
fn record_script_hash<'a>(
    approver: Option<&mut crate::settings::Settings>,
    others: impl IntoIterator<Item = &'a mut crate::settings::Settings>,
    script_id: &str,
    hash: &str,
) -> Option<crate::settings::Settings> {
    let observation = approver.as_ref().and_then(|s| s.file_observation.clone());
    for settings in others {
        settings.scripts.update_hash(script_id, hash.to_string());
        settings.file_observation = observation.clone();
    }
    match approver {
        Some(settings) => {
            settings.scripts.update_hash(script_id, hash.to_string());
            Some(settings.clone())
        }
        None => None,
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
        super::record_script_hash(Some(&mut first), [&mut second], &one, "hash-one")
            .expect("snapshot")
            .save()
            .expect("save");
        super::record_script_hash(Some(&mut second), [&mut first], &two, "hash-two")
            .expect("snapshot")
            .save()
            .expect("save");

        let restarted = Settings::load();
        let hash = |id: &str| restarted.scripts.get(id).map(|entry| entry.sha256.clone());
        assert_eq!(hash(&one).as_deref(), Some("hash-one"));
        assert_eq!(hash(&two).as_deref(), Some("hash-two"));
        assert_eq!(
            first.scripts.get(&two).map(|e| e.sha256.as_str()),
            Some("hash-two")
        );
    }

    /// 사본이 갈라졌으면 승인한 윈도우의 사본을 저장한다.
    #[test]
    fn the_approving_windows_copy_is_saved() {
        let _home = crate::test_support::IsolatedHome::new();
        let (mut approver, [one, _]) = with_scripts();
        let (mut other, _) = with_scripts();
        let only_here = approver
            .scripts
            .add("three".into(), "/three.lua".into(), String::new());
        super::record_script_hash(Some(&mut approver), [&mut other], &one, "hash-one")
            .expect("snapshot")
            .save()
            .expect("save");
        let restarted = Settings::load();
        assert!(restarted.scripts.get(&only_here).is_some());
        assert_eq!(
            restarted.scripts.get(&one).map(|e| e.sha256.as_str()),
            Some("hash-one")
        );
    }
}
