//! Retry only nonblocking namespace contention on the already charged profile worker.
use super::*;
pub(super) fn run<T>(
    shared: &Shared,
    deadline: Instant,
    phase: &'static str,
    mut operation: impl FnMut() -> Result<T, ProfileError>,
) -> Result<T, PrepareError> {
    loop {
        match operation() {
            Err(ProfileError::Busy) => {
                tracing::debug!(phase, "private profile namespace admission contended");
                wait(shared, deadline)?;
            }
            Err(error) => {
                tracing::warn!(phase, %error, "private profile namespace admission refused");
                return Err(PrepareError::Storage(error));
            }
            Ok(value) => return Ok(value),
        }
    }
}
fn wait(shared: &Shared, deadline: Instant) -> Result<(), PrepareError> {
    // operation() has returned: its filesystem descriptors/namespace lock are gone.
    // Evaluate the external current predicate before taking our memory-state mutex.
    if !shared.active() {
        return Err(PrepareError::Retired);
    }
    let state = shared.state.lock().map_err(|_| poisoned())?;
    if shared.cancelled.load(Ordering::Acquire) {
        return Err(PrepareError::Retired);
    }
    if Instant::now() >= deadline {
        return Err(PrepareError::AdmissionTimeout);
    }
    let delay = deadline
        .saturating_duration_since(Instant::now())
        .min(Duration::from_millis(50));
    let (state, _) = shared
        .ready
        .wait_timeout(state, delay)
        .map_err(|_| poisoned())?;
    drop(state);
    // Drop notifies the condvar; current-only retirement is observed within one wait slice.
    if !shared.active() {
        return Err(PrepareError::Retired);
    }
    if Instant::now() >= deadline {
        return Err(PrepareError::AdmissionTimeout);
    }
    Ok(())
}
fn poisoned() -> PrepareError {
    tracing::error!("private profile namespace admission state poisoned; ownership retained");
    PrepareError::Worker
}
