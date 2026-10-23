//! Read-only shutdown observations; request receipts stay with their consumers.
use super::AppServices;

impl AppServices {
    /// Final flush/join observation only; None means no configured writer.
    pub(crate) fn memory_writer_shutdown_observation(
        &self,
    ) -> Option<tasty_utils::batch_writer::ShutdownObservation> {
        self.memory_writer
            .as_ref()
            .map(|parts| parts.writer.shutdown_observation())
    }

    /// Final flush/join observation only; None means no configured writer.
    pub(crate) fn file_writer_shutdown_observation(
        &self,
    ) -> Option<tasty_utils::batch_writer::ShutdownObservation> {
        self.file_writer
            .as_ref()
            .map(|parts| parts.writer.shutdown_observation())
    }

    /// Final flush/join observation only; None means no configured GUI writer.
    #[cfg(feature = "gui")]
    pub(crate) fn state_writer_shutdown_observation(
        &self,
    ) -> Option<tasty_utils::batch_writer::ShutdownObservation> {
        self.state_writer
            .as_ref()
            .map(|parts| parts.writer.shutdown_observation())
    }
}
