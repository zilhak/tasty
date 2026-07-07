//! Plugin이 제공하는 surface kind를 SurfaceKindRegistry에 등록.
//!
//! `Surface` trait의 `kind() -> &'static str` 제약 때문에 plugin 매니페스트의
//! 동적 kind 문자열은 여기서 한 번 `Box::leak`으로 정적화한다 (plugin 종류당 1회).
//!
//! 등록된 kind의 `create`/`restore`는 빈 `RemoteSurface`를 반환한다. 실제 트리는
//! plugin이 비동기로 보내오는 `surface.create` 응답에서 set된다 (단계 06D에서 라우팅).

use std::sync::Arc;
use std::sync::mpsc::Sender;

use crate::model::Surface;

use crate::engine::surface_registry::{SurfaceKindDef, SurfaceKindRegistry};
use crate::plugin::manifest::SurfaceKindDecl;
use crate::plugin_bridge::host_cmd::HostCmd;
use crate::plugin_bridge::remote_surface::RemoteSurface;

/// kind 문자열을 정적화하여 반환. 같은 입력에 대해 leak이 반복되지 않도록
/// caller가 한 번만 호출하도록 보장해야 한다 (PluginManager가 hello 1회당 호출).
fn leak_kind(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

fn leak_str(s: &str) -> &'static str {
    Box::leak(s.to_string().into_boxed_str())
}

/// plugin manager가 hello를 받은 직후 호출. registry에 plugin kind를 등록.
/// 이미 같은 kind가 등록돼 있으면 덮어쓰며 warn 로그.
///
/// `host_cmd_tx`는 새 surface가 만들어질 때마다 manager에 등록 요청을 보내는 채널.
pub fn register_remote_kind(
    registry: &SurfaceKindRegistry,
    plugin_id: &str,
    decl: &SurfaceKindDecl,
    host_cmd_tx: Sender<HostCmd>,
) {
    // 호스트 내장 kind 보호: plugin(또는 사용자 dir 에 남은 옛 builtin)이 host 가
    // 직접 렌더하는 kind 를 remote 로 가로채지 못하게 한다. 예: T11 에서 explorer 가
    // host builtin 으로 승격된 뒤에도 `~/.tasty/plugins/com.tasty.explorer` 가 남아
    // remote "explorer" 를 재선언하면 native 렌더가 깨진다 — 여기서 차단한다.
    if crate::engine::surface_registry::builtins::is_host_builtin_kind(&decl.kind) {
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

    registry.register(SurfaceKindDef {
        kind: kind_static,
        display_name_i18n_key: i18n_key_static,
        icon: decl.icon.clone(),
        create: Arc::new(move |sid, cwd, params| {
            let initial_name = params
                .get("display_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| kind_static.to_string());
            let surface =
                RemoteSurface::new(sid, kind_static, plugin_id_for_create.clone(), initial_name);
            // host 가 carry 한 cwd 를 surface 의 source_cwd() 시발점으로 적재.
            // 이후 plugin 이 root 변경 시 `surface.set_cwd` 로 갱신.
            surface.set_cwd(cwd.map(std::path::PathBuf::from));
            let handles = surface.handles();
            if let Err(e) = tx_create.send(HostCmd::RemoteSurfaceCreated {
                surface_id: sid,
                plugin_id: plugin_id_for_create.clone(),
                kind: kind_static.to_string(),
                cwd: cwd.map(std::path::PathBuf::from),
                params: params.clone(),
                handles,
            }) {
                tracing::warn!("RemoteSurfaceCreated host cmd send failed: {e}");
            }
            Ok(Box::new(surface) as Box<dyn Surface>)
        }),
        restore: Arc::new(move |sid, data| {
            let surface = RemoteSurface::new(
                sid,
                kind_static,
                plugin_id_for_restore.clone(),
                kind_static.to_string(),
            );
            // 옛 snapshot 을 cache 의 초기값으로 carry. plugin 의 surface.restore
            // 응답이 늦거나 영영 안 와도 capture 시 옛 data 로 정상 저장됨 →
            // layout.json 이 kind="empty" 로 오염되는 race 차단.
            surface.cache_snapshot(data.clone());
            let handles = surface.handles();
            if let Err(e) = tx_restore.send(HostCmd::RemoteSurfaceRestored {
                surface_id: sid,
                plugin_id: plugin_id_for_restore.clone(),
                kind: kind_static.to_string(),
                data: data.clone(),
                handles,
            }) {
                tracing::warn!("RemoteSurfaceRestored host cmd send failed: {e}");
            }
            Ok(Box::new(surface) as Box<dyn Surface>)
        }),
        snapshot: Arc::new(|s: &dyn Surface| {
            let any = s.as_any();
            let rs = any.downcast_ref::<RemoteSurface>()?;
            rs.snapshot_cache.lock().ok()?.clone()
        }),
        preset_fields: crate::engine::surface_registry::PresetFieldSpec::from_decls(
            &decl.preset_fields,
        ),
        param_aliases: decl.param_aliases.clone(),
        default_params: decl.default_params.clone(),
        consumes_egui_input: decl.consumes_egui_input,
        zoomable: decl.zoomable,
        egui_copy: decl.egui_copy,
        copy_path: decl.copy_path,
        egui_paste: decl.egui_paste,
    });

    tracing::info!(
        "registered remote surface kind '{}' from plugin '{}'",
        kind_static,
        plugin_id
    );
}
