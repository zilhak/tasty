//! 이벤트에 연결된 스크립트를 읽고 등록 해시와 같을 때 실행 요청을 보낸다.
//! 변경·읽기 실패·제출 거절·억제는 로그와 경고 알림을 남기고 건너뛴다. 자동 승인이나 확인 팝업은 만들지 않는다.
//! 실행 중과 완료 직후의 재진입을 억제하지만 지연된 이벤트 연쇄 전체를 차단한다고 보장하지 않는다.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
#[cfg(any(feature = "gui", test))]
use std::sync::atomic::Ordering;

use tasty_settings::ScriptRegistry;

use super::lua_runs::{AutofireProblem, LuaRuns, RunOrigin, autofire_notice};
use crate::system_notice::SystemNotice;

/// 완료 카운터를 다음 두 checkpoint에 걸쳐 반영한다.
/// 완료 직후 남은 이벤트가 곧바로 같은 스크립트를 다시 실행하지 않도록 지연한다.
pub(crate) struct AutofireGuard {
    submitted: u64,
    /// 재진입 억제 판단에 반영한 완료 수.
    acknowledged: u64,
    /// 이전 checkpoint에서 읽은 완료 수.
    #[cfg(any(feature = "gui", test))]
    prev_sample: u64,
    /// worker의 CompletionToken drop으로 증가한다.
    completed: Arc<AtomicU64>,
}

impl AutofireGuard {
    pub(crate) fn new() -> Self {
        Self {
            submitted: 0,
            acknowledged: 0,
            #[cfg(any(feature = "gui", test))]
            prev_sample: 0,
            completed: Arc::new(AtomicU64::new(0)),
        }
    }

    fn suppressed(&self) -> bool {
        self.submitted > self.acknowledged
    }

    /// 이전 표본을 완료 수로 인정하고 현재 값을 다음 호출에 쓸 표본으로 보관한다.
    /// 렌더링 프레임 완료나 모든 이벤트의 처리가 끝났음을 확인하는 함수는 아니다.
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn checkpoint(&mut self) {
        self.acknowledged = self.prev_sample;
        self.prev_sample = self.completed.load(Ordering::SeqCst);
    }

    fn note_submitted(&mut self) {
        self.submitted += 1;
    }

    fn token(&self) -> tasty_lua::CompletionToken {
        tasty_lua::CompletionToken::new(self.completed.clone())
    }
}

fn display_name(entry: &tasty_settings::ScriptEntry) -> String {
    if entry.name.is_empty() {
        entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.id.clone())
    } else {
        entry.name.clone()
    }
}

/// 등록 해시와 같은 소스를 읽는다. 읽지 못했거나 바뀌었으면 경고 알림을 돌려준다.
fn read_approved(
    event: &str,
    name: &str,
    entry: &tasty_settings::ScriptEntry,
) -> Result<String, SystemNotice> {
    let source = std::fs::read_to_string(&entry.path).map_err(|e| {
        tracing::warn!(
            target: "tasty_lua",
            "autofire '{event}': script read failed {}: {e}",
            entry.path.display()
        );
        autofire_notice(event, name, AutofireProblem::ReadFailed(&e.to_string()))
    })?;
    if tasty_settings::hash_bytes(source.as_bytes()) != entry.sha256 {
        tracing::warn!(
            target: "tasty_lua",
            "autofire '{event}': '{}' blocked — file changed since registration \
             (re-approve in Settings › Misc › Scripts)",
            entry.name
        );
        return Err(autofire_notice(event, name, AutofireProblem::Changed));
    }
    Ok(source)
}

/// 실행하지 못했으면 경고 알림을 돌려준다. 실행 결과는 나중에 `LuaRuns`로 돌아온다.
fn try_run_entry(
    lua: &tasty_lua::LuaEngine,
    guard: &mut AutofireGuard,
    runs: &mut LuaRuns,
    event: &str,
    entry: &tasty_settings::ScriptEntry,
) -> Option<SystemNotice> {
    let name = display_name(entry);
    let source = match read_approved(event, &name, entry) {
        Ok(source) => source,
        Err(notice) => return Some(notice),
    };
    match lua.submit_script(&source, Some(&name), Some(guard.token())) {
        Ok(ticket) => {
            guard.note_submitted();
            let event = event.to_string();
            runs.record(ticket, RunOrigin::Autofire { event, name });
            None
        }
        Err(rejected) => {
            tracing::warn!(target: "tasty_lua", "autofire '{event}': '{name}' not queued: {rejected:?}");
            Some(autofire_notice(
                event,
                &name,
                AutofireProblem::Rejected(rejected),
            ))
        }
    }
}

/// 같은 이벤트에 연결된 여러 스크립트는 함께 요청한다. 억제 여부는 반복문에 들어가기 전에 한 번만 본다.
/// 실행하지 못한 스크립트마다, 억제했으면 이벤트당 하나의 경고 알림을 돌려준다.
pub(crate) fn dispatch(
    lua: Option<&tasty_lua::LuaEngine>,
    scripts: &ScriptRegistry,
    guard: &mut AutofireGuard,
    runs: &mut LuaRuns,
    event: &str,
) -> Vec<SystemNotice> {
    if scripts.entries_for_event(event).next().is_none() {
        return Vec::new();
    }
    let Some(lua) = lua else {
        tracing::warn!(target: "tasty_lua", "autofire '{event}' skipped — lua engine unavailable");
        return scripts
            .entries_for_event(event)
            .map(|entry| {
                autofire_notice(
                    event,
                    &display_name(entry),
                    AutofireProblem::Rejected(tasty_lua::RunRejected::WorkerGone),
                )
            })
            .collect();
    };
    if guard.suppressed() {
        tracing::warn!(
            target: "tasty_lua",
            "autofire '{event}' suppressed until the completion checkpoints advance"
        );
        return vec![super::lua_runs::autofire_suppressed_notice(event)];
    }
    scripts
        .entries_for_event(event)
        .filter_map(|entry| try_run_entry(lua, guard, runs, event, entry))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_script(tag: &str, content: &str) -> (PathBuf, String) {
        let dir = std::env::temp_dir().join(format!("tasty-autofire-{}-{tag}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.lua");
        std::fs::write(&path, content).unwrap();
        (path, tasty_settings::hash_bytes(content.as_bytes()))
    }

    fn registry_with(path: PathBuf, sha256: String, event: &str) -> ScriptRegistry {
        let mut reg = ScriptRegistry::default();
        let id = reg.add("t".into(), path, sha256);
        reg.add_trigger(
            &id,
            tasty_settings::AutoTrigger::Event { name: event.into() },
        );
        reg
    }

    #[test]
    fn guard_reentry_state_machine() {
        let mut g = AutofireGuard::new();
        assert!(!g.suppressed());
        g.note_submitted();
        assert!(g.suppressed(), "제출 직후부터 억제");
        drop(g.token());
        g.checkpoint();
        assert!(
            g.suppressed(),
            "완료 후 첫 checkpoint에서는 재실행을 계속 억제한다"
        );
        g.checkpoint();
        assert!(
            !g.suppressed(),
            "완료 후 두 번째 checkpoint에서 재실행을 허용한다"
        );
    }

    fn dispatch_now(
        engine: Option<&tasty_lua::LuaEngine>,
        reg: &ScriptRegistry,
        guard: &mut AutofireGuard,
        runs: &mut LuaRuns,
        event: &str,
    ) -> Vec<SystemNotice> {
        dispatch(engine, reg, guard, runs, event)
    }

    fn assert_lua_warning(n: &SystemNotice, needle: &str) {
        assert_eq!(n.level, crate::notification::NotificationLevel::Warning);
        assert_eq!(n.source, super::super::lua_runs::NOTICE_SOURCE);
        assert!(
            n.title.len() + n.body.len() > 0 && n.body.contains(needle),
            "본문이 대상을 설명해야 한다: {n:?}"
        );
    }

    #[test]
    fn dispatch_runs_matching_script_then_reentry_suppresses() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, hash) = temp_script("run", "local x = 1");
        let reg = registry_with(path, hash, "window.create.post");
        let mut guard = AutofireGuard::new();
        let mut runs = LuaRuns::default();

        let notices = dispatch_now(
            Some(&engine),
            &reg,
            &mut guard,
            &mut runs,
            "window.create.post",
        );
        assert_eq!(guard.submitted, 1, "일치 해시 → 실행 제출");
        assert!(notices.is_empty(), "제출에는 알림이 없다");

        let notices = dispatch_now(
            Some(&engine),
            &reg,
            &mut guard,
            &mut runs,
            "window.create.post",
        );
        assert_eq!(guard.submitted, 1, "재진입 가드가 재실행을 차단");
        assert_eq!(notices.len(), 1, "억제는 이벤트당 경고 하나");
        assert_lua_warning(&notices[0], "window.create.post");

        // 같은 worker에 eval을 보내 앞선 실행을 기다린 뒤 checkpoint를 두 번 진행한다.
        engine.eval("return 0").expect("worker alive");
        let settled = runs.settle(engine.drain_run_outcomes());
        assert!(settled.notices.is_empty(), "성공한 실행은 알림이 없다");
        guard.checkpoint();
        guard.checkpoint();
        dispatch_now(
            Some(&engine),
            &reg,
            &mut guard,
            &mut runs,
            "window.create.post",
        );
        assert_eq!(guard.submitted, 2, "정산 후 새 트리거는 다시 실행");
    }

    #[test]
    fn a_failing_autofire_reports_a_warning_and_the_next_trigger_still_runs() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, hash) = temp_script("boom", "error('boom')");
        let reg = registry_with(path, hash, "workspace.create.post");
        let mut guard = AutofireGuard::new();
        let mut runs = LuaRuns::default();

        dispatch_now(
            Some(&engine),
            &reg,
            &mut guard,
            &mut runs,
            "workspace.create.post",
        );
        engine.eval("return 0").expect("worker alive");
        let settled = runs.settle(engine.drain_run_outcomes());
        assert_eq!(settled.notices.len(), 1);
        assert_lua_warning(&settled.notices[0], "boom");
        assert!(settled.for_views.is_empty());

        guard.checkpoint();
        guard.checkpoint();
        dispatch_now(
            Some(&engine),
            &reg,
            &mut guard,
            &mut runs,
            "workspace.create.post",
        );
        assert_eq!(
            guard.submitted, 2,
            "실패 뒤에도 다음 자동실행은 억제되지 않는다"
        );
    }

    #[test]
    fn dispatch_blocks_on_tofu_mismatch() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, _real) = temp_script("tofu", "local x = 1");
        let stale = tasty_settings::hash_bytes(b"original content");
        let reg = registry_with(path, stale.clone(), "window.create.post");
        let mut guard = AutofireGuard::new();
        let mut runs = LuaRuns::default();

        let notices = dispatch_now(
            Some(&engine),
            &reg,
            &mut guard,
            &mut runs,
            "window.create.post",
        );
        assert_eq!(
            guard.submitted, 0,
            "TOFU 불일치 → 실행 차단 (해시 갱신 없음)"
        );
        assert!(!guard.suppressed(), "차단은 제출이 아니므로 가드 미점유");
        assert_eq!(notices.len(), 1);
        assert_lua_warning(&notices[0], "window.create.post");
        assert_eq!(
            reg.entries_for_event("window.create.post")
                .next()
                .unwrap()
                .sha256,
            stale,
            "차단 알림을 남겨도 해시는 바꾸지 않는다"
        );
    }

    #[test]
    fn a_missing_script_file_reports_a_read_warning() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, hash) = temp_script("gone", "local x = 1");
        std::fs::remove_file(&path).unwrap();
        let reg = registry_with(path, hash, "tab.create.post");
        let mut guard = AutofireGuard::new();
        let mut runs = LuaRuns::default();

        let notices = dispatch_now(
            Some(&engine),
            &reg,
            &mut guard,
            &mut runs,
            "tab.create.post",
        );
        assert_eq!(guard.submitted, 0);
        assert_eq!(notices.len(), 1);
        assert_lua_warning(&notices[0], "tab.create.post");
    }

    #[test]
    fn a_missing_engine_reports_instead_of_skipping_silently() {
        let (path, hash) = temp_script("noengine", "local x = 1");
        let reg = registry_with(path, hash, "tab.create.post");
        let mut guard = AutofireGuard::new();
        let mut runs = LuaRuns::default();
        let notices = dispatch_now(None, &reg, &mut guard, &mut runs, "tab.create.post");
        assert_eq!(notices.len(), 1);
        assert_lua_warning(&notices[0], "tab.create.post");
    }

    #[test]
    fn dispatch_ignores_unbound_event() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, hash) = temp_script("unbound", "local x = 1");
        let reg = registry_with(path, hash, "window.create.post");
        let mut guard = AutofireGuard::new();
        let mut runs = LuaRuns::default();

        let notices = dispatch_now(
            Some(&engine),
            &reg,
            &mut guard,
            &mut runs,
            "tab.create.post",
        );
        assert_eq!(guard.submitted, 0, "바인딩 없는 이벤트는 no-op");
        assert!(notices.is_empty());
    }
}
