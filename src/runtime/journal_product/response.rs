//! Public response values are frozen in the same commit as their structural result.
use serde::{Deserialize, Serialize};
use tasty_domain::{Rejection, StructureModels};
use tasty_ipc::protocol::JsonRpcResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum ResponsePlan {
    Fixed(JsonRpcResponse),
    CategoryCreated {
        stream: String,
        id: u32,
    },
    WorkspaceUpdated {
        stream: String,
        id: u32,
        display_index: usize,
    },
}

impl ResponsePlan {
    pub(super) fn render(&self, after: &StructureModels) -> Result<Vec<u8>, Rejection> {
        let response = match self {
            Self::Fixed(response) => response.clone(),
            Self::WorkspaceUpdated {
                stream,
                id,
                display_index,
            } => {
                let workspace = after
                    .streams
                    .get(stream)
                    .and_then(|model| model.workspaces.get(id))
                    .ok_or_else(|| Rejection("updated workspace response target missing".into()))?;
                JsonRpcResponse::success(
                    serde_json::Value::Null,
                    serde_json::json!({
                        "id": id, "name": workspace.name, "subtitle": workspace.subtitle,
                        "description": workspace.description, "index": display_index,
                        "category": workspace.category, "attach_mapping": workspace.attach_mapping,
                    }),
                )
            }
            Self::CategoryCreated { stream, id } => {
                let category = after
                    .streams
                    .get(stream)
                    .and_then(|model| model.categories.get(id))
                    .ok_or_else(|| Rejection("created category response target missing".into()))?;
                JsonRpcResponse::success(
                    serde_json::Value::Null,
                    serde_json::json!({"id":id,"name":category.name}),
                )
            }
        };
        serde_json::to_vec(&response).map_err(|error| Rejection(error.to_string()))
    }

    pub(super) fn rejected(error: &Rejection) -> Vec<u8> {
        serde_json::to_vec(&JsonRpcResponse::invalid_params(
            serde_json::Value::Null,
            error.to_string(),
        ))
        .expect("error response serializes")
    }
}
