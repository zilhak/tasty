//! attach mesh mirror 클라이언트측 surface
//! (`docs/dev-guide/attach-behavior.md` "mesh mirror 채널").
//!
//! `EguiMeshSurface`(호스트 모델)는 "로컬 `PluginManager`가 관리하는 실제 plugin"을
//! 전제하는 `plugin_id` 필드를 갖는다 — attach mirror(로컬에 그 plugin 프로세스가
//! 없고 서버에만 있음)에 재사용하면 필드 의미가 어긋난다. `RemoteSurface`도 부적합
//! (plugin 프로세스가 로컬에 실재하는 다른 용도의 타입). 이 타입은 그래서 신설됐다 —
//! `plugin_id`/`kind`는 순수 표시용 메타로만 보유하고, 실제 렌더 데이터(mesh 바이트)는
//! attach 세션이 TCP 로 받아 별도 저장소(`CoreState`의 attach mesh frame store)에 넣는다.

use std::path::PathBuf;

use super::SurfaceId;
use super::surface_trait::Surface;

/// attach mesh mirror 로 받은 원격 egui-mesh surface(markdown/image/mesh_demo 등)의
/// 클라이언트측 stand-in. 로컬에 plugin 프로세스가 없다 — 렌더은 attach 세션이 수신한
/// mesh 바이트를 GPU 가 직접 디코드해서 한다(`gfx/gpu/egui_mesh_prepare.rs`의
/// `render_attach_mesh_surfaces`).
pub struct AttachMeshSurface {
    pub id: SurfaceId,
    /// 원격 kind(markdown/image/mesh_demo). 서버가 이미 화이트리스트로 검증한 값만
    /// 내려오므로 여기서 재검증하지 않는다 — 순수 표시/디스패치용.
    pub kind_static: &'static str,
    /// 원격 plugin_id. 로컬에 그 plugin 프로세스가 없으므로 **표시용 메타**일 뿐,
    /// `PluginManager` 조회에 쓰이지 않는다(host 측 `EguiMeshSurface::plugin_id`와의
    /// 핵심 차이).
    pub plugin_id: String,
    pub display_name: String,
}

impl AttachMeshSurface {
    pub fn new(id: SurfaceId, kind: &str, plugin_id: String, display_name: String) -> Self {
        Self {
            id,
            kind_static: intern_known_kind(kind),
            plugin_id,
            display_name,
        }
    }
}

/// 서버가 이미 `is_egui_mesh_allowed` 화이트리스트로 검증한 kind 문자열만 이 타입에
/// 도달한다 — 클라이언트·서버가 같은 바이너리(같은 bundled plugin 상수
/// 집합)이므로 `Box::leak` 없이 고정 리터럴로 매핑 가능하다(호스트측
/// `register_egui_mesh_kind`의 `leak_str`은 *임의* manifest 문자열을 다루는 서버
/// 전용 필요라 이 클라이언트 경로에는 해당하지 않는다).
fn intern_known_kind(kind: &str) -> &'static str {
    match kind {
        // markdown 은 webview 채널로 전환되어(ADR-0065) `is_egui_mesh_allowed`
        // 화이트리스트(`src/core/surface_registry/egui_mesh.rs`)에서 이미 빠졌다 —
        // 서버가 이 값을 내려보내는 경로가 없어 이 분기는 도달하지 않는다. image/
        // mesh_demo 와 동일 패턴을 유지하고 서버 쪽 문서 근거가 있어 남겨둔다.
        "markdown" => "markdown",
        "image" => "image",
        "mesh_demo" => "mesh_demo",
        // 서버 화이트리스트가 미래에 확장되고 클라이언트가 그 커밋보다 뒤처진 경우의
        // 방어적 fallback — 렌더은 되지 않지만(kind 불일치로 plugin 특화 처리 없음)
        // 최소한 patch 로 죽지 않는다.
        _ => "mesh",
    }
}

impl Surface for AttachMeshSurface {
    crate::impl_surface_any!();

    fn kind(&self) -> &'static str {
        self.kind_static
    }

    fn type_name(&self) -> &'static str {
        "AttachMesh"
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    /// 원격 파일이라 로컬 파일시스템에 대응 경로가 없다 — cwd 상속 대상이 아니다.
    fn source_cwd(&self) -> Option<PathBuf> {
        None
    }

    fn display_name(&self) -> String {
        self.display_name.clone()
    }

    fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind_static,
            "type": "AttachMesh",
            "id": self.id,
            "plugin_id": self.plugin_id,
            "display_name": self.display_name,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_basics() {
        let s = AttachMeshSurface::new(7, "markdown", "com.tasty.markdown".into(), "Readme".into());
        assert_eq!(s.kind(), "markdown");
        assert_eq!(s.type_name(), "AttachMesh");
        assert_eq!(s.surface_id(), Some(7));
        assert_eq!(s.display_name(), "Readme");
        assert_eq!(s.source_cwd(), None);
    }

    #[test]
    fn unknown_kind_falls_back_without_panicking() {
        let s = AttachMeshSurface::new(1, "future_kind", "com.tasty.future".into(), "X".into());
        assert_eq!(s.kind(), "mesh");
    }

    #[test]
    fn tree_json_shape() {
        let s = AttachMeshSurface::new(3, "image", "com.tasty.image".into(), "Pic".into());
        let j = s.to_tree_json();
        assert_eq!(j["kind"], "image");
        assert_eq!(j["type"], "AttachMesh");
        assert_eq!(j["id"], 3);
        assert_eq!(j["plugin_id"], "com.tasty.image");
    }
}
