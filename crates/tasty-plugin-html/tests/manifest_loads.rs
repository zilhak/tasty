//! HTML 매니페스트와 프리셋 필드 선언을 검사한다.

use std::path::PathBuf;
use tasty_plugin_manifest::{Manifest, PresetFieldInputType};

#[test]
fn manifest_loads_and_validates() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let m = Manifest::load(&dir).expect("manifest should load + validate");
    assert_eq!(m.id, "com.tasty.html");
    assert_eq!(m.surface_kinds.len(), 1);
    let kind = &m.surface_kinds[0];
    assert_eq!(kind.kind, "html");
    // 프리셋 편집 필드 — URL(경로 파생 없음).
    assert_eq!(kind.preset_fields.len(), 1);
    let f = &kind.preset_fields[0];
    assert_eq!(f.param_key, "url");
    assert_eq!(f.input_type, PresetFieldInputType::Url);
    assert!(!f.derive_cwd);
}
