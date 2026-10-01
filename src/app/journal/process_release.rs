//! Disposal of exact process owners without synthesizing durable operation outcomes.
use super::*;

impl JournalApplication {
    /// End execution before moving private resources into their final receipt owner. A queued
    /// durable obligation remains unfinished for recovery; stopping is never proof of its outcome.
    #[cfg(not(feature = "gui"))]
    pub(crate) fn begin_process_shutdown(&mut self) {
        self.worker.stop();
        let reason = self
            .halted
            .get_or_insert_with(|| "headless process is shutting down".into())
            .clone();
        self.fail_pending_commands(&reason);
    }

    /// Halted execution is not resumed. Preserve every existing resource/receipt in the process
    /// owner release container, then let exact PTY and plugin observations govern registry removal.
    pub(crate) fn release_halted_resources(
        &mut self,
        session: &mut EngineSession,
        plugins: Option<&mut crate::plugin::PluginManager>,
    ) -> bool {
        if !self.is_halted() {
            return false;
        }
        if let Some(plugins) = plugins {
            plugins.poll_retirement_control();
        }
        let mut release = session.engine_release.take().unwrap_or_default();
        let keys: Vec<_> = self
            .creations
            .keys()
            .filter(|(engine, _)| *engine == session.id)
            .copied()
            .collect();
        for key in keys {
            if let Some(creation) = self.creations.remove(&key) {
                creation.retain_for_release(&mut release);
            }
        }
        for (_, candidate) in session.pending_materializations.drain() {
            candidate.retire_for_release(&mut release);
        }
        for (_, retirement) in session.pending_resource_retirements.drain() {
            retirement.retain_for_release(&mut release);
        }
        self.resource_cleanups
            .retain(|_, cleanup| cleanup.engine != session.id);
        session.engine_release = Some(release);
        true
    }
}
