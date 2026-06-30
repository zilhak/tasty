//! 본 plugin 의 매니페스트가 `tasty-plugin-manifest` 의 schema 검증을 통과하고,
//! egui-mesh rendering 선언이 보존되는지 self-contained 검증.

use std::path::PathBuf;
use tasty_plugin_manifest::{Manifest, SurfaceKindRendering};

#[test]
fn manifest_loads_and_validates() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let m = Manifest::load(&dir).expect("manifest should load + validate");
    assert_eq!(m.id, "com.tasty.mesh-demo");
    assert_eq!(m.api_version, "1");
    assert_eq!(m.surface_kinds.len(), 1);
    assert_eq!(m.surface_kinds[0].kind, "mesh_demo");
    assert_eq!(
        m.surface_kinds[0].rendering,
        SurfaceKindRendering::EguiMesh,
        "rendering must parse to EguiMesh (wire key 'egui-mesh')"
    );
}
