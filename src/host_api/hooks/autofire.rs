//! Lua 스크립트 자동실행(autofire) — 트리거 이벤트 발화 시 등록 스크립트 실행.
//!
//! 배선: host lifecycle 이벤트 fire 지점(`hooks::lua::fire`) → 등록 트리거 매칭
//! (`ScriptRegistry::entries_for_event`) → 소스 read + SHA256 재검(TOFU) →
//! 일치 시 `run_script_tracked` / 불일치 시 **실행 차단** + `tracing::warn`.
//! 단축키 트리거(`try_dispatch_script_shortcut`)와 동형 시퀀스이며, 트리거 소스만
//! 단축키에서 이벤트로 바뀐 경로다 (ADR-0031).
//!
//! # TOFU 불일치 처리 (수동 경로와 다름)
//!
//! 수동(단축키) 발화는 사용자가 계기이므로 확인 popup 을 띄우지만, 자동 발화는
//! 사용자 개입 없이 일어나므로 popup/배너 없이 **차단 + warn 로그**만 남긴다
//! (배너 발화 정책: 배너는 사용자 직접 조작에서만). 해시는 자동 갱신하지 않는다 —
//! 사용자는 Misc›Scripts 관리창의 changed 배지로 확인하고 재승인한다.
//!
//! # 재진입 가드 (cascade 차단)
//!
//! 자동실행 스크립트가 `tasty.run_cli` 로 자기 트리거 대상을 만들면(예:
//! `surface.create.post` 에 바인딩된 스크립트가 split 을 실행) 재발화 → 재실행의
//! 무한 연쇄가 생긴다. per-job deadline 은 1회 실행만 보므로 이 연쇄를 못 막는다.
//! [`AutofireGuard`] 가 "자동실행 in-flight + 완료 직후 1 프레임" 동안 신규
//! 자동실행을 전역 억제해 연쇄를 유한(1회)으로 끊는다. origin(user/agent) 게이트는
//! 미배선(v1 스코프 밖)이므로 이 가드가 1차 필수 방어다.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tasty_settings::ScriptRegistry;

/// 자동실행 재진입 가드. `App` 이 1개 소유하고, 메인 스레드만 만진다
/// (`completed` 카운터만 워커가 [`tasty_lua::CompletionToken`] drop 으로 증가).
///
/// suppression 판정은 `submitted > acknowledged`. 완료(`completed`)는 즉시
/// acknowledge 되지 않고 [`AutofireGuard::checkpoint`] 를 **두 번** 지나야
/// 반영된다 — 완료 직후 프레임까지 억제를 유지하기 위한 의도된 지연이다.
pub(crate) struct AutofireGuard {
    /// 자동실행으로 제출된 스크립트 수 (메인 전용).
    submitted: u64,
    /// "정산"된 완료 수 — suppression 판정 기준 (메인 전용).
    acknowledged: u64,
    /// 직전 checkpoint 에서 샘플한 `completed` 값 (메인 전용).
    prev_sample: u64,
    /// 워커가 실행 종료 시 증가시키는 완료 카운터 (CompletionToken 공유).
    completed: Arc<AtomicU64>,
}

impl AutofireGuard {
    pub(crate) fn new() -> Self {
        Self {
            submitted: 0,
            acknowledged: 0,
            prev_sample: 0,
            completed: Arc::new(AtomicU64::new(0)),
        }
    }

    /// 자동실행 억제 중인지 — in-flight 스크립트가 있거나 완료가 아직 정산 전.
    fn suppressed(&self) -> bool {
        self.submitted > self.acknowledged
    }

    /// 프레임 경계 체크포인트 — `about_to_wait` 시작 시 1회 호출.
    ///
    /// 완료 카운트를 **한 프레임 늦게** acknowledge 한다. `run_cli` 가 유발한
    /// host 이벤트는 스크립트 완료 *이전에* 이미 pending 큐/이벤트 루프에 들어가
    /// 있으므로(run_cli 는 IPC 응답까지 블록), 완료 직후 프레임까지 suppression 을
    /// 유지해야 그 이벤트들이 같은 스크립트를 재점화하지 못한다.
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

/// `event` 를 트리거로 등록한 스크립트를 모두 실행한다 (TOFU 재검 통과분만).
///
/// 억제 판정은 fire 단위 — 같은 fire 에 바인딩된 복수 스크립트는 함께 실행되고,
/// 그 실행이 정산될 때까지의 **후속** fire 가 억제된다.
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
            "autofire '{event}' suppressed — auto-run script still in flight (reentry guard)"
        );
        return;
    }
    for entry in scripts.entries_for_event(event) {
        let source = match std::fs::read_to_string(&entry.path) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(
                    target: "tasty_lua",
                    "autofire '{event}': script read failed {}: {e}",
                    entry.path.display()
                );
                continue;
            }
        };
        // TOFU 재검 — 불일치는 차단. 해시 자동 갱신 금지(자동 승인은 TOFU 무의미).
        if tasty_settings::hash_bytes(source.as_bytes()) != entry.sha256 {
            tracing::warn!(
                target: "tasty_lua",
                "autofire '{event}': '{}' blocked — file changed since registration \
                 (re-approve in Settings › Misc › Scripts)",
                entry.name
            );
            continue;
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
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    /// 내용을 담은 임시 스크립트 파일 생성 → (경로, 정확한 해시).
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
        // 워커 완료 신호 — 즉시 풀리지 않는다.
        drop(g.token());
        g.checkpoint();
        assert!(
            g.suppressed(),
            "완료 직후 프레임은 여전히 억제 (cascade 이벤트가 이 창에서 drain 됨)"
        );
        g.checkpoint();
        assert!(!g.suppressed(), "완료 + 1 프레임 뒤 정산 완료");
    }

    #[test]
    fn dispatch_runs_matching_script_then_reentry_suppresses() {
        let engine = tasty_lua::LuaEngine::new().expect("init");
        let (path, hash) = temp_script("run", "local x = 1");
        let reg = registry_with(path, hash, "window.create.post");
        let mut guard = AutofireGuard::new();

        dispatch(Some(&engine), &reg, &mut guard, "window.create.post");
        assert_eq!(guard.submitted, 1, "일치 해시 → 실행 제출");

        // 같은 정산 창 안의 재발화(cascade 시나리오) → 억제.
        dispatch(Some(&engine), &reg, &mut guard, "window.create.post");
        assert_eq!(guard.submitted, 1, "재진입 가드가 재실행을 차단");

        // 실행 완료를 직렬 워커의 블로킹 eval 로 고정 후 2 프레임 정산 → 재발화 허용.
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
        // 등록 해시를 다른 내용 기준으로 — 등록 후 파일이 변경된 상황.
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
