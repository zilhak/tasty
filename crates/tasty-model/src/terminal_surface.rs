use super::SurfaceId;
pub use super::surface_layout::{SurfaceLayout, SurfaceRegion};
use super::surface_trait::Surface;

/// 트리에서 터미널 ID만 보관하는 marker. PTY·스크롤백은 호스트 TerminalStore가 소유한다.
pub struct TerminalSurface {
    pub id: SurfaceId,
}

/// kind 등록을 기다리는 플러그인의 복원 정보. 등록 뒤 호스트의 restore 콜백에 전달한다.
#[derive(Clone)]
pub struct DeferredPlugin {
    /// surface kind 식별자(예: `"markdown"`). registry 등록을 기다리는 대상.
    pub kind: String,
    /// 해당 kind 의 `restore` 콜백이 받을 JSON. reify 까지 owned 로 보관한다.
    pub snapshot: serde_json::Value,
}

impl Surface for TerminalSurface {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        "terminal"
    }

    fn type_name(&self) -> &'static str {
        "Terminal"
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    /// Terminal 의 cwd 는 `engine.runtime.terminals.get(id).get_cwd()` 로 store 경유 —
    /// trait 는 None 반환. caller(host 의 `CoreState::surface_cwd`)가 분기 처리. Surface cwd
    /// invariant — `docs/design/policies/cwd.md#surface-cwd-invariant`.
    fn source_cwd(&self) -> Option<std::path::PathBuf> {
        None
    }

    fn to_tree_json(&self) -> serde_json::Value {
        // cols/rows 는 caller 가 engine.runtime.terminals.get(id) 로 enrichment.
        serde_json::json!({
            "type": "Terminal",
            "id": self.id,
            "cols": 0,
            "rows": 0,
        })
    }
}
