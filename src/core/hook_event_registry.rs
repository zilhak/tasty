//! Plugin 이 manifest `[[contributes.hook_events]]` 로 선언한 surface hook 이벤트
//! 키의 host-side 레지스트리.
//!
//! 코어 [`tasty_hooks::HookEvent::parse`] 는 미인식 문자열을 `Custom(String)` 으로
//! 무조건 수용하므로, 오타·미존재 이벤트도 조용히 등록돼 영원히 발사되지 않는
//! 죽은 hook 이 생긴다. 이를 막기 위해 plugin 이 자기가 발사하는 키를 선언하고,
//! host 가 `hook.set` / `surface.fire_hook` 에서 (내장 ∪ 활성 plugin 선언) 집합으로
//! 검증한다.
//!
//! plugin hello 시 [`register`](PluginHookEventRegistry::register) 로 집계하고,
//! unload/remove 시 [`unregister`](PluginHookEventRegistry::unregister) 로 제거한다 —
//! 활성 plugin 만 검증 집합에 남으므로 비활성 plugin 의 이벤트 등록은 거부된다
//! (dead-setting 방지). `surface_registry` 와 동일하게 내부 `RwLock` 으로 부팅 후
//! 동적 갱신을 허용한다.

use std::collections::{BTreeSet, HashMap};
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

/// poison 을 보고했는가(첫 1 회만). poison 은 sticky 라 매 호출 남기면 그 로그가
/// 다른 진단을 덮는다.
static POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// 두 질문에 대한 답
/// ([`error-handling.md`](../../../docs/dev-guide/error-handling.md) "락 poison").
///
/// ① **임계구역이 불변식을 깨진 채 남길 수 있는가 — 아니다.** 네 접근자가 하는 일은
///    `HashMap<String, Vec<String>>` 의 삽입·제거·조회뿐이고 락 안에서 호출자 클로저를
///    돌리지 않는다. 어느 지점에서 패닉이 나도 맵은 구조적으로 성립한다.
/// ② **여기서 패닉하면 무엇이 죽는가 — 프로세스 전부.** 조회 경로는
///    `src/adapters/ipc/handler/hooks.rs` 의 `hook.set` / `surface.fire_hook` 검증이라
///    IPC 스레드에서 돌고, 등록 경로는 plugin hello 처리다.
///
/// 그래서 **복구하고 첫 1 회를 보고한다.**
///
/// 이전 형태가 나빴던 이유는 방향이 둘로 갈렸기 때문이다. `register`/`unregister` 는
/// 로그를 남기고 **돌아섰다** — poison 이 sticky 라 그 시점부터 등록·해제가 영구히
/// 안 먹는다. `contains`/`all_keys` 는 조용히 기본값으로 떨어졌는데, 소비자
/// (`hooks.rs:23`)가 `contains` 를 **긍정 극성**으로 써서 `false` 가 곧 모든 custom
/// hook 키 거부(fail-closed)가 되고, 이어지는 `all_keys()` 의 빈 목록이 에러 문구를
/// `(none — no active plugin declares hook events)` 로 만든다 — poison 상태를
/// "선언한 plugin 이 없다" 로 **오도**한다.
const WHAT: &str = "plugin hook event registry";

#[derive(Default)]
pub struct PluginHookEventRegistry {
    /// plugin_id → 선언된 hook 이벤트 키 목록.
    by_plugin: RwLock<HashMap<String, Vec<String>>>,
}

impl PluginHookEventRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// plugin 의 선언 키 집합을 등록(덮어쓰기). hello 시 호출.
    pub fn register(&self, plugin_id: &str, keys: Vec<String>) {
        let mut map = crate::poison::recover_write(self.by_plugin.write(), WHAT, &POISON_REPORTED);
        map.insert(plugin_id.to_string(), keys);
    }

    /// plugin 의 선언 키 집합을 제거. unload/remove 시 호출.
    pub fn unregister(&self, plugin_id: &str) {
        let mut map = crate::poison::recover_write(self.by_plugin.write(), WHAT, &POISON_REPORTED);
        map.remove(plugin_id);
    }

    /// 활성 plugin 중 하나라도 이 키를 선언했는지.
    pub fn contains(&self, key: &str) -> bool {
        crate::poison::recover_read(self.by_plugin.read(), WHAT, &POISON_REPORTED)
            .values()
            .any(|ks| ks.iter().any(|k| k == key))
    }

    /// 선언된 전체 키를 정렬·중복 제거해 반환 (검증 실패 에러 메시지용).
    pub fn all_keys(&self) -> Vec<String> {
        let mut set: BTreeSet<String> = BTreeSet::new();
        let map = crate::poison::recover_read(self.by_plugin.read(), WHAT, &POISON_REPORTED);
        for ks in map.values() {
            for k in ks {
                set.insert(k.clone());
            }
        }
        set.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_contains_unregister() {
        let reg = PluginHookEventRegistry::new();
        reg.register(
            "com.tasty.claude",
            vec!["claude-idle".to_string(), "needs-input".to_string()],
        );
        assert!(reg.contains("claude-idle"));
        assert!(reg.contains("needs-input"));
        assert!(!reg.contains("claude-iddle"));
        assert_eq!(reg.all_keys(), vec!["claude-idle", "needs-input"]);

        reg.unregister("com.tasty.claude");
        assert!(!reg.contains("claude-idle"));
        assert!(reg.all_keys().is_empty());
    }

    /// poison 이어도 네 접근자가 **계속 답하고 계속 반영한다.**
    ///
    /// 이전에는 방향이 갈렸다 — 읽기는 조용히 `false`/빈 목록으로 떨어져
    /// `hooks.rs` 가 모든 custom hook 키를 거부했고(fail-closed), 쓰기는 로그만 남기고
    /// 돌아서 sticky poison 아래서 등록·해제가 영구히 안 먹었다. 그래서 이 테스트는
    /// "패닉하지 않는다" 가 아니라 **기능이 계속 도는지**를 단언한다.
    #[test]
    fn a_poisoned_registry_keeps_answering_and_keeps_recording() {
        let reg = std::sync::Arc::new(PluginHookEventRegistry::new());
        reg.register("com.tasty.claude", vec!["claude-idle".to_string()]);

        // 다른 스레드가 write 락을 든 채 패닉시켜 poison 을 만든다.
        let poisoner = std::sync::Arc::clone(&reg);
        let handle = std::thread::spawn(move || {
            let _guard = poisoner.by_plugin.write().expect("fresh lock");
            panic!("poison the hook event registry");
        });
        assert!(handle.join().is_err(), "poisoner 스레드는 패닉해야 한다");
        assert!(
            reg.by_plugin.read().is_err(),
            "이 시점에 poison 이어야 실험이 성립한다"
        );

        // 읽기 — 계속 답한다.
        assert!(reg.contains("claude-idle"));
        assert_eq!(reg.all_keys(), vec!["claude-idle"]);

        // 쓰기 — 계속 반영된다.
        reg.register("com.tasty.codex", vec!["codex-idle".to_string()]);
        assert!(reg.contains("codex-idle"));
        reg.unregister("com.tasty.claude");
        assert!(!reg.contains("claude-idle"));
        assert_eq!(reg.all_keys(), vec!["codex-idle"]);
    }
}
