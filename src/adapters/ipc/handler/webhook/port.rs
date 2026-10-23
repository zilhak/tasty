//! Runtime port file work shares the ordered file queue, including read-time migration.
use crate::app::{file_writer::command, services::AppServices};
use serde_json::{Value, json};
use std::path::PathBuf;
use tasty_ipc::protocol::JsonRpcResponse;

#[derive(Clone, Copy)]
pub(super) enum Operation {
    Read,
    Set(u16),
    Clear,
}
impl Operation {
    fn execute(self, path: &std::path::Path) -> Result<Option<u16>, String> {
        use tasty_settings::webhook_port_file as file;
        match self {
            // A query returns the backend's interpreted port, including its documented
            // warning/fallback behavior. This is not a claim that migration was saved;
            // actual failed replacement attempts remain in physical operation metrics.
            Self::Read => Ok(file::read_port(path)),
            Self::Set(port) => file::set_port(path, port)
                .map(|()| Some(port))
                .map_err(|e| e.to_string()),
            Self::Clear => file::clear_port(path)
                .map(|()| None)
                .map_err(|e| e.to_string()),
        }
    }
}

pub(super) fn request(
    core: &AppServices,
    id: Value,
    operation: Operation,
    status: Option<Value>,
) -> JsonRpcResponse {
    request_at(
        core,
        id,
        operation,
        status,
        crate::webhook::config::config_path(),
    )
}

fn request_at(
    core: &AppServices,
    id: Value,
    operation: Operation,
    status: Option<Value>,
    path: PathBuf,
) -> JsonRpcResponse {
    let bytes = path.as_os_str().len().saturating_add(256);
    let ticket = command::submit_classified(
        core.file_command_queue().as_ref(),
        bytes,
        move || operation.execute(&path),
        |result| match result {
            Ok(_) => 64,
            Err(error) => error.len().saturating_add(64),
        },
        |result| command::CommandDisposition {
            outcome: if result.is_ok() {
                command::CommandOutcome::KnownSuccess
            } else {
                command::CommandOutcome::KnownFailure
            },
            partial: false,
        },
    );
    let ticket = match ticket {
        Ok(ticket) => ticket,
        Err(error) => return transport_error(id, error),
    };
    let answer_id = id.clone();
    let running_port = core.ports.webhook_port();
    core.reply_with_file(id, move || ticket.poll(), move |_, result| match result {
        Ok(Ok(saved_port)) => {
            let value = match status {
                Some(mut status) => { status["saved_port"] = json!(saved_port); status },
                None => json!({"saved_port":saved_port,"applies_from":"next_start","restart_required":running_port != saved_port}),
            };
            JsonRpcResponse::success(answer_id, value)
        }
        Ok(Err(error)) => JsonRpcResponse::error_with_data(
            answer_id, -32603, format!("failed to persist webhook port: {error}"),
            json!({"saved":false,"applied":false,"outcome_unknown":false}),
        ),
        Err(error) => transport_error(answer_id, error),
    })
}

fn transport_error(id: Value, error: command::Failure) -> JsonRpcResponse {
    JsonRpcResponse::error_with_data(
        id,
        -32603,
        error.message,
        json!({"saved":if error.unknown {Value::Null} else {json!(false)},"applied":false,"outcome_unknown":error.unknown}),
    )
}

#[cfg(test)]
mod tests;
