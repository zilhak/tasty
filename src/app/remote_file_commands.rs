//! Remote file work reuses the process file writer. Values and secrets have no Debug output.
mod detection;
mod failure;
mod mutation;
mod status;
use crate::app::services::AppServices;
pub(crate) use detection::sink as detection_sink;
pub(crate) use failure::Failure;
pub(crate) use mutation::{Output, Request, Ticket, Value, submit};
use serde_json::Value as Json;
use tasty_ipc::protocol::JsonRpcResponse;

pub(crate) fn poll(ticket: &Ticket) -> Option<Result<Output, Failure>> {
    ticket
        .poll()
        .map(|result| result.map_err(Failure::Transport).and_then(|value| value))
}
pub(crate) fn reply(
    core: &AppServices,
    id: Json,
    request: Request,
    finish: impl FnOnce(&AppServices, Json, Output) -> JsonRpcResponse + Send + 'static,
) -> JsonRpcResponse {
    let ticket = match submit(core, request) {
        Ok(ticket) => ticket,
        Err(error) => return error.response(id),
    };
    let answer_id = id.clone();
    core.reply_with_file(
        id,
        move || poll(&ticket),
        move |core, result| match result {
            Ok(output) => finish(core, answer_id, output),
            Err(error) => error.response(answer_id),
        },
    )
}
