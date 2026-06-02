//! Plugin이 contributes한 IPC namespace prefix를 호스트가 추적하기 위한 registry.
//!
//! 호스트 IPC dispatcher는 `<prefix>.<method>` 메서드를 받았을 때 이 registry로
//! 어느 plugin에 forward할지 해결한다.

use std::collections::HashMap;

/// prefix → plugin_id 매핑. 단일 plugin이 여러 prefix를 점유할 수 있다.
#[derive(Debug, Default)]
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

    /// plugin이 unload될 때 그 plugin이 등록한 모든 prefix를 한 번에 제거.
    pub fn unregister_plugin(&mut self, plugin_id: &str) {
        if let Some(prefixes) = self.plugin_to_prefixes.remove(plugin_id) {
            for p in prefixes {
                self.prefix_to_plugin.remove(&p);
            }
        }
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
