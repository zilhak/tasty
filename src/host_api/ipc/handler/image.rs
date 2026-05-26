//! `image.*` IPC 핸들러. plugin/CLI/외부 클라이언트가 image surface를 조작하기 위한
//! "얇은 어댑터" — 호스트의 ImagePanel/ImageView/ImageViewStore에 위임한다.
//!
//! 모든 메서드는 `surface_id`를 명시적으로 받는다 (포커스 독립성 원칙).

use serde_json::{Value, json};

use crate::ipc::protocol::JsonRpcResponse;
use crate::state::AppState;

use super::require_surface_id;

/// `image.open { surface_id, path }` — surface를 image kind로 (재)설정 + 파일 로드.
pub fn handle_open(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let sid = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let path = match params.get("path").and_then(|v| v.as_str()) {
        Some(p) => p.to_string(),
        None => return JsonRpcResponse::invalid_params(id, "Missing required 'path' parameter"),
    };
    let ok = state.convert_surface_to_kind(engine, sid, "image", &json!({ "file": path.clone() }));
    if !ok {
        return JsonRpcResponse::invalid_params(id, format!("Surface {sid} not found"));
    }
    // 이전 ImageView가 남아 있으면 다음 렌더에서 새 path 픽셀이 로드되지 않을 수 있다.
    state.image_views.drop_view(sid);
    JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": sid, "path": path }))
}

/// `image.save { surface_id, path? }` — 현재 픽셀 버퍼를 PNG로 저장. path 생략 시
/// `ImagePanel::save_path()` 사용 (열려 있는 파일의 `.png` 확장자 버전).
pub fn handle_save(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let sid = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let explicit = params
        .get("path")
        .and_then(|v| v.as_str())
        .map(String::from);
    let final_path = match explicit {
        Some(p) => p,
        None => match state
            .image_panel_mut(engine, sid)
            .and_then(|p| p.save_path())
        {
            Some(p) => p,
            None => {
                return JsonRpcResponse::invalid_params(
                    id,
                    "No save path: provide 'path' or open a file first",
                );
            }
        },
    };

    let result = match state.image_views.get_mut(sid) {
        Some(view) => view.save_png(&final_path),
        None => {
            return JsonRpcResponse::invalid_params(
                id,
                format!("Image surface {sid} has no view yet (render it first)"),
            );
        }
    };
    match result {
        Ok(()) => {
            if let Some(panel) = state.image_panel_mut(engine, sid) {
                if panel.is_blank() {
                    panel.assign_file_path(final_path.clone());
                }
            }
            JsonRpcResponse::success(id, json!({ "ok": true, "path": final_path }))
        }
        Err(e) => JsonRpcResponse::internal_error(id, format!("save failed: {e}")),
    }
}

/// `image.export_png { surface_id, path }` — `save`와 동일하되 path 필수.
pub fn handle_export_png(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    if params.get("path").and_then(|v| v.as_str()).is_none() {
        return JsonRpcResponse::invalid_params(id, "Missing required 'path' parameter");
    }
    handle_save(state, engine, id, params)
}

/// `image.next { surface_id }` — 디렉터리 내 다음 이미지로 이동.
pub fn handle_next(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    step_navigation(state, engine, id, params, true)
}

/// `image.prev { surface_id }` — 디렉터리 내 이전 이미지로 이동.
pub fn handle_prev(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    step_navigation(state, engine, id, params, false)
}

fn step_navigation(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: Value,
    params: &Value,
    forward: bool,
) -> JsonRpcResponse {
    let sid = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let mut image_views = std::mem::take(&mut state.image_views);
    let result = if let Some(panel) = state.image_panel_mut(engine, sid) {
        let new_path = if forward {
            panel.step_next()
        } else {
            panel.step_prev()
        };
        match new_path {
            Some(path) => {
                if let Some(view) = image_views.get_mut(sid) {
                    view.load_after_navigation(panel);
                }
                Ok(path)
            }
            None => Err("No sibling images available".to_string()),
        }
    } else {
        Err(format!("Surface {sid} is not an image"))
    };
    state.image_views = image_views;

    match result {
        Ok(path) => JsonRpcResponse::success(id, json!({ "ok": true, "path": path })),
        Err(e) => JsonRpcResponse::invalid_params(id, e),
    }
}

/// `image.paste { surface_id }` — 시스템 클립보드의 이미지를 floating selection으로 paste.
pub fn handle_paste(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    id: Value,
    params: &Value,
) -> JsonRpcResponse {
    let sid = match require_surface_id(params, &id) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let mut cb = match arboard::Clipboard::new() {
        Ok(cb) => cb,
        Err(e) => {
            return JsonRpcResponse::internal_error(id, format!("clipboard open failed: {e}"));
        }
    };
    let image = match cb.get_image() {
        Ok(img) => img,
        Err(e) => {
            return JsonRpcResponse::invalid_params(id, format!("no image on clipboard: {e}"));
        }
    };

    let pixels: Vec<egui::Color32> = image
        .bytes
        .chunks_exact(4)
        .map(|c| egui::Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3]))
        .collect();
    let color_image = egui::ColorImage {
        size: [image.width, image.height],
        pixels,
    };

    let mut image_views = std::mem::take(&mut state.image_views);
    let pasted = if let Some(panel) = state.image_panel_mut(engine, sid) {
        let view = image_views.get_or_init(panel);
        view.paste_image(color_image);
        true
    } else {
        false
    };
    state.image_views = image_views;

    if pasted {
        JsonRpcResponse::success(id, json!({ "ok": true, "surface_id": sid }))
    } else {
        JsonRpcResponse::invalid_params(id, format!("Surface {sid} is not an image"))
    }
}

/// `image.list` — 열린 모든 image surface 목록.
pub fn handle_list(
    _state: &AppState,
    engine: &crate::engine_state::EngineState,
    id: Value,
) -> JsonRpcResponse {
    let mut entries: Vec<Value> = Vec::new();
    for workspace in &engine.workspaces {
        for pid in workspace.pane_layout().all_pane_ids() {
            if let Some(pane) = workspace.pane_layout().find_pane(pid) {
                for tab in &pane.tabs {
                    collect_image_panels(tab.layout(), &mut entries);
                }
            }
        }
    }
    JsonRpcResponse::success(id, json!({ "entries": entries }))
}

fn collect_image_panels(layout: &crate::model::SurfaceLayout, out: &mut Vec<Value>) {
    match layout {
        crate::model::SurfaceLayout::Leaf(surface) => {
            if let Some(img) = surface.as_any().downcast_ref::<crate::model::ImagePanel>() {
                out.push(json!({
                    "surface_id": img.id,
                    "path": img.file_path,
                    "dir_count": img.dir_images.len(),
                    "current_index": img.current_index,
                }));
            }
        }
        crate::model::SurfaceLayout::Split { first, second, .. } => {
            collect_image_panels(first, out);
            collect_image_panels(second, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::AppState;

    fn make_state() -> (AppState, crate::engine_state::EngineState) {
        let waker: crate::terminal::Waker = std::sync::Arc::new(|| {});
        let mut engine = crate::engine_state::EngineState::new(80, 24, waker).unwrap();
        let state = AppState::new(&mut engine);
        // image kind는 본래 com.tasty.image plugin이 hello 시 등록한다. 단위 테스트는
        // plugin 프로세스를 띄우지 않으므로 host whitelist 등록을 직접 호출한다.
        crate::engine::surface_registry::builtins::register_image(&engine.surface_registry);
        (state, engine)
    }

    fn first_surface_id(
        state: &mut AppState,
        engine: &mut crate::engine_state::EngineState,
    ) -> u32 {
        let mut ids = Vec::new();
        state
            .active_workspace_mut(engine)
            .pane_layout_mut()
            .for_each_terminal_mut(&mut |sid, _| ids.push(sid));
        ids[0]
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
        let (mut state, mut engine) = make_state();
        let sid = first_surface_id(&mut state, &mut engine);
        let tmp = tempfile::tempdir().unwrap();
        let path = write_blank_png(tmp.path(), "a.png");

        let resp = handle_open(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid, "path": path.clone() }),
        );
        assert!(resp.result.is_some(), "open failed: {resp:?}");
        let panel = state
            .image_panel_mut(&mut engine, sid)
            .expect("surface is now ImagePanel");
        assert_eq!(panel.file_path.as_deref(), Some(path.as_str()));
    }

    #[test]
    fn open_rejects_missing_path() {
        let (mut state, mut engine) = make_state();
        let sid = first_surface_id(&mut state, &mut engine);
        let resp = handle_open(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn open_rejects_unknown_surface() {
        let (mut state, mut engine) = make_state();
        let resp = handle_open(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": 999_999, "path": "/tmp/x.png" }),
        );
        assert!(resp.error.is_some());
    }

    #[test]
    fn next_prev_wrap_around_in_directory() {
        let (mut state, mut engine) = make_state();
        let sid = first_surface_id(&mut state, &mut engine);
        let tmp = tempfile::tempdir().unwrap();
        let a = write_blank_png(tmp.path(), "a.png");
        let b = write_blank_png(tmp.path(), "b.png");
        let c = write_blank_png(tmp.path(), "c.png");

        // Open with first file. After convert_surface_to_kind, ImagePanel populates
        // dir_images by scanning the directory.
        let resp = handle_open(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid, "path": a }),
        );
        assert!(resp.result.is_some());

        let resp = handle_next(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid }),
        );
        let v = resp.result.expect("next ok");
        assert_eq!(v["path"], b);

        let resp = handle_next(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid }),
        );
        assert_eq!(resp.result.unwrap()["path"], c);

        // wrap forward
        let resp = handle_next(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid }),
        );
        resp.result.expect("wraps to first");

        // step back
        let resp = handle_prev(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid }),
        );
        resp.result.expect("prev ok");
    }

    #[test]
    fn list_finds_image_surfaces() {
        let (mut state, mut engine) = make_state();
        let sid = first_surface_id(&mut state, &mut engine);
        // Convert to image (blank canvas — no file).
        assert!(state.convert_surface_to_kind(&mut engine, sid, "image", &json!({})));

        let resp = handle_list(&state, &engine, Value::Null);
        let v = resp.result.expect("list ok");
        let entries = v["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["surface_id"], sid);
    }

    #[test]
    fn save_rejects_without_view() {
        let (mut state, mut engine) = make_state();
        let sid = first_surface_id(&mut state, &mut engine);
        // Convert to image but never render → no ImageView in store.
        assert!(state.convert_surface_to_kind(&mut engine, sid, "image", &json!({})));

        let resp = handle_save(
            &mut state,
            &mut engine,
            Value::Null,
            &json!({ "surface_id": sid, "path": "/tmp/x.png" }),
        );
        assert!(resp.error.is_some());
    }
}
