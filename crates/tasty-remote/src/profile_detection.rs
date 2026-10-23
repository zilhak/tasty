//! Profile probes retain preparation, SSH workers and saved status receipts independently of Views.
mod jobs;
pub mod persistence;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tasty_remote_profiles::RemoteProfile;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetectionId(pub u64);
pub struct DetectionUpdate {
    pub id: DetectionId,
    pub name: String,
    pub result: Result<String, String>,
}
#[derive(Default)]
struct State {
    jobs: Vec<jobs::Job>,
    next: u64,
    stopping: bool,
}
#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    sink: Option<persistence::Submit>,
}
#[derive(Clone, Default)]
pub struct ProfileDetections(Arc<Inner>);
impl ProfileDetections {
    pub fn with_file_sink(sink: Option<persistence::Submit>) -> Self {
        Self(Arc::new(Inner {
            state: Mutex::new(State::default()),
            sink,
        }))
    }
    pub fn enqueue(&self, name: String) -> Result<DetectionId, String> {
        let root = tasty_utils::path::tasty_home().ok_or("remote profile home is unavailable")?;
        self.prepare(root, name, None)
    }
    /// The saved profile is compared with the ordered fresh input before starting SSH.
    pub fn enqueue_saved_at(
        &self,
        root: PathBuf,
        profile: RemoteProfile,
    ) -> Result<DetectionId, String> {
        self.prepare(root, profile.name.clone(), Some(profile))
    }
    fn prepare(
        &self,
        root: PathBuf,
        name: String,
        expected: Option<RemoteProfile>,
    ) -> Result<DetectionId, String> {
        let mut state = self.0.state.lock().unwrap();
        if state.stopping {
            return Err("remote profile detection is shutting down".into());
        }
        if state.jobs.len() >= 8 {
            return Err("remote profile detection capacity exhausted".into());
        }
        let sink = self
            .0
            .sink
            .as_ref()
            .ok_or("profile file writer is unavailable")?;
        let id = DetectionId(
            state
                .next
                .checked_add(1)
                .ok_or("remote profile detection ID exhausted")?,
        );
        let receipt = sink(persistence::Request::Load {
            root,
            name: name.clone(),
            expected,
        })
        .map_err(|error| error.message)?;
        // Only accepted preparation invalidates an older probe. Earlier status writes already
        // executing finish before this ordered read; later old submissions see cancellation.
        for job in &state.jobs {
            if job.name == name {
                job.cancel.request_cancel();
            }
        }
        state.next = id.0;
        state.jobs.push(jobs::Job::preparing(id, name, receipt));
        Ok(id)
    }
    pub fn poll(&self) -> Vec<DetectionUpdate> {
        self.0.poll()
    }
    pub fn has_pending(&self) -> bool {
        !self.0.state.lock().unwrap().jobs.is_empty()
    }
    pub fn begin_shutdown(&self) {
        self.0.begin_shutdown();
    }
    /// Counts unfinished preparation, SSH workers and already accepted status saves.
    pub fn poll_shutdown(&self) -> usize {
        self.0.begin_shutdown();
        self.0.poll();
        self.0.state.lock().unwrap().jobs.len()
    }
}
impl Inner {
    fn begin_shutdown(&self) {
        let mut state = self.state.lock().unwrap();
        state.stopping = true;
        for job in &state.jobs {
            job.cancel.request_cancel();
        }
    }
    fn poll(&self) -> Vec<DetectionUpdate> {
        let mut state = self.state.lock().unwrap();
        let mut updates = Vec::new();
        let stopping = state.stopping;
        for job in std::mem::take(&mut state.jobs) {
            match job.advance(stopping, self.sink.as_ref()) {
                jobs::Progress::Pending(job) => state.jobs.push(job),
                jobs::Progress::Done(update) => {
                    match &update.result {
                        Ok(mode) => {
                            tracing::info!(profile=%update.name,%mode,"remote profile detection completed")
                        }
                        Err(error) => {
                            tracing::warn!(profile=%update.name,%error,"remote profile detection did not complete")
                        }
                    }
                    updates.push(update);
                }
            }
        }
        updates
    }
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.begin_shutdown();
        self.poll();
        let remaining = self.state.lock().unwrap().jobs.len();
        if remaining != 0 {
            tracing::warn!(
                remaining,
                "remote profile owners still unfinished at owner drop"
            );
        }
    }
}
