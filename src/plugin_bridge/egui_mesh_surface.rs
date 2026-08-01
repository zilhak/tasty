//! egui-mesh surface 의 host 측 자리표.
//!
//! plugin 이 자기 프로세스에서 egui 를 tessellate 한 mesh 를 host 가 합성하는
//! 채널(ADR-0028)의 surface 측 모델. taxonomy/화이트리스트 등록과 layout
//! 라우팅(생성·식별·영속화)을 담당하고, 실제 mesh 수신·합성(buffer_id/
//! generation/ppp 추적, raw_input forward, 전용 `egui_wgpu::Renderer`)은
//! `gfx/gpu/egui_mesh_prepare.rs`/`view/main/egui_mesh.rs` 가 이 타입을 받아
//! 담당한다 — A1-S5(`4f7cde24`) 이후 렌더 경로가 이미 붙어있다.
//!
//! `RemoteSurface` 형제다. `Surface` trait 의 `kind() -> &'static str` 제약 때문에
//! plugin manifest 의 동적 kind 문자열은 `register_egui_mesh_kind` 에서 `Box::leak`
//! 으로 한 번 정적화한다 (plugin 등록 시 1회).

use std::path::PathBuf;

use crate::model::{Surface, SurfaceId};

/// egui-mesh 채널 surface 의 host 측 빈 stand-in.
pub struct EguiMeshSurface {
    pub id: SurfaceId,
    /// `Box::leak` 으로 정적화된 plugin kind (registry 등록 시 1회 leak).
    pub kind_static: &'static str,
    pub plugin_id: String,
    /// 탭 제목 등에 표시되는 이름. 생성 params 의 `display_name` 또는 kind 명.
    pub display_name: String,
    /// 생성 params 의 `file` (예: markdown 경로). plugin 이 콘텐츠를 소유하지만, host 는
    /// layout 영속화(snapshot→restore)에서 plugin 에 다시 넘겨주기 위해 보관한다.
    /// file 의미가 없는 kind(mesh-demo 등)면 `None`.
    pub file: Option<String>,
}

impl EguiMeshSurface {
    pub fn new(
        id: SurfaceId,
        kind_static: &'static str,
        plugin_id: String,
        display_name: String,
        file: Option<String>,
    ) -> Self {
        Self {
            id,
            kind_static,
            plugin_id,
            display_name,
            file,
        }
    }
}

impl Surface for EguiMeshSurface {
    tasty_model::impl_surface_any!();

    fn kind(&self) -> &'static str {
        self.kind_static
    }

    fn type_name(&self) -> &'static str {
        "EguiMesh"
    }

    fn surface_id(&self) -> Option<SurfaceId> {
        Some(self.id)
    }

    /// file 기반 egui-mesh surface(markdown 등)의 cwd 는 그 파일이 속한 폴더다.
    /// 이 surface 에서 새 터미널 split 등을 열 때 시작 폴더로 상속되고, markdown
    /// 제자리 이동(04) 후엔 새 파일의 부모로 따라간다. file 이 없는 kind(mesh-demo
    /// 등)는 고유 cwd 의미가 없어 None.
    fn source_cwd(&self) -> Option<PathBuf> {
        self.file
            .as_ref()
            .map(PathBuf::from)
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    }

    fn display_name(&self) -> String {
        self.display_name.clone()
    }

    fn to_tree_json(&self) -> serde_json::Value {
        serde_json::json!({
            "kind": self.kind_static,
            "type": "EguiMesh",
            "id": self.id,
            "plugin_id": self.plugin_id,
            "display_name": self.display_name,
        })
    }

    /// attach mesh mirror 후보 신호 — 화이트리스트 판정은 앱 계층
    /// (`is_egui_mesh_allowed`)의 책임, 여기선 raw (kind, plugin_id) 만 알린다.
    fn attach_mesh_info(&self) -> Option<(&str, &str)> {
        Some((self.kind_static, &self.plugin_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_basics() {
        let s = EguiMeshSurface::new(
            7,
            "markdown",
            "com.tasty.markdown".into(),
            "Readme".into(),
            Some("/docs/readme.md".into()),
        );
        assert_eq!(s.kind(), "markdown");
        assert_eq!(s.type_name(), "EguiMesh");
        assert_eq!(s.surface_id(), Some(7));
        assert_eq!(s.display_name(), "Readme");
        // file 기반 surface 의 cwd 는 그 파일의 부모 폴더.
        assert_eq!(s.source_cwd(), Some(PathBuf::from("/docs")));
    }

    #[test]
    fn source_cwd_none_without_file() {
        let s = EguiMeshSurface::new(9, "mesh-demo", "com.tasty.demo".into(), "Demo".into(), None);
        assert_eq!(s.source_cwd(), None);
    }

    #[test]
    fn tree_json_shape() {
        let s = EguiMeshSurface::new(
            3,
            "markdown",
            "com.tasty.markdown".into(),
            "Doc".into(),
            None,
        );
        let j = s.to_tree_json();
        assert_eq!(j["kind"], "markdown");
        assert_eq!(j["type"], "EguiMesh");
        assert_eq!(j["id"], 3);
        assert_eq!(j["plugin_id"], "com.tasty.markdown");
    }
}
