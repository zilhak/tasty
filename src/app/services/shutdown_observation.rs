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

    /// Nonconsuming timing alongside the original final outcome.
    pub(crate) fn memory_writer_shutdown_report(
        &self,
    ) -> Option<tasty_utils::batch_writer::ShutdownReport> {
        Some(tasty_utils::batch_writer::ShutdownReport {
            observation: self.memory_writer_shutdown_observation()?,
            timing: self.memory_writer.as_ref()?.writer.shutdown_timing(),
        })
    }

    /// Nonconsuming timing alongside the original final outcome.
    pub(crate) fn file_writer_shutdown_report(
        &self,
    ) -> Option<tasty_utils::batch_writer::ShutdownReport> {
        Some(tasty_utils::batch_writer::ShutdownReport {
            observation: self.file_writer_shutdown_observation()?,
            timing: self.file_writer.as_ref()?.writer.shutdown_timing(),
        })
    }

    /// Nonconsuming timing alongside the original final outcome.
    #[cfg(feature = "gui")]
    pub(crate) fn state_writer_shutdown_report(
        &self,
    ) -> Option<tasty_utils::batch_writer::ShutdownReport> {
        Some(tasty_utils::batch_writer::ShutdownReport {
            observation: self.state_writer_shutdown_observation()?,
            timing: self.state_writer.as_ref()?.writer.shutdown_timing(),
        })
    }
}
