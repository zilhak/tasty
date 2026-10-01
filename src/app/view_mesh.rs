//! App-owned plugin/attach relay; the View receives only local render cache updates.
use crate::plugin::PluginManager;
use tasty_ipc::stream_hub::StreamHub;
use tasty_plugin_protocol::protocol::{ModifiersWire, RawInputWire, SurfaceSetContextParams};
/// GUI가 만든 mesh를 attach 구독자에게 중계한다. 평소에는 새 컨텍스트를 만들지 않는다.
/// 아직 한 번도 렌더하지 않은 surface만 초기 생성·컨텍스트를 보낸다.
/// 기존 surface의 새 구독·복구 요청은 pending_full에 넣고 해당 tick의 중계를 생략한다.
pub(super) fn relay_subscribed_mesh(
    engine: &mut crate::runtime::engine_access::EngineMut<'_>,
    mgr: &PluginManager,
    stream_hub: &StreamHub,
) -> Vec<MeshViewUpdate> {
    let mut updates = Vec::new();
    for sid in engine.remote.mesh_mirror.active_surface_ids() {
        let Some(ctx) = engine.remote.mesh_mirror.get(sid) else {
            continue;
        };
        let client_id = ctx.client_id;
        let width_px = ctx.width_px;
        let height_px = ctx.height_px;
        let pixels_per_point = ctx.pixels_per_point;
        let theme = ctx.theme.clone();
        let focused = ctx.focused;

        let need_full = engine.remote.mesh_mirror.take_need_full_textures(sid);
        // 컨텍스트 전송은 기존 로컬 경로가 맡으며 여기서는 변경 표시만 비운다.
        let _ = engine.remote.mesh_mirror.take_dirty(sid);

        if mgr.egui_mesh_frame(sid).is_none() {
            let Some(ms) = engine.runtime.surfaces.get(&sid).and_then(|surface| {
                surface
                    .as_any()
                    .downcast_ref::<crate::runtime::egui_mesh_surface::EguiMeshSurface>()
            }) else {
                engine.remote.mesh_mirror.remove(sid);
                continue;
            };
            let plugin_id = ms.plugin_id.clone();
            mgr.send_egui_mesh_surface_create(
                &plugin_id,
                sid,
                ms.kind_static,
                ms.file.as_deref(),
                &ms.display_name,
                &ms.retirement_binding,
            );
            mgr.send_surface_set_context(
                &plugin_id,
                &SurfaceSetContextParams {
                    surface_id: sid,
                    width_px,
                    height_px,
                    pixels_per_point,
                    raw_input: RawInputWire {
                        time: None,
                        focused,
                        modifiers: ModifiersWire::default(),
                        events: Vec::new(),
                    },
                    theme: theme.clone(),
                    need_full_textures: true,
                },
            );

            updates.push(MeshViewUpdate::Bootstrap {
                surface: sid,
                plugin: plugin_id,
                width: width_px,
                height: height_px,
                ppp: pixels_per_point,
                theme,
                focused,
            });
        } else if need_full {
            // Windowed rendering uses local View geometry. The returned request is
            // queued as LocalMesh on the next frame; parked engines use mesh_forward.
            updates.push(MeshViewUpdate::Full { surface: sid });
            continue;
        }

        crate::plugin_bridge::mesh_forward::relay_mesh_frame_if_new(
            &mut *engine,
            mgr,
            stream_hub,
            sid,
            client_id,
        );
    }
    updates
}

pub(super) enum MeshViewUpdate {
    Bootstrap {
        surface: u32,
        plugin: String,
        width: u32,
        height: u32,
        ppp: f32,
        theme: Option<tasty_plugin_protocol::protocol::ThemeWire>,
        focused: bool,
    },
    Full {
        surface: u32,
    },
}
