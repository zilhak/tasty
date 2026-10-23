//! Observer producer retirement while the GUI keeps rendering shutdown frames.
use super::{App, shutdown_trace};
use std::time::Instant;

impl App {
    pub(super) fn wait_for_observer_shutdown(&mut self) -> bool {
        let progress = self.shutdown_poll_observer_sinks();
        self.services.poll_memory_writer();
        self.services.poll_file_writer();
        self.finish_plugin_files();
        self.settle_settings_saves();
        self.services
            .poll_memory_replies(self.plugin_manager.as_mut());
        let state = self.state.shutdown.as_mut().expect("shutdown phase");
        if state.observer_started.is_none() && progress.pending != 0 {
            tracing::info!(target: "tasty::shutdown", pending = progress.pending, "observer_sink_wait_started");
        }
        let started = *state.observer_started.get_or_insert_with(Instant::now);
        if state.observer_drain.keep_waiting(
            progress.pending,
            Instant::now(),
            "observer shutdown timed out; sink workers remain unjoined",
        ) {
            return true;
        }
        if !std::mem::replace(&mut state.observer_reported, true) {
            shutdown_trace::observer_outcome(progress, started.elapsed());
        }
        self.shutdown_fence_observer_submissions();
        false
    }
}
