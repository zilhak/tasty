//! Keep the original IPC caller and its once-only continuation until registry settlement.
use crate::{
    app::services::AppServices,
    webhook::registry::{Completed, Receipt},
};
use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;
pub(super) fn answer(
    core: &AppServices,
    id: Value,
    mut receipt: Receipt,
    finish: impl FnOnce(&AppServices, Value, Completed) -> JsonRpcResponse + Send + 'static,
) -> JsonRpcResponse {
    let answer_id = id.clone();
    core.reply_with_file(id,move||receipt.poll(),move|core,result|match result {
        Ok(completed)=>finish(core,answer_id,completed),
        Err(error)=>JsonRpcResponse::error_with_data(answer_id,-32603,error.message,json!({"applied":false,"saved":if error.unknown {Value::Null}else{json!(false)},"outcome_unknown":error.unknown})),
    })
}

#[cfg(test)]
mod tests;
