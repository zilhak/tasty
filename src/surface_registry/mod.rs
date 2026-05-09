//! Surface 종류별 메타·동작 정의 레지스트리.
//!
//! 단계 03C에서는 *골격*만 도입한다 (kind 문자열만 보유, factory/render/snapshot 등은 03D에서 채움).
//! 본체 7종(Markdown/Explorer/Html/Image/Empty/ClipboardViewer/Terminal)이 단계 03D에서 등록된다.
//! 외부 plugin은 단계 05에서 같은 레지스트리에 추가될 예정.

use std::collections::HashMap;
use std::sync::Arc;

/// surface 종류별 메타 정보. 03C는 식별자만 둔다 — 03D에서 create/render/snapshot/restore/on_close
/// 함수 포인터들을 채운다.
pub struct SurfaceKindDef {
    /// 안정 식별자 (lowercase snake_case). 예: `"terminal"`, `"markdown"`.
    pub kind: &'static str,
}

/// surface 종류 lookup 테이블. `Arc<SurfaceKindRegistry>` 단위로 EngineState에 보관되어
/// 매 프레임 dispatch에 사용된다.
#[derive(Default)]
pub struct SurfaceKindRegistry {
    kinds: HashMap<&'static str, Arc<SurfaceKindDef>>,
}

impl SurfaceKindRegistry {
    pub fn new() -> Self {
        Self {
            kinds: HashMap::new(),
        }
    }

    pub fn register(&mut self, def: SurfaceKindDef) {
        let kind = def.kind;
        if self.kinds.insert(kind, Arc::new(def)).is_some() {
            tracing::warn!("SurfaceKindRegistry: kind '{}' overwritten", kind);
        }
    }

    pub fn get(&self, kind: &str) -> Option<Arc<SurfaceKindDef>> {
        self.kinds.get(kind).cloned()
    }

    pub fn contains(&self, kind: &str) -> bool {
        self.kinds.contains_key(kind)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&&'static str, &Arc<SurfaceKindDef>)> {
        self.kinds.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_lookup() {
        let mut reg = SurfaceKindRegistry::new();
        reg.register(SurfaceKindDef { kind: "alpha" });
        reg.register(SurfaceKindDef { kind: "beta" });
        assert!(reg.contains("alpha"));
        assert!(reg.contains("beta"));
        assert!(!reg.contains("gamma"));
        assert_eq!(reg.get("alpha").unwrap().kind, "alpha");
    }

    #[test]
    fn duplicate_register_overwrites() {
        let mut reg = SurfaceKindRegistry::new();
        reg.register(SurfaceKindDef { kind: "x" });
        reg.register(SurfaceKindDef { kind: "x" });
        // No panic; second register overwrites silently (warn logged).
        assert!(reg.contains("x"));
    }
}
