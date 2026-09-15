//! Debug-only completion records, written after dispatch actually returns.

use std::cell::Cell;

use super::{AttachSource, Outcome};

pub(super) struct Observation {
    connector_entries: Cell<u32>,
}

impl Observation {
    pub(super) fn new() -> Self {
        Self {
            connector_entries: Cell::new(0),
        }
    }

    pub(super) fn enter_connector(&self) {
        self.connector_entries.set(self.connector_entries.get() + 1);
    }

    pub(super) fn complete<T, E>(
        &self,
        own_port: Option<u16>,
        port: u16,
        workspace: u32,
        source: AttachSource,
        outcome: &Outcome<T, E>,
    ) {
        let record = serde_json::json!({
            "port": port,
            "workspace": workspace,
            "source": source.label(),
            "connector_entries": self.connector_entries.get(),
            "outcome": match outcome {
                Outcome::RejectedSelf => "rejected_self",
                Outcome::Connected(Ok(_)) => "connected",
                Outcome::Connected(Err(_)) => "connect_failed",
            },
        });
        // A self request is already a warning. Keep its completion visible under the
        // normal test log filter, including when a regression entered the connector.
        if own_port == Some(port) {
            tracing::warn!(target: "tasty::attach_dispatch", "attach_dispatch_completed {record}");
        } else {
            tracing::debug!(target: "tasty::attach_dispatch", "attach_dispatch_completed {record}");
        }
    }
}
