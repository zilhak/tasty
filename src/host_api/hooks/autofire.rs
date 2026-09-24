//! 이벤트에 연결된 스크립트를 읽고 등록 해시와 같을 때 실행 요청을 보낸다.
//! 변경·읽기 실패는 로그 후 건너뛰며 자동 승인이나 확인 팝업을 만들지 않는다.
//! 실행 중과 완료 직후의 재진입을 억제하지만 지연된 이벤트 연쇄 전체를 차단한다고 보장하지 않는다.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
#[cfg(any(feature = "gui", test))]
use std::sync::atomic::Ordering;

use tasty_settings::ScriptRegistry;

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

fn try_run_entry(
    lua: &tasty_lua::LuaEngine,
    guard: &mut AutofireGuard,
    event: &str,
    entry: &tasty_settings::ScriptEntry,
) {
    let source = match std::fs::read_to_string(&entry.path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(
                target: "tasty_lua",
                "autofire '{event}': script read failed {}: {e}",
                entry.path.display()
            );
            return;
        }
    };
    if tasty_settings::hash_bytes(source.as_bytes()) != entry.sha256 {
        tracing::warn!(
            target: "tasty_lua",
            "autofire '{event}': '{}' blocked — file changed since registration \
             (re-approve in Settings › Misc › Scripts)",
            entry.name
        );
        return;
    }
    let name = if entry.name.is_empty() {
        entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| entry.id.clone())
    } else {
        entry.name.clone()
    };
    lua.run_script_tracked(&source, Some(&name), guard.token());
    guard.note_submitted();
}

/// 같은 이벤트에 연결된 여러 스크립트는 함께 요청한다. 억제 여부는 반복문에 들어가기 전에 한 번만 본다.
pub(crate) fn dispatch(
    lua: Option<&tasty_lua::LuaEngine>,
    scripts: &ScriptRegistry,
    guard: &mut AutofireGuard,
    event: &str,
) {
    let Some(lua) = lua else { return };
    if scripts.entries_for_event(event).next().is_none() {
        return;
    }
    if guard.suppressed() {
        tracing::warn!(
            target: "tasty_lua",
            "autofire '{event}' suppressed until the completion checkpoints advance"
        );
        return;
    }
    for entry in scripts.entries_for_event(event) {
        try_run_entry(lua, guard, event, entry);
    }
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

    #[test]
    fn dispatch_runs_matching_script_then_reentry_suppresses() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, hash) = temp_script("run", "local x = 1");
        let reg = registry_with(path, hash, "window.create.post");
        let mut guard = AutofireGuard::new();

        dispatch(Some(&engine), &reg, &mut guard, "window.create.post");
        assert_eq!(guard.submitted, 1, "일치 해시 → 실행 제출");

        dispatch(Some(&engine), &reg, &mut guard, "window.create.post");
        assert_eq!(guard.submitted, 1, "재진입 가드가 재실행을 차단");

        // 같은 worker에 eval을 보내 앞선 실행을 기다린 뒤 checkpoint를 두 번 진행한다.
        engine.eval("return 0").expect("worker alive");
        guard.checkpoint();
        guard.checkpoint();
        dispatch(Some(&engine), &reg, &mut guard, "window.create.post");
        assert_eq!(guard.submitted, 2, "정산 후 새 트리거는 다시 실행");
    }

    #[test]
    fn dispatch_blocks_on_tofu_mismatch() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, _real) = temp_script("tofu", "local x = 1");
        let stale = tasty_settings::hash_bytes(b"original content");
        let reg = registry_with(path, stale, "window.create.post");
        let mut guard = AutofireGuard::new();

        dispatch(Some(&engine), &reg, &mut guard, "window.create.post");
        assert_eq!(
            guard.submitted, 0,
            "TOFU 불일치 → 실행 차단 (해시 갱신 없음)"
        );
        assert!(!guard.suppressed(), "차단은 제출이 아니므로 가드 미점유");
    }

    #[test]
    fn dispatch_ignores_unbound_event() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, hash) = temp_script("unbound", "local x = 1");
        let reg = registry_with(path, hash, "window.create.post");
        let mut guard = AutofireGuard::new();

        dispatch(Some(&engine), &reg, &mut guard, "tab.create.post");
        assert_eq!(guard.submitted, 0, "바인딩 없는 이벤트는 no-op");
    }
}
