//! 플러그인 surface kind를 등록하고 RemoteSurface 생성·복원 요청을 매니저에 전달한다.
//! 이름과 snapshot은 플러그인 응답을 받은 뒤 공유 핸들로 갱신한다.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::model::Surface;

use crate::plugin::manifest::SurfaceKindDecl;
use crate::plugin_bridge::host_cmd::HostCmd;
use crate::plugin_bridge::remote_surface::RemoteSurface;
use crate::runtime::surface_registry::{
    KindSource, RegisteredRendering, SurfaceKindDef, SurfaceKindRegistry,
};

// Surface::kind가 static 문자열을 요구한다. 같은 kind를 다시 등록해도 추가 할당하며 해제하지 않는다.
fn leak_kind(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

// 같은 등록 함수를 쓰는 remote와 webview를 실제 렌더링 정보에서는 구분한다.
fn registered_rendering(decl: &SurfaceKindDecl) -> RegisteredRendering {
    match decl.rendering {
        crate::plugin::manifest::SurfaceKindRendering::Webview => RegisteredRendering::Webview,
        _ => RegisteredRendering::Remote,
    }
}

/// kind를 등록한다. host_cmd_tx는 surface 생성·복원 요청을 매니저에 전달한다.
pub fn register_remote_kind(
    registry: &SurfaceKindRegistry,
    plugin_id: &str,
    decl: &SurfaceKindDecl,
    host_cmd_tx: Sender<HostCmd>,
) {
    if crate::runtime::surface_registry::builtins::is_host_builtin_kind(&decl.kind) {
        tracing::warn!(
            "plugin '{}' declared remote kind '{}' which is a host builtin; ignoring \
             (host-rendered surface takes precedence)",
            plugin_id,
            decl.kind
        );
        return;
    }

    let kind_static: &'static str = leak_kind(&decl.kind);
    let i18n_key_static: &'static str = leak_str(&decl.display_name_i18n_key);
    let plugin_id_owned = plugin_id.to_string();

    let plugin_id_for_create = plugin_id_owned.clone();
    let plugin_id_for_restore = plugin_id_owned;
    let tx_create = host_cmd_tx.clone();
    let tx_restore = host_cmd_tx;
    let preset_fields =
        crate::runtime::surface_registry::PresetFieldSpec::from_decls(&decl.preset_fields);
    let preset_fields_for_create = preset_fields.clone();

    registry.register(SurfaceKindDef {
        kind: kind_static,
        rendering: registered_rendering(decl),
        source: KindSource::Plugin(plugin_id.to_string()),
        display_name_i18n_key: i18n_key_static,
        icon: decl.icon.clone(),
        create: Arc::new(move |sid, cwd, params| {
            let initial_name = params
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| kind_static.to_string());
            let surface =
                RemoteSurface::new(sid, kind_static, plugin_id_for_create.clone(), initial_name)
                    .with_initial_file(params);
            // 선언한 파일 경로에서 얻은 cwd를 상속 cwd보다 우선한다.
            // 이 값은 호스트 source_cwd용이며 플러그인에 보내는 cwd는 원래 인자를 유지한다.
            let surface_cwd = crate::runtime::surface_registry::PresetFieldSpec::derive_cwd(
                &preset_fields_for_create,
                params,
            )
            .or_else(|| cwd.map(std::path::PathBuf::from));
            surface.set_cwd(surface_cwd);
            let handles = surface.handles();
            let command = HostCmd::RemoteSurfaceCreated {
                surface_id: sid,
                plugin_id: plugin_id_for_create.clone(),
                kind: kind_static.to_string(),
                cwd: cwd.map(std::path::PathBuf::from),
                params: params.clone(),
                handles,
            };
            let sender = tx_create.clone();
            Ok(crate::runtime::surface_registry::PreparedKind::deferred(
                Box::new(surface),
                Box::new(move || {
                    sender.send(command).map_err(|error| {
                        anyhow::anyhow!("RemoteSurfaceCreated host command failed: {error}")
                    })
                }),
            ))
        }),
        restore: Arc::new(move |sid, data| {
            let surface = RemoteSurface::new(
                sid,
                kind_static,
                plugin_id_for_restore.clone(),
                kind_static.to_string(),
            );
            // 복원 응답을 받기 전에 다시 저장해도 기존 snapshot을 유지한다.
            surface.cache_snapshot(data.clone());
            let handles = surface.handles();
            let command = HostCmd::RemoteSurfaceRestored {
                surface_id: sid,
                plugin_id: plugin_id_for_restore.clone(),
                kind: kind_static.to_string(),
                data: data.clone(),
                handles,
            };
            let sender = tx_restore.clone();
            Ok(crate::runtime::surface_registry::PreparedKind::deferred(
                Box::new(surface),
                Box::new(move || {
                    sender.send(command).map_err(|error| {
                        anyhow::anyhow!("RemoteSurfaceRestored host command failed: {error}")
                    })
                }),
            ))
        }),
        snapshot: Arc::new(|s: &dyn Surface| {
            let any = s.as_any();
            let rs = any.downcast_ref::<RemoteSurface>()?;
            // poison 복구로 snapshot을 유지하고 최초 오류를 기록한다.
            crate::poison::recover_mutex(
                rs.snapshot_cache.lock(),
                super::remote_surface::SNAPSHOT_WHAT,
                &super::remote_surface::SNAPSHOT_POISON_REPORTED,
            )
            .clone()
        }),
        preset_fields,
        param_aliases: decl.param_aliases.clone(),
        default_params: decl.default_params.clone(),
        consumes_egui_input: decl.consumes_egui_input,
        zoomable: decl.zoomable,
        egui_copy: decl.egui_copy,
        copy_path: decl.copy_path,
        egui_paste: decl.egui_paste,
        name_from_param: decl.name_from_param.clone(),
        tab_name_from_params: None,
        records_recent: decl.records_recent,
        convert_requires_input: decl.convert_requires_input,
        convert_input_popup: decl
            .convert_input_popup
            .as_ref()
            .map(|p| format!("{plugin_id}/{p}")),
    });

    tracing::info!(
        "registered remote surface kind '{}' from plugin '{}'",
        kind_static,
        plugin_id
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_same_id_candidate_does_not_publish_or_mutate_the_original_before_install() {
        let registry = SurfaceKindRegistry::new();
        let (sender, receiver) = std::sync::mpsc::channel();
        let decl = serde_json::from_value(serde_json::json!({
            "kind":"test-document", "display_name_i18n_key":"surface.kind.markdown", "rendering":"remote"
        })).unwrap();
        register_remote_kind(&registry, "com.test.document", &decl, sender);
        let definition = registry.get_live("test-document").unwrap();
        let old = (definition.create)(41, None, &serde_json::json!({"display_name":"original"}))
            .unwrap()
            .publish()
            .unwrap();
        let HostCmd::RemoteSurfaceCreated {
            handles: old_handles,
            ..
        } = receiver.try_recv().unwrap()
        else {
            panic!("create")
        };
        let candidate =
            (definition.create)(41, None, &serde_json::json!({"display_name":"candidate"}))
                .unwrap();
        assert!(receiver.try_recv().is_err());
        drop(candidate);
        assert!(receiver.try_recv().is_err());
        assert_eq!(old.display_name(), "original");
        assert_eq!(*old_handles.display_name.lock().unwrap(), "original");
        let candidate =
            (definition.create)(41, None, &serde_json::json!({"display_name":"replacement"}))
                .unwrap();
        candidate.publish().unwrap();
        let HostCmd::RemoteSurfaceCreated {
            handles: new_handles,
            ..
        } = receiver.try_recv().unwrap()
        else {
            panic!("create")
        };
        assert!(!old_handles.binding().matches(&new_handles));
        assert_eq!(*old_handles.display_name.lock().unwrap(), "original");
        assert_eq!(*new_handles.display_name.lock().unwrap(), "replacement");
    }
}
