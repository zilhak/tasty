//! 설치된 매니페스트의 IPC prefix 소유자를 기록한다. 실행 중인지 여부와는 별개다.
//! 프로세스 상태와 호출 가능 여부는 validate_namespace_call에서 따로 확인한다.

use std::collections::HashMap;

/// prefix → plugin_id 매핑. 단일 plugin이 여러 prefix를 점유할 수 있다.
#[derive(Debug, Default, PartialEq)]
pub struct IpcNamespaceRegistry {
    prefix_to_plugin: HashMap<String, String>,
    plugin_to_prefixes: HashMap<String, Vec<String>>,
}

impl IpcNamespaceRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 등록. 이미 다른 plugin이 점유 중이면 Err.
    /// 같은 plugin의 동일 prefix 재등록은 idempotent.
    pub fn register(&mut self, plugin_id: &str, prefix: &str) -> anyhow::Result<()> {
        if let Some(existing) = self.prefix_to_plugin.get(prefix) {
            if existing != plugin_id {
                anyhow::bail!(
                    "ipc namespace prefix '{prefix}' already owned by plugin '{existing}'"
                );
            }
            return Ok(());
        }
        self.prefix_to_plugin
            .insert(prefix.to_string(), plugin_id.to_string());
        self.plugin_to_prefixes
            .entry(plugin_id.to_string())
            .or_default()
            .push(prefix.to_string());
        Ok(())
    }

    /// 해당 플러그인이 등록한 prefix를 모두 제거한다.
    pub fn unregister_plugin(&mut self, plugin_id: &str) {
        if let Some(prefixes) = self.plugin_to_prefixes.remove(plugin_id) {
            for p in prefixes {
                self.prefix_to_plugin.remove(&p);
            }
        }
    }

    /// 지금 소유자로 등록돼 있는 plugin id 들.
    pub fn plugin_ids(&self) -> Vec<String> {
        self.plugin_to_prefixes.keys().cloned().collect()
    }

    /// 해당 플러그인의 prefix 목록. 패키지가 삭제돼도 이 목록으로 등록을 해제할 수 있다.
    pub fn prefixes_of(&self, plugin_id: &str) -> &[String] {
        self.plugin_to_prefixes
            .get(plugin_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// 소유자 목록을 다시 구성하기 전에 모든 항목을 지운다.
    pub fn clear(&mut self) {
        self.prefix_to_plugin.clear();
        self.plugin_to_prefixes.clear();
    }

    /// prefix의 등록 여부만 확인한다. resolve는 메서드명에서 소유자를 찾는다.
    pub fn owns_prefix(&self, prefix: &str) -> bool {
        self.prefix_to_plugin.contains_key(prefix)
    }

    /// 메서드명(`"codex.spawn"`)을 보고 어느 plugin이 처리할지 해결.
    /// 등록되지 않은 prefix면 None — 호스트가 자기 핸들러 검사로 진행.
    pub fn resolve(&self, method: &str) -> Option<&str> {
        let dot = method.find('.')?;
        let prefix = &method[..dot];
        self.prefix_to_plugin.get(prefix).map(|s| s.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_then_resolve() {
        let mut r = IpcNamespaceRegistry::new();
        r.register("com.example.codex", "codex").unwrap();
        assert_eq!(r.resolve("codex.spawn"), Some("com.example.codex"));
        assert_eq!(r.resolve("codex.wait"), Some("com.example.codex"));
    }

    #[test]
    fn prefix_conflict_rejected() {
        let mut r = IpcNamespaceRegistry::new();
        r.register("com.example.codex", "codex").unwrap();
        let err = r
            .register("com.example.evil", "codex")
            .unwrap_err()
            .to_string();
        assert!(err.contains("already owned"), "got: {err}");
    }

    #[test]
    fn same_plugin_reregister_is_idempotent() {
        let mut r = IpcNamespaceRegistry::new();
        r.register("com.example.codex", "codex").unwrap();
        r.register("com.example.codex", "codex").unwrap();
        assert_eq!(r.resolve("codex.spawn"), Some("com.example.codex"));
    }

    #[test]
    fn unregister_clears_all() {
        let mut r = IpcNamespaceRegistry::new();
        r.register("com.example.codex", "codex").unwrap();
        r.register("com.example.codex", "cdx").unwrap();
        r.unregister_plugin("com.example.codex");
        assert_eq!(r.resolve("codex.spawn"), None);
        assert_eq!(r.resolve("cdx.spawn"), None);
    }

    #[test]
    fn resolve_unknown_returns_none() {
        let r = IpcNamespaceRegistry::new();
        assert_eq!(r.resolve("nope.bar"), None);
        assert_eq!(r.resolve("noformat"), None);
    }
}
