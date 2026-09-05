//! Plugin이 contributes한 IPC namespace prefix를 호스트가 추적하기 위한 registry.
//!
//! 호스트 IPC dispatcher는 `<prefix>.<method>` 메서드를 받았을 때 이 registry로
//! 어느 plugin에 forward할지 해결한다.
//!
//! **이 표가 답하는 것은 "누가 소유하는가" 하나이고, 재료는 설치된 매니페스트다**
//! (`refresh_packages` 가 채운다). "지금 살아 있는가" 는 다른 물음이며 같은 자리에서
//! `processes` 검사가 따로 답한다(`validate_namespace_call` → `-32002`). 두 물음을 이
//! 표 하나에 겹쳐 두면 꺼진 plugin 의 메서드가 "그런 메서드 없다" 로 답해 거짓이 된다
//! — 근거는 [ADR-0173](../../../docs/adr/0173-namespace-resolution-reads-the-manifest-not-the-process-table.md).

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

    /// 지금 소유자로 등록돼 있는 plugin id 들.
    pub fn plugin_ids(&self) -> Vec<String> {
        self.plugin_to_prefixes.keys().cloned().collect()
    }

    /// 한 plugin 이 점유한 prefix 들. 없으면 빈 슬라이스.
    ///
    /// 해제할 때 **매니페스트를 다시 찾지 않고** 여기서 꺼내 쓰라고 있다 — 패키지가
    /// 디스크에서 사라진 뒤에는 매니페스트 조회가 실패해 mirror 가 남는다.
    pub fn prefixes_of(&self, plugin_id: &str) -> &[String] {
        self.plugin_to_prefixes
            .get(plugin_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
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
