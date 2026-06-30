//! egui-mesh surface kind 화이트리스트 + 등록 (ADR-0028).
//!
//! plugin 매니페스트의 `[[surface_kinds]]` 에 `rendering = "egui-mesh"` 를 선언하면
//! plugin 이 자기 프로세스에서 egui 를 tessellate 한 mesh 를 host 가 합성한다
//! (실제 합성 경로는 A1-S5). `host_rendered` 와 동일한 bundled 전용 개방 정책이다 —
//! 임의 plugin 이 채널을 가로채지 못하도록 `(kind, plugin_id)` 화이트리스트로 매칭하고,
//! 추가로 plugin 의 `api_version` 이 호스트와 일치하는지 게이트한다 (epaint 와이어가
//! host·plugin 동일 컴파일을 강제하는 동안의 보호 — ADR-0028 개방 정책).
//!
//! 매칭/게이트 실패 시 등록을 거부하고 warn 로그를 남긴다.

use std::sync::Arc;

use crate::engine::surface_registry::{SurfaceKindDef, SurfaceKindRegistry};
use crate::model::Surface;
use crate::plugin::manifest::{HOST_API_VERSION, SurfaceKindDecl};
use crate::plugin_bridge::egui_mesh_surface::EguiMeshSurface;

/// kind / i18n 키 문자열을 정적화. 같은 입력에 대해 leak 이 반복되지 않도록
/// caller(`register_egui_mesh_kind`)가 plugin hello 1회당 한 번만 호출한다.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// `(kind, plugin_id)` 쌍이 egui-mesh 채널로 허용된 bundled 조합인지 확인.
///
/// `host_rendered::is_host_rendered_allowed` 미러. ADR-0028 scope 에 따라 markdown
/// 을 첫 소비자로 두고, 이후 단계에서 image 하이브리드 등이 추가된다.
fn is_egui_mesh_allowed(kind: &str, plugin_id: &str) -> bool {
    matches!((kind, plugin_id), ("markdown", "com.tasty.markdown"))
}

/// plugin manager 가 hello 직후 매니페스트에 `rendering = "egui-mesh"` 선언이 있을 때 호출.
///
/// 화이트리스트 매칭 + api_version 게이트를 통과하고 아직 registry 에 없으면
/// `EguiMeshSurface` 빈 stand-in 을 생성하는 `SurfaceKindDef` 를 등록한다. 이미
/// 등록돼 있으면 idempotent no-op.
///
/// 반환값: 등록(또는 기존 사용)이 허용되면 `true`, 게이트 실패면 `false`.
pub fn register_egui_mesh_kind(
    registry: &SurfaceKindRegistry,
    plugin_id: &str,
    decl: &SurfaceKindDecl,
    api_version: &str,
) -> bool {
    if !is_egui_mesh_allowed(&decl.kind, plugin_id) {
        tracing::warn!(
            "plugin '{}' declared egui-mesh kind '{}' which is not allowed by the host \
             whitelist; ignoring",
            plugin_id,
            decl.kind
        );
        return false;
    }
    if api_version != HOST_API_VERSION {
        tracing::warn!(
            "plugin '{}' egui-mesh kind '{}' has api_version '{}' incompatible with host \
             '{}'; ignoring",
            plugin_id,
            decl.kind,
            api_version,
            HOST_API_VERSION
        );
        return false;
    }
    if registry.contains(&decl.kind) {
        tracing::debug!(
            "egui-mesh kind '{}' from plugin '{}' already registered",
            decl.kind,
            plugin_id
        );
        return true;
    }

    let kind_static: &'static str = leak_str(&decl.kind);
    let i18n_key_static: &'static str = leak_str(&decl.display_name_i18n_key);
    let plugin_id_for_create = plugin_id.to_string();
    let plugin_id_for_restore = plugin_id.to_string();

    registry.register(SurfaceKindDef {
        kind: kind_static,
        display_name_i18n_key: i18n_key_static,
        create: Arc::new(move |sid, _cwd, params| {
            let name = params
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| kind_static.to_string());
            Ok(Box::new(EguiMeshSurface::new(
                sid,
                kind_static,
                plugin_id_for_create.clone(),
                name,
            )) as Box<dyn Surface>)
        }),
        restore: Arc::new(move |sid, data| {
            let name = data
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| kind_static.to_string());
            Ok(Box::new(EguiMeshSurface::new(
                sid,
                kind_static,
                plugin_id_for_restore.clone(),
                name,
            )) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|s: &dyn Surface| {
            let ms = s.as_any().downcast_ref::<EguiMeshSurface>()?;
            Some(serde_json::json!({ "display_name": ms.display_name }))
        }),
    });

    tracing::info!(
        "registered egui-mesh surface kind '{}' for plugin '{}'",
        kind_static,
        plugin_id
    );
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn decl(kind: &str) -> SurfaceKindDecl {
        // toml 파싱 대신 직접 구성 — 본 모듈의 게이트만 검증한다.
        serde_json::from_value(json!({
            "kind": kind,
            "display_name_i18n_key": "surface.kind.markdown",
            "rendering": "egui-mesh",
        }))
        .unwrap()
    }

    #[test]
    fn markdown_allowed_for_markdown_plugin() {
        assert!(is_egui_mesh_allowed("markdown", "com.tasty.markdown"));
        assert!(!is_egui_mesh_allowed("markdown", "com.example.evil"));
        assert!(!is_egui_mesh_allowed("image", "com.tasty.markdown"));
    }

    #[test]
    fn register_rejects_unauthorized_plugin() {
        let reg = SurfaceKindRegistry::new();
        assert!(!register_egui_mesh_kind(
            &reg,
            "com.example.evil",
            &decl("markdown"),
            HOST_API_VERSION
        ));
        assert!(!reg.contains("markdown"));
    }

    #[test]
    fn register_rejects_api_version_mismatch() {
        let reg = SurfaceKindRegistry::new();
        assert!(!register_egui_mesh_kind(
            &reg,
            "com.tasty.markdown",
            &decl("markdown"),
            "999"
        ));
        assert!(!reg.contains("markdown"));
    }

    #[test]
    fn register_succeeds_and_creates_stand_in() {
        let reg = SurfaceKindRegistry::new();
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.markdown",
            &decl("markdown"),
            HOST_API_VERSION
        ));
        let def = reg.get("markdown").unwrap();
        let s = (def.create)(5, None, &json!({"display_name": "Readme"})).unwrap();
        assert_eq!(s.kind(), "markdown");
        assert_eq!(s.type_name(), "EguiMesh");
        assert_eq!(s.surface_id(), Some(5));
        assert_eq!(s.display_name(), "Readme");
        // snapshot → restore 라운드트립.
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert_eq!(snap["display_name"], "Readme");
        let restored = (def.restore)(5, &snap).unwrap();
        assert_eq!(restored.kind(), "markdown");
        assert_eq!(restored.display_name(), "Readme");
    }

    #[test]
    fn register_is_idempotent() {
        let reg = SurfaceKindRegistry::new();
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.markdown",
            &decl("markdown"),
            HOST_API_VERSION
        ));
        // 두 번째 호출: registry 에 이미 있으므로 no-op + 성공 반환.
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.markdown",
            &decl("markdown"),
            HOST_API_VERSION
        ));
        assert!(reg.contains("markdown"));
    }
}
