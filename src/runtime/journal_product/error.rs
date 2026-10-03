/// Admission failures keep their category across the worker boundary and public reply.
#[derive(Debug, Clone)]
pub(crate) enum JournalError {
    QueueFull(&'static str),
    KeyConflict,
    StorageBudget {
        used: u64,
        reserved: u64,
        requested: u64,
        ceiling: u64,
    },
    Halted(String),
    Internal(String),
}
impl JournalError {
    pub(crate) fn ipc_code(&self) -> i32 {
        match self {
            Self::QueueFull(_) | Self::StorageBudget { .. } => {
                tasty_ipc::protocol::ERR_COMMAND_QUEUE_FULL
            }
            Self::KeyConflict => tasty_ipc::protocol::ERR_IDEMPOTENCY_KEY_CONFLICT,
            Self::Halted(_) | Self::Internal(_) => -32603,
        }
    }
}
impl From<String> for JournalError {
    fn from(reason: String) -> Self {
        Self::Internal(reason)
    }
}
impl From<&str> for JournalError {
    fn from(reason: &str) -> Self {
        Self::Internal(reason.into())
    }
}
impl From<tasty_event_store::StoreError> for JournalError {
    fn from(error: tasty_event_store::StoreError) -> Self {
        match error {
            tasty_event_store::StoreError::AdmissionCapacity {
                used,
                reserved,
                requested,
                ceiling,
            } => Self::StorageBudget {
                used,
                reserved,
                requested,
                ceiling,
            },
            error => Self::Internal(error.to_string()),
        }
    }
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::QueueFull(message) => f.write_str(message),
            Self::KeyConflict => f.write_str("idempotency key belongs to a different request"),
            Self::StorageBudget {
                used,
                reserved,
                requested,
                ceiling,
            } => write!(
                f,
                "journal admission capacity: used={used}, reserved={reserved}, requested={requested}, ceiling={ceiling}"
            ),
            Self::Halted(message) | Self::Internal(message) => f.write_str(message),
        }
    }
}
impl std::error::Error for JournalError {}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_category_is_independent_of_diagnostic_wording() {
        let budget = JournalError::from(tasty_event_store::StoreError::AdmissionCapacity {
            used: 4,
            reserved: 0,
            requested: 2,
            ceiling: 5,
        });
        assert!(matches!(budget, JournalError::StorageBudget { .. }));
        assert_eq!(
            budget.ipc_code(),
            tasty_ipc::protocol::ERR_COMMAND_QUEUE_FULL
        );
        assert_eq!(
            JournalError::QueueFull("another diagnostic").ipc_code(),
            budget.ipc_code()
        );
        assert_eq!(
            JournalError::KeyConflict.ipc_code(),
            tasty_ipc::protocol::ERR_IDEMPOTENCY_KEY_CONFLICT
        );
        assert_eq!(
            JournalError::Internal("idempotency key capacity exhausted".into()).ipc_code(),
            -32603
        );
    }
}
