use super::super::file_writer::command;
use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;
use tasty_remote_profiles::{MutationFailure, MutationReason};

pub(crate) enum Failure {
    Mutation(MutationFailure),
    Transport(command::Failure),
    Status {
        source: MutationFailure,
        secret_written: bool,
        passkeys_saved: bool,
    },
    Invalid(String),
    Read(String),
    Missing(String),
    Cleanup {
        message: String,
        removed: bool,
        secret_written: bool,
        profile_saved: bool,
    },
}
impl Failure {
    pub(crate) fn partial(&self) -> bool {
        match self {
            Self::Mutation(error) => error.secret_written.is_some() || error.passkeys_saved,
            Self::Cleanup { .. } | Self::Status { .. } => true,
            _ => false,
        }
    }
    pub(crate) fn unknown(&self) -> bool {
        matches!(self, Self::Transport(error) if error.unknown)
    }
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Mutation(error) => error.to_string(),
            Self::Transport(error) => error.message.clone(),
            Self::Status { source, .. } => {
                format!("remote profile committed; status update failed: {source}")
            }
            Self::Invalid(error) | Self::Read(error) | Self::Missing(error) => error.clone(),
            Self::Cleanup { message, .. } => {
                format!("passkey list committed; managed file cleanup failed: {message}")
            }
        }
    }
    pub(crate) fn response(&self, id: Value) -> JsonRpcResponse {
        let code = match self {
            Self::Invalid(_) => -32602,
            Self::Missing(_) => -32040,
            Self::Mutation(error) | Self::Status { source: error, .. } => match error.reason {
                MutationReason::AlreadyExists => -32041,
                MutationReason::NotFound => -32040,
                _ => -32603,
            },
            _ => -32603,
        };
        let mut data = json!({
            "saved":if self.unknown() {Value::Null} else {json!(false)},
            "partial":if self.unknown() {Value::Null} else {json!(self.partial())},
            "outcome_unknown":self.unknown(),
        });
        match self {
            Self::Status {
                source,
                secret_written,
                passkeys_saved,
            } => {
                data["profile_saved"] = json!(true);
                data["secret_written"] = json!(secret_written);
                data["passkeys_saved"] = json!(passkeys_saved);
                data["stage"] = json!("DetectionStatus");
                data["failure_stage"] = json!(format!("{:?}", source.stage));
            }
            Self::Mutation(error) => {
                data["secret_written"] = json!(error.secret_written.is_some());
                data["passkeys_saved"] = json!(error.passkeys_saved);
                data["stage"] = json!(format!("{:?}", error.stage));
            }
            Self::Cleanup {
                removed,
                secret_written,
                profile_saved,
                ..
            } => {
                data["passkeys_saved"] = json!(true);
                data["profile_saved"] = json!(profile_saved);
                data["secret_written"] = json!(secret_written);
                data["removed"] = json!(removed);
                data["cleanup_failed"] = json!(true);
            }
            _ => {}
        }
        JsonRpcResponse::error_with_data(id, code, self.message(), data)
    }
}
