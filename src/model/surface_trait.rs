use std::any::Any;
use std::path::PathBuf;

use super::{PhysicalRect, SurfaceId};

/// Common behavior for all Surface types.
///
/// Each surface type (TerminalSurface, MarkdownPanel, EmptySurface, DiffPanel,
/// ImagePanel, RemoteSurface) implements this trait.
/// All methods have default implementations suitable for non-terminal surfaces.
pub trait Surface: Any {
    /// Stable identifier for this surface kind (lowercase, snake_case).
    /// 예: `"terminal"`, `"markdown"`. IPC/registry/플러그인이
    /// 식별자로 쓰며, 절대 변경되지 않는다.
    fn kind(&self) -> &'static str;

    /// Any-cast accessor. Used by the surface registry's render/snapshot/restore
    /// closures and other callers that need to recover the concrete surface type
    /// without a per-kind downcast method on the trait.
    /// 모든 구현체는 `crate::impl_surface_any!()` 매크로 한 줄로 채울 수 있다.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Display-only type name (e.g. "Terminal", "Markdown"). 사용자에게 보이는
    /// 라벨이며 식별 비교에는 `kind()`를 써야 한다. 향후 i18n 적용 가능.
    fn type_name(&self) -> &'static str;

    /// Get this surface's ID.
    fn surface_id(&self) -> Option<SurfaceId>;

    /// All surface IDs contained in this surface.
    fn all_surface_ids(&self) -> Vec<SurfaceId> {
        self.surface_id().into_iter().collect()
    }

    /// The focused surface ID.
    fn focused_surface_id(&self) -> Option<SurfaceId> {
        self.surface_id()
    }

    /// Whether this surface contains the given surface ID.
    fn contains_surface(&self, surface_id: SurfaceId) -> bool {
        self.all_surface_ids().contains(&surface_id)
    }

    /// Resize-fitting hook. Layout 가 leaf 의 rect 를 알릴 때 호출. 기본 no-op.
    /// 현재 모든 구현 (TerminalSurface 포함) 이 default 만 — Terminal resize 는
    /// 별 PTY resize 경로로 분리. 본 메서드는 후속 surface kind 들의 옵션.
    fn resize_all(&mut self, _rect: PhysicalRect, _cell_width: f32, _cell_height: f32) {}

    /// The "source" working directory associated with this surface, if any.
    ///
    /// 단축키 등 사용자 행위로 새 surface(터미널/탭/워크스페이스 등)를 만들 때
    /// 이 값을 시작 cwd로 상속한다.
    ///
    /// - TerminalSurface: 터미널의 OSC 7 cwd
    /// - MarkdownPanel: 파일의 부모 디렉터리
    /// - webview-enabled surface (plugin 정의): plugin 측에서 결정
    /// - 그 외(Image/Empty): None
    fn source_cwd(&self) -> Option<PathBuf> {
        None
    }

    /// Display name for tab title. Default: type_name.
    fn display_name(&self) -> String {
        self.type_name().to_string()
    }

    /// webview URL accessor. webview overlay 를 사용하는 surface kind 가 자신의
    /// URL 을 반환. host 의 `sync_webviews` 가 이 메서드로 surface 별 URL 을
    /// 식별. 일반 surface 는 default `None` 반환.
    ///
    /// `Option<String>` 시그니처는 plugin RemoteSurface 가 lock 으로 보관한 URL
    /// 캐시를 owned 로 cloning 해 반환할 수 있게 한다.
    fn webview_url(&self) -> Option<String> {
        None
    }

    /// Produce a JSON tree representation of this surface.
    fn to_tree_json(&self) -> serde_json::Value {
        let mut obj = serde_json::json!({
            "kind": self.kind(),
            "type": self.type_name(), // 호환성을 위한 별칭. 신규 코드는 `kind` 사용.
        });
        if let Some(id) = self.surface_id() {
            obj["id"] = serde_json::json!(id);
        }
        obj
    }
}
