//! egui-mesh surface kind 화이트리스트 + 등록 (ADR-0028).
//!
//! plugin 매니페스트의 `[[surface_kinds]]` 에 `rendering = "egui-mesh"` 를 선언하면
//! plugin 이 자기 프로세스에서 egui 를 tessellate 한 mesh 를 host 가 합성한다
//! (실제 합성 경로는 A1-S5). bundled 전용 개방 정책이다 —
//! 임의 plugin 이 채널을 가로채지 못하도록 `(kind, plugin_id)` 화이트리스트로 매칭하고,
//! 추가로 plugin 의 `api_version` 이 호스트와 일치하는지 게이트한다 (epaint 와이어가
//! host·plugin 동일 컴파일을 강제하는 동안의 보호 — ADR-0028 개방 정책).
//!
//! 매칭/게이트 실패 시 등록을 거부하고 warn 로그를 남긴다.

use std::sync::Arc;

use crate::core::surface_registry::{SurfaceKindDef, SurfaceKindRegistry};
use crate::model::Surface;
use crate::plugin::manifest::{HOST_API_VERSION, SurfaceKindDecl};
use crate::plugin_bridge::egui_mesh_surface::EguiMeshSurface;

/// kind / i18n 키 문자열을 정적화. 같은 입력에 대해 leak 이 반복되지 않도록
/// caller(`register_egui_mesh_kind`)가 plugin hello 1회당 한 번만 호출한다.
fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// `create`/`restore` 클로저 공용 — params/data JSON 에서 `display_name`(없으면
/// `fallback_kind`) 과 `file` 을 뽑는다.
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

/// `(kind, plugin_id)` 쌍이 egui-mesh 채널로 허용된 bundled 조합인지 확인.
///
/// ADR-0028 scope 에 따라 image(B2 하이브리드 — 비트맵을 plugin egui 텍스처로 올려 mesh 로
/// 렌더)를 소비자로 둔다. markdown(B1) 은 Stage B 에서 `rendering = "webview"` 로 이관돼
/// 더 이상 이 화이트리스트에 없다 — `[[contributes.popup]]` 확인 팝업(large-file/file-open)
/// 은 여전히 egui-mesh 지만, popup 채널은 이 kind 화이트리스트와 무관한 별도 경로다.
pub(crate) fn is_egui_mesh_allowed(kind: &str, plugin_id: &str) -> bool {
    matches!(
        (kind, plugin_id),
        ("image", "com.tasty.image") | ("mesh_demo", "com.tasty.mesh-demo")
    )
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

/// `SurfaceKindDef` 를 조립해 registry 에 등록. 반환값은 leak 된 kind 문자열
/// (호출자의 로그용).
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
        display_name_i18n_key: i18n_key_static,
        icon: decl.icon.clone(),
        // 생성 params 의 `file` 을 stand-in 에 보관한다. plugin 에 surface.create 를
        // 알리는 것은 host_cmd 채널이 아니라 첫 set_context bootstrap 직전에 직접
        // 송신한다(`MainView::forward_egui_mesh_context`) — 같은 plugin req 채널 FIFO 라
        // create 가 set_context 보다 먼저 도착해, set_context-before-create 레이스를 없앤다.
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
            // file 이 없으면 키 자체를 뺀다(null 을 싣지 않는다). preset 은 TOML 로
            // 저장하는데 TOML 은 null 을 표현하지 못해, file=null 을 실으면 이 surface 가
            // 든 pane 의 preset capture 전체가 "unsupported unit type" 으로 깨진다
            // (layout 은 JSON 이라 담겨서 이 결함이 preset 에서만 드러났다). markdown/html
            // snapshot 도 값이 없으면 키를 빼는 같은 관례를 쓴다.
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
        // decl 은 local popup id 만 안다 — 등록 시점에 소유 plugin_id 로 qualify 해
        // host 가 `open_popup_instance` 로 곧장 열 수 있는 `<plugin>/<popup>` 형태로 만든다.
        convert_input_popup: decl
            .convert_input_popup
            .as_ref()
            .map(|p| format!("{plugin_id}/{p}")),
    });
    kind_static
}

/// `create`/`restore` 클로저 공용 본체 — display_name/file 을 뽑아
/// `EguiMeshSurface` stand-in 을 만든다.
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
        // toml 파싱 대신 직접 구성 — 본 모듈의 게이트만 검증한다.
        serde_json::from_value(json!({
            "kind": kind,
            "display_name_i18n_key": "surface.kind.markdown",
            "rendering": "egui-mesh",
        }))
        .unwrap()
    }

    #[test]
    fn markdown_is_no_longer_egui_mesh_allowed() {
        // Stage B: markdown 은 webview 로 이관돼 이 화이트리스트에서 빠졌다 — 확인 팝업
        // (large-file/file-open)은 여전히 egui-mesh 지만 별도 popup 채널이라 무관하다.
        assert!(!is_egui_mesh_allowed("markdown", "com.tasty.markdown"));
    }

    #[test]
    fn image_allowed_for_image_plugin() {
        assert!(is_egui_mesh_allowed("image", "com.tasty.image"));
        // 다른 plugin 이 image kind 를 가로채지 못한다.
        assert!(!is_egui_mesh_allowed("image", "com.example.evil"));
        // image plugin 이 mesh_demo kind 를 가로채지 못한다 (조합 매칭).
        assert!(!is_egui_mesh_allowed("mesh_demo", "com.tasty.image"));
    }

    #[test]
    fn mesh_demo_allowed_for_demo_plugin() {
        assert!(is_egui_mesh_allowed("mesh_demo", "com.tasty.mesh-demo"));
        // 다른 plugin 이 demo kind 를 가로채지 못한다.
        assert!(!is_egui_mesh_allowed("mesh_demo", "com.example.evil"));
        // demo plugin 이 image kind 를 가로채지 못한다 (조합 매칭).
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
        // snapshot → restore 라운드트립.
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert_eq!(snap["display_name"], "Readme");
        let restored = (def.restore)(5, &snap).unwrap();
        assert_eq!(restored.kind(), "mesh_demo");
        assert_eq!(restored.display_name(), "Readme");
    }

    #[test]
    fn snapshot_omits_file_key_when_file_absent() {
        // preset 은 TOML 로 저장하는데 TOML 은 null 을 표현하지 못한다 — file 없는
        // egui-mesh surface 의 snapshot 이 file:null 을 실으면 그 surface 가 든 pane 의
        // preset capture 가 통째로 "unsupported unit type" 으로 깨진다(layout 은 JSON 이라
        // 담겨서 이 결함이 preset 에서만 드러났다). 값이 없으면 키 자체를 뺀다.
        let reg = SurfaceKindRegistry::new();
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.mesh-demo",
            &decl("mesh_demo"),
            HOST_API_VERSION
        ));
        let def = reg.get("mesh_demo").unwrap();
        // file 없이 생성 → snapshot 에 file 키가 아예 없어야 한다(null 아님).
        let s = (def.create)(7, None, &json!({ "display_name": "Demo" })).unwrap();
        let snap = (def.snapshot)(s.as_ref()).unwrap();
        assert!(
            snap.get("file").is_none(),
            "file 없으면 snapshot 에 file 키가 없어야 한다(preset TOML 은 null 불가): {snap}"
        );
        // file 이 있으면 그대로 실린다.
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
        // decl 의 capability flag(zoomable/egui_copy)가 SurfaceKindDef 로 전달되는지.
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
        // 두 번째 호출: registry 에 이미 있으므로 no-op + 성공 반환.
        assert!(register_egui_mesh_kind(
            &reg,
            "com.tasty.mesh-demo",
            &decl("mesh_demo"),
            HOST_API_VERSION
        ));
        assert!(reg.contains("mesh_demo"));
    }
}
