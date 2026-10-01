//! image.open과 image.list는 호스트에서 처리한다. 픽셀 편집은 image 플러그인으로 전달한다.
//! open은 surface_id를 명시하고 list는 모든 이미지 surface를 조회한다.

use crate::runtime::engine_access::EngineRef;
use serde_json::{Value, json};

use tasty_ipc::protocol::JsonRpcResponse;

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
    #[test]
    fn list_finds_image_surfaces_without_executing_a_conversion() {
        use tasty_core::{DomainEvent as E, SurfaceSpec};
        let (_, mut owner) =
            crate::state::tests::test_state_from_model(crate::state::tests::test_model(vec![
                E::CategoryCreated {
                    id: 0,
                    name: "normal".into(),
                    index: 0,
                },
                E::WorkspaceCreated {
                    id: 1,
                    name: "workspace".into(),
                    category: 0,
                    index: 0,
                    pane: 1,
                },
                E::TabCreated {
                    id: 1,
                    pane: 1,
                    index: 0,
                    name: "image".into(),
                    surface: SurfaceSpec {
                        id: 1,
                        kind: "image".into(),
                        data: None,
                    },
                },
            ]));
        owner.runtime.surfaces.insert(
            1,
            Box::new(crate::runtime::egui_mesh_surface::EguiMeshSurface::new(
                1,
                "image",
                "com.tasty.image".into(),
                "image".into(),
                Some("/image.png".into()),
            )),
        );
        let response = handle_list(&owner.as_ref(), json!(1));
        assert_eq!(
            response.result,
            Some(json!({"entries":[{"surface_id":1,"path":"/image.png"}]}))
        );
    }
}
