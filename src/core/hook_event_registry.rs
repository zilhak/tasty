//! plugin이 선언한 hook 이벤트 키를 보관한다. HookEvent::parse가 임의 custom 키를 받으므로
//! host는 내장 키와 등록된 선언을 합쳐 hook.set·surface.fire_hook의 오타를 검사한다.
//! hello 때 등록하고 unload·remove 때 해제한다.

use std::collections::{BTreeSet, HashMap};
use std::sync::RwLock;
use std::sync::atomic::AtomicBool;

/// poison 로그가 반복되지 않도록 최초 보고를 기억한다.
static POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// 락 안에서는 맵을 삽입·제거·조회하며 외부 콜백을 실행하지 않는다.
/// poison 뒤에도 맵을 복구해 조회·갱신을 계속하고 최초 한 번 로그를 남긴다.
/// 빈 기본값으로 대체하면 선언된 키까지 미등록으로 거절하게 된다.
const WHAT: &str = "plugin hook event registry";

#[derive(Default)]
pub struct PluginHookEventRegistry {
    by_plugin: RwLock<HashMap<String, Vec<String>>>,
}

impl PluginHookEventRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 같은 plugin의 선언을 덮어쓴다.
    pub fn register(&self, plugin_id: &str, keys: Vec<String>) {
        let mut map = crate::poison::recover_write(self.by_plugin.write(), WHAT, &POISON_REPORTED);
        map.insert(plugin_id.to_string(), keys);
    }

    pub fn unregister(&self, plugin_id: &str) {
        let mut map = crate::poison::recover_write(self.by_plugin.write(), WHAT, &POISON_REPORTED);
        map.remove(plugin_id);
    }

    pub fn contains(&self, key: &str) -> bool {
        crate::poison::recover_read(self.by_plugin.read(), WHAT, &POISON_REPORTED)
            .values()
            .any(|ks| ks.iter().any(|k| k == key))
    }

    /// 진단에 표시할 전체 키를 정렬하고 중복을 제거한다.
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

    #[test]
    fn a_poisoned_registry_keeps_answering_and_keeps_recording() {
        let reg = std::sync::Arc::new(PluginHookEventRegistry::new());
        reg.register("com.tasty.claude", vec!["claude-idle".to_string()]);

        let poisoner = std::sync::Arc::clone(&reg);
        let handle = std::thread::spawn(move || {
            let _guard = poisoner.by_plugin.write().expect("fresh lock");
            panic!("poison the hook event registry");
        });
        assert!(handle.join().is_err(), "poisoner 스레드는 패닉해야 한다");
        assert!(
            reg.by_plugin.read().is_err(),
            "검사할 락이 poison 상태여야 한다"
        );

        assert!(reg.contains("claude-idle"));
        assert_eq!(reg.all_keys(), vec!["claude-idle"]);

        reg.register("com.tasty.codex", vec!["codex-idle".to_string()]);
        assert!(reg.contains("codex-idle"));
        reg.unregister("com.tasty.claude");
        assert!(!reg.contains("claude-idle"));
        assert_eq!(reg.all_keys(), vec!["codex-idle"]);
    }
}
