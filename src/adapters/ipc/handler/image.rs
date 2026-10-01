//! image.open과 image.list는 호스트에서 처리한다. 픽셀 편집은 image 플러그인으로 전달한다.
//! open은 surface_id를 명시하고 list는 모든 이미지 surface를 조회한다.

use crate::runtime::engine_access::EngineRef;
use serde_json::{Value, json};

use tasty_ipc::protocol::JsonRpcResponse;

use super::require_surface_id;

pub fn handle_list(engine: &EngineRef<'_>, id: Value) -> JsonRpcResponse {
    let mut entries: Vec<Value> = Vec::new();
    for workspace in &engine.workspaces() {
        for pid in workspace.pane_layout().all_pane_ids() {
            if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    collect_image_panels(engine, tab.layout(), &mut entries);
                }
            }
        }
    }
    JsonRpcResponse::success(id, json!({ "entries": entries }))
}

fn collect_image_panels(
    engine: &EngineRef<'_>,
    layout: &crate::model::SurfaceLayout,
    out: &mut Vec<Value>,
) {
    match layout {
        crate::model::SurfaceLayout::Leaf(surface) => {
            // dir_count/current_index는 플러그인이 관리하므로 여기서는 surface_id/path만 반환한다.
            if let Some(ms) = engine.find_surface_by_id(surface.id).and_then(|surface| {
                surface
                    .as_any()
                    .downcast_ref::<crate::runtime::egui_mesh_surface::EguiMeshSurface>()
            }) && ms.kind_static == "image"
            {
                out.push(json!({
                    "surface_id": ms.id,
                    "path": ms.file,
                }));
            }
        }
        crate::model::SurfaceLayout::Split { first, second, .. } => {
            collect_image_panels(engine, first, out);
            collect_image_panels(engine, second, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::RequestContext;

    // TempDir을 유지해야 시험 도중 파일이 사라지지 않는다.
    // 플러그인을 실행하지 않으므로 image kind는 시험에서 직접 등록한다.
    fn make_test_core_state() -> (
        crate::app::services::AppServices,
        RequestContext,
        crate::runtime::engine_session::EngineSession,
        tempfile::TempDir,
    ) {
        use std::sync::{Arc, Mutex};
        use tasty_memory::MemoryStorage;
        use tasty_themes::{ThemeStorage, ThemeStore};

        use crate::adapters::test::{
            fake_clock::FakeClock, mem_fs::MemFileSystem, mock_clipboard::MockClipboard,
            mock_process::MockProcessSpawner, tmp_home::TmpHome,
        };
        use crate::app::services::builder::AppServicesBuilder;
        use crate::ports::notification_sound::NoopPlayer;

        let term_waker: crate::terminal::Waker = Arc::new(|| {});

        let mut engine_session =
            crate::runtime::engine_session::EngineSession::new(80, 24, term_waker).unwrap();
        let mut engine = engine_session.borrow_mut();

        let preset_store: Arc<Mutex<tasty_presets::PresetStore>> =
            Arc::new(Mutex::new(tasty_presets::PresetStore::load_default()));
        let memory: Arc<Mutex<dyn MemoryStorage>> =
            Arc::new(Mutex::new(tasty_memory::testing::InMemoryStorage::new()));
        let themes: Arc<dyn ThemeStorage> = Arc::new(ThemeStore::new());

        let state = RequestContext::new(&mut engine, preset_store.clone(), memory.clone());
        let decl: tasty_plugin_manifest::SurfaceKindDecl = serde_json::from_value(json!({
            "kind": "image",
            "display_name_i18n_key": "surface.kind.image",
            "rendering": "egui-mesh",
        }))
        .expect("test SurfaceKindDecl");
        assert!(
            crate::runtime::surface_registry::egui_mesh::register_egui_mesh_kind(
                &engine.runtime.surface_registry,
                "com.tasty.image",
                &decl,
                crate::plugin::manifest::HOST_API_VERSION,
            )
        );

        let home_tmp = tempfile::tempdir().expect("test tempdir");
        let home = TmpHome::new(home_tmp.path().to_path_buf());

        let core = AppServicesBuilder::new()
            .with_fs(Arc::new(MemFileSystem::new()))
            .with_clock(Arc::new(FakeClock::default()))
            .with_clipboard(Arc::new(MockClipboard::default()))
            .with_process(Arc::new(MockProcessSpawner))
            .with_home(Arc::new(home))
            .with_sound_player(Arc::new(NoopPlayer))
            .with_memory(memory)
            .with_themes(themes)
            .with_preset_store(preset_store)
            .with_settings_storage(Arc::new(tasty_settings::FileSettingsStorage))
            .build()
            .expect("test AppServices build");

        (core, state, engine_session, home_tmp)
    }

    fn first_surface_id(state: &mut RequestContext, engine: &mut EngineMut<'_>) -> u32 {
        let ws_ids: std::collections::HashSet<u32> = state
            .active_workspace_mut(engine)
            .all_surface_ids()
            .into_iter()
            .collect();
        engine
            .runtime
            .terminals
            .iter()
            .find_map(|(sid, _)| ws_ids.contains(&sid).then_some(sid))
            .expect("no terminal surface in active workspace")
    }

    fn write_blank_png(dir: &std::path::Path, name: &str) -> String {
        let path = dir.join(name);
        let file = std::fs::File::create(&path).expect("create png");
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer
            .write_image_data(&[0u8; 2 * 2 * 4])
            .expect("png write");
        writer.finish().expect("png finish");
        path.to_string_lossy().into_owned()
    }

    #[test]
    fn open_converts_surface_to_image_with_path() {
        let (mut core, mut state, mut engine_session, _home_tmp) = make_test_core_state();
        let mut engine = engine_session.borrow_mut();
        let sid = first_surface_id(&mut state, &mut engine);
        let tmp = tempfile::tempdir().unwrap();
        let path = write_blank_png(tmp.path(), "a.png");

        let resp = handle_open(
            &mut core,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid, "path": path.clone() }),
        );
        assert!(resp.result.is_some(), "open failed: {resp:?}");
        let resp = handle_list(&engine, Value::Null);
        let v = resp.result.expect("list ok");
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["surface_id"], sid);
        assert_eq!(entries[0]["path"], path.as_str());
    }

    #[test]
    fn open_rejects_missing_path() {
        let (mut core, mut state, mut engine_session, _home_tmp) = make_test_core_state();
        let mut engine = engine_session.borrow_mut();
        let sid = first_surface_id(&mut state, &mut engine);
        let resp = handle_open(
            &mut core,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn open_rejects_unknown_surface() {
        let (mut core, _state, mut engine_session, _home_tmp) = make_test_core_state();
        let mut engine = engine_session.borrow_mut();
        let resp = handle_open(
            &mut core,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": 999_999, "path": "/tmp/x.png" }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn list_finds_image_surfaces() {
        let (_core, mut state, mut engine_session, _home_tmp) = make_test_core_state();
        let mut engine = engine_session.borrow_mut();
        let sid = first_surface_id(&mut state, &mut engine);
        assert!(state.test_convert_surface_to_kind(&mut engine, sid, "image", &json!({})));

        let resp = handle_list(&engine, Value::Null);
        let v = resp.result.expect("list ok");
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["surface_id"], sid);
    }
}
