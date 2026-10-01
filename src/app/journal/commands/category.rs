//! Legacy category parameter and response semantics, resolved once after identity admission.
use super::*;
use crate::ipc::handler::params;
use tasty_core::StructuralCommand;

pub(super) enum Resolved {
    Create(String),
    Apply(StructuralCommand, JsonRpcResponse),
}

pub(super) fn resolve(
    request: &JsonRpcRequest,
    core: &crate::core::CoreState,
) -> Result<Resolved, JsonRpcResponse> {
    let id = serde_json::Value::Null;
    let input = &request.params;
    let bad = |message: &str| JsonRpcResponse::invalid_params(id.clone(), message);
    let name = || {
        input
            .get("name")
            .and_then(|value| value.as_str())
            .map(str::to_owned)
            .ok_or_else(|| bad("Missing required 'name' parameter"))
    };
    let success = |value| JsonRpcResponse::success(id.clone(), value);
    Ok(match request.method.as_str() {
        "workspace_category.create" => Resolved::Create(name()?),
        "workspace_category.rename" => {
            let category = params::opt_int::<u64>(input, "id", &id)?
                .ok_or_else(|| bad("Missing required 'id' parameter"))?;
            let name = name()?;
            Resolved::Apply(
                StructuralCommand::RenameCategory {
                    id: category as u32,
                    name: name.clone(),
                },
                success(serde_json::json!({"id":category,"name":name})),
            )
        }
        "workspace_category.delete" => {
            let category = params::require_u32(input, "id", &id)?;
            Resolved::Apply(
                StructuralCommand::DeleteCategory { id: category },
                success(serde_json::json!({"deleted":true,"id":category})),
            )
        }
        "workspace_category.move" => {
            let named = params::opt_int::<u64>(input, "id", &id)?;
            let from = params::opt_int::<u64>(input, "from_index", &id)?;
            let from = match (named, from) {
                (Some(_), Some(_)) => {
                    return Err(bad(
                        "give either 'id' (the category to move) or 'from_index', not both",
                    ));
                }
                (Some(category), None) => core
                    .category_index(category as u32)
                    .ok_or_else(|| bad(&format!("no workspace category {category}")))?,
                (None, Some(index)) => index as usize,
                (None, None) => return Err(bad("Missing 'id' or 'from_index' parameter")),
            };
            let to = params::opt_int::<u64>(input, "to_index", &id)?
                .ok_or_else(|| bad("Missing 'to_index' parameter"))? as usize;
            if from == 0 || to == 0 {
                return Err(bad("the 'normal' category is fixed at position 0"));
            }
            let category = core
                .categories()
                .get(from)
                .ok_or_else(|| bad("category index out of range"))?
                .id;
            Resolved::Apply(
                StructuralCommand::ReorderCategory {
                    id: category,
                    to_index: to,
                },
                success(serde_json::json!({"moved":true})),
            )
        }
        _ => return Err(bad("unsupported structural request")),
    })
}
