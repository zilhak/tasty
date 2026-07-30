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
        let mut map = match self.by_plugin.write() {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("PluginHookEventRegistry write lock poisoned: {e}");
                return;
            }
        };
        map.insert(plugin_id.to_string(), keys);
    }

    /// plugin 의 선언 키 집합을 제거. unload/remove 시 호출.
    pub fn unregister(&self, plugin_id: &str) {
        let mut map = match self.by_plugin.write() {
            Ok(m) => m,
            Err(e) => {
                tracing::error!("PluginHookEventRegistry write lock poisoned: {e}");
                return;
            }
        };
        map.remove(plugin_id);
    }

    /// 활성 plugin 중 하나라도 이 키를 선언했는지.
    pub fn contains(&self, key: &str) -> bool {
        self.by_plugin
            .read()
            .map(|m| m.values().any(|ks| ks.iter().any(|k| k == key)))
            .unwrap_or(false)
    }

    /// 선언된 전체 키를 정렬·중복 제거해 반환 (검증 실패 에러 메시지용).
    pub fn all_keys(&self) -> Vec<String> {
        let mut set: BTreeSet<String> = BTreeSet::new();
        if let Ok(m) = self.by_plugin.read() {
            for ks in m.values() {
                for k in ks {
                    set.insert(k.clone());
                }
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
}
