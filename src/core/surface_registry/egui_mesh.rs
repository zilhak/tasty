//! egui-mesh 종류는 (kind, plugin ID) 허용 목록과 host API version 일치 여부를 검사한다.
//! 선언을 등록하는 단계이며 실제 mesh 전송·합성은 다른 경로에서 수행한다.

use std::sync::Arc;

use crate::core::egui_mesh_surface::EguiMeshSurface;
use crate::core::surface_registry::{
    KindSource, RegisteredRendering, SurfaceKindDef, SurfaceKindRegistry,
};
use crate::model::Surface;
use tasty_plugin_manifest::{HOST_API_VERSION, SurfaceKindDecl};

/// registry가 static 문자열을 요구하므로 메모리를 반환하지 않는다. 철회 후 재등록하면 다시 할당한다.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn extract_display_name_and_file(
    data: &serde_json::Value,
    fallback_kind: &str,
) -> (String, Option<String>) {
    let name = data
        .get("display_name")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| fallback_kind.to_string());
    let file = data
        .get("file")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    (name, file)
}

/// surface 종류의 허용 조합. plugin 팝업의 egui-mesh 채널은 이 목록과 별개다.
pub(crate) fn is_egui_mesh_allowed(kind: &str, plugin_id: &str) -> bool {
    matches!(
        (kind, plugin_id),
        ("image", "com.tasty.image") | ("mesh_demo", "com.tasty.mesh-demo")
    )
}

/// 허용 조합·API version을 검사하고 살아 있는 등록이 없으면 정의를 만든다.
/// 기존 등록이 있으면 종류 이름만 보고 성공하며 소유자·메타데이터를 다시 대조하지 않는다.
pub fn register_egui_mesh_kind(
    registry: &SurfaceKindRegistry,
    plugin_id: &str,
    decl: &SurfaceKindDecl,
    api_version: &str,
) -> bool {
    if !check_egui_mesh_whitelist(plugin_id, decl) {
        return false;
    }
    if !check_egui_mesh_api_version(plugin_id, decl, api_version) {
        return false;
    }
    if registry_already_has_egui_mesh_kind(registry, plugin_id, decl) {
        return true;
    }

    let kind_static = build_and_register_egui_mesh_kind_def(registry, plugin_id, decl);
    log_egui_mesh_kind_registered(kind_static, plugin_id);
    true
}

fn check_egui_mesh_whitelist(plugin_id: &str, decl: &SurfaceKindDecl) -> bool {
    if is_egui_mesh_allowed(&decl.kind, plugin_id) {
        return true;
    }
    tracing::warn!(
        "plugin '{}' declared egui-mesh kind '{}' which is not allowed by the host \
         whitelist; ignoring",
        plugin_id,
        decl.kind
    );
    false
}

fn check_egui_mesh_api_version(plugin_id: &str, decl: &SurfaceKindDecl, api_version: &str) -> bool {
    if api_version == HOST_API_VERSION {
        return true;
    }
    tracing::warn!(
        "plugin '{}' egui-mesh kind '{}' has api_version '{}' incompatible with host \
         '{}'; ignoring",
        plugin_id,
        decl.kind,
        api_version,
        HOST_API_VERSION
    );
    false
}

fn registry_already_has_egui_mesh_kind(
    registry: &SurfaceKindRegistry,
    plugin_id: &str,
    decl: &SurfaceKindDecl,
) -> bool {
    if !registry.contains(&decl.kind) {
        return false;
    }
    tracing::debug!(
        "egui-mesh kind '{}' from plugin '{}' already registered",
        decl.kind,
        plugin_id
    );
    true
}

fn log_egui_mesh_kind_registered(kind_static: &str, plugin_id: &str) {
    tracing::info!(
        "registered egui-mesh surface kind '{}' for plugin '{}'",
        kind_static,
        plugin_id
    );
}

fn build_and_register_egui_mesh_kind_def(
    registry: &SurfaceKindRegistry,
    plugin_id: &str,
    decl: &SurfaceKindDecl,
) -> &'static str {
    let kind_static: &'static str = leak_str(&decl.kind);
    let i18n_key_static: &'static str = leak_str(&decl.display_name_i18n_key);
    let plugin_id_for_create = plugin_id.to_string();
    let plugin_id_for_restore = plugin_id.to_string();

    registry.register(SurfaceKindDef {
        kind: kind_static,
        rendering: RegisteredRendering::EguiMesh,
        source: KindSource::Plugin(plugin_id.to_string()),
        display_name_i18n_key: i18n_key_static,
        icon: decl.icon.clone(),
        // 여기서는 생성 params만 보관한다. plugin에 surface.create를 보내는 일은 bootstrap 경로가 맡는다.
        create: Arc::new(move |sid, _cwd, params| {
            build_egui_mesh_surface(sid, params, kind_static, plugin_id_for_create.clone())
        }),
        restore: Arc::new(move |sid, data| {
            build_egui_mesh_surface(sid, data, kind_static, plugin_id_for_restore.clone())
        }),
        snapshot: Arc::new(|s: &dyn Surface| {
            let ms = s.as_any().downcast_ref::<EguiMeshSurface>()?;
            let mut obj = serde_json::Map::new();
            obj.insert("display_name".into(), serde_json::json!(ms.display_name));
            // preset의 TOML은 null을 표현하지 못하므로 값이 없으면 키를 생략한다.
            if let Some(file) = &ms.file {
                obj.insert("file".into(), serde_json::json!(file));
            }
            Some(serde_json::Value::Object(obj))
        }),
        preset_fields: crate::core::surface_registry::PresetFieldSpec::from_decls(
            &decl.preset_fields,
        ),
        param_aliases: decl.param_aliases.clone(),
        default_params: decl.default_params.clone(),
        consumes_egui_input: decl.consumes_egui_input,
        zoomable: decl.zoomable,
        egui_copy: decl.egui_copy,
        copy_path: decl.copy_path,
        egui_paste: decl.egui_paste,
        name_from_param: decl.name_from_param.clone(),
        records_recent: decl.records_recent,
        convert_requires_input: decl.convert_requires_input,
        convert_input_popup: decl
            .convert_input_popup
            .as_ref()
            .map(|p| format!("{plugin_id}/{p}")),
    });
    kind_static
}

fn build_egui_mesh_surface(
    sid: crate::model::SurfaceId,
    data: &serde_json::Value,
    kind_static: &'static str,
    plugin_id: String,
) -> anyhow::Result<Box<dyn Surface>> {
    let (name, file) = extract_display_name_and_file(data, kind_static);
    Ok(Box::new(EguiMeshSurface::new(
        sid,
        kind_static,
        plugin_id,
        name,
        file,
    )) as Box<dyn Surface>)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decl(kind: &str) -> SurfaceKindDecl {
        // 매니페스트 파서가 아닌 등록 조건만 검사할 입력이다.
        serde_json::from_value(json!({
            "kind": kind,
            "display_name_i18n_key": "surface.kind.markdown",
            "rendering": "egui-mesh",
        }))
        .unwrap()
    }

    #[test]
    fn markdown_is_no_longer_egui_mesh_allowed() {
        assert!(!is_egui_mesh_allowed("markdown", "com.tasty.markdown"));
    }

    #[test]
    fn image_allowed_for_image_plugin() {
        assert!(is_egui_mesh_allowed("image", "com.tasty.image"));
        assert!(!is_egui_mesh_allowed("image", "com.example.evil"));
        assert!(!is_egui_mesh_allowed("mesh_demo", "com.tasty.image"));
    }

    #[test]
    fn mesh_demo_allowed_for_demo_plugin() {
        assert!(is_egui_mesh_allowed("mesh_demo", "com.tasty.mesh-demo"));
        assert!(!is_egui_mesh_allowed("mesh_demo", "com.example.evil"));
        assert!(!is_egui_mesh_allowed("image", "com.tasty.mesh-demo"));
    }

    #[test]
    fn register_rejects_unauthorized_plugin() {
        let reg = SurfaceKindRegistry::new();
        assert!(!register_egui_mesh_kind(
            &reg,
            "com.example.evil",
            &decl("mesh_demo"),
            HOST_API_VERSION
        ));
        assert!(!reg.contains("mesh_demo"));
    }

    #[test]
    fn register_rejects_api_version_mismatch() {
        let reg = SurfaceKindRegistry::new();
        assert!(!register_egui_mesh_kind(
            &reg,
            "com.tasty.mesh-demo",
            &decl("mesh_demo"),
            "999"
        ));
        assert!(!reg.contains("mesh_demo"));
    }

    #[test]
    fn register_succeeds_and_creates_stand_in() {
        let reg = SurfaceKindRegistry::new();
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.mesh-demo",
            &decl("mesh_demo"),
            HOST_API_VERSION
        ));
        let def = reg.get("mesh_demo").unwrap();
        let s = (def.create)(5, None, &json!({"display_name": "Readme"})).unwrap();
        assert_eq!(s.kind(), "mesh_demo");
        assert_eq!(s.type_name(), "EguiMesh");
        assert_eq!(s.surface_id(), Some(5));
        assert_eq!(s.display_name(), "Readme");
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert_eq!(snap["display_name"], "Readme");
        let restored = (def.restore)(5, &snap).unwrap();
        assert_eq!(restored.kind(), "mesh_demo");
        assert_eq!(restored.display_name(), "Readme");
    }

    #[test]
    fn snapshot_omits_file_key_when_file_absent() {
        let reg = SurfaceKindRegistry::new();
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.mesh-demo",
            &decl("mesh_demo"),
            HOST_API_VERSION
        ));
        let def = reg.get("mesh_demo").unwrap();
        let s = (def.create)(7, None, &json!({ "display_name": "Demo" })).unwrap();
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert!(
            snap.get("file").is_none(),
            "file 없으면 snapshot 에 file 키가 없어야 한다(preset TOML 은 null 불가): {snap}"
        );
        let s2 = (def.create)(
            8,
            None,
            &json!({ "display_name": "Doc", "file": "/tmp/a.png" }),
        )
        .unwrap();
        let snap2 = (def.snapshot)(s2.as_ref()).unwrap();
        assert_eq!(snap2["file"], "/tmp/a.png");
    }

    #[test]
    fn register_forwards_capability_flags() {
        let decl: SurfaceKindDecl = serde_json::from_value(json!({
            "kind": "mesh_demo",
            "display_name_i18n_key": "surface.kind.markdown",
            "rendering": "egui-mesh",
            "zoomable": true,
            "egui_copy": true,
        }))
        .unwrap();
        let reg = SurfaceKindRegistry::new();
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.mesh-demo",
            &decl,
            HOST_API_VERSION
        ));
        let def = reg.get("mesh_demo").unwrap();
        assert!(def.zoomable);
        assert!(def.egui_copy);
        assert!(!def.egui_paste);
        assert!(!def.consumes_egui_input);
    }

    #[test]
    fn register_is_idempotent() {
        let reg = SurfaceKindRegistry::new();
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.mesh-demo",
            &decl("mesh_demo"),
            HOST_API_VERSION
        ));
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.mesh-demo",
            &decl("mesh_demo"),
            HOST_API_VERSION
        ));
        assert!(reg.contains("mesh_demo"));
    }
}
