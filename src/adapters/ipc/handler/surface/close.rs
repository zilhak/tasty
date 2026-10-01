//! Public close occupancy guard; execution belongs to journal retirement.
use tasty_ipc::protocol::JsonRpcResponse;
pub(in crate::adapters::ipc::handler) fn refuse_if_hard_occupied(
    engine: &crate::runtime::engine_access::EngineRef<'_>,
    id: &serde_json::Value,
    surface_id: u32,
) -> Option<JsonRpcResponse> {
    if !engine.live.occupancy.is_hard_occupied(surface_id) {
        return None;
    }
    Some(JsonRpcResponse::invalid_params(
        id.clone(),
        format!(
            "Surface {surface_id} is occupied by a remote attach session (hard-occupied) \
             — someone is working in that terminal right now. Release it from the \
             attaching instance first."
        ),
    ))
}
