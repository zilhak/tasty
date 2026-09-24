use std::any::Any;
use std::path::PathBuf;

use super::{PhysicalRect, SurfaceId};

/// Surface 공통 동작. 부팅 워커에서 메인 스레드로 모델을 옮기므로 구현체는 Send여야 한다.
/// kind, Any 접근자, cwd 등 필수 메서드는 각 구현체가 의미를 정한다.
pub trait Surface: Any + Send {
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

    /// mesh mirror 후보의 kind와 plugin ID. 실제 허용 여부는 호스트가 검사한다.
    /// 모델은 구체적인 플러그인 타입을 참조하지 않는다. 기본값 None은 후보가 아님을 뜻한다.
    fn attach_mesh_info(&self) -> Option<(&str, &str)> {
        None
    }

    /// 원문 mirror 후보의 kind·plugin ID·원격 파일 경로. 실제 허용 여부는 호스트가 검사한다.
    /// 파일 경로 None은 빈 문서 후보이고 반환값 전체가 None이면 후보가 아니다.
    /// 구현체의 잠긴 캐시에서 복사해 반환할 수 있도록 owned 값으로 받는다.
    fn attach_content_info(&self) -> Option<(&str, &str, Option<PathBuf>)> {
        None
    }

    /// leaf 영역에 맞출 때 호출하는 선택적 hook. 기본값은 아무것도 하지 않는다.
    /// 터미널 PTY 크기 변경은 별도 경로가 맡는다.
    fn resize_all(&mut self, _rect: PhysicalRect, _cell_width: f32, _cell_height: f32) {}

    /// The "source" working directory associated with this surface, if any.
    ///
    /// 단축키 등 사용자 행위로 새 surface(터미널/탭/워크스페이스 등)를 만들 때
    /// 이 값을 시작 cwd로 상속한다. **default 본문 없음** — 모든 Surface impl
    /// 이 의미를 명시적으로 결정해야 한다 (Surface cwd invariant —
    /// `docs/design/policies/cwd.md#surface-cwd-invariant`).
    ///
    /// TerminalSurface는 None이며 호스트가 TerminalStore에서 cwd를 구한다.
    /// 다른 구현은 파일 부모 경로나 carry한 cwd 등 자기 의미에 맞는 값을 반환한다.
    fn source_cwd(&self) -> Option<PathBuf>;

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
