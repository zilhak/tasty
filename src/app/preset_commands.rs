//! A private worker store and an admission-ordered receipt collector for the visible cache.
#[cfg(test)]
pub(crate) mod tests;
use super::file_writer::{FileExecutor, command};
use crate::intent::preset::{PresetMutationError, SaveOutcome};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use tasty_presets::{OwnedPreset, PresetChange, PresetKind, PresetStore};
use tasty_utils::batch_writer::Handle;

pub struct Ticket<T>(Arc<Mutex<Option<Result<T, PresetMutationError>>>>);
impl<T> Clone for Ticket<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T> Ticket<T> {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(None)))
    }
    pub fn poll(&self) -> Option<Result<T, PresetMutationError>> {
        self.0.lock().unwrap().take()
    }
    fn finish(&self, result: Result<T, PresetMutationError>) {
        *self.0.lock().unwrap() = Some(result);
    }
}
pub(crate) struct Outcome<T> {
    pub(crate) changes: Vec<PresetChange>,
    pub(crate) result: Result<T, PresetMutationError>,
    pub(crate) saved: bool,
    pub(crate) partial: bool,
    pub(crate) completed_edits: usize,
}
impl<T> Outcome<T> {
    pub(crate) fn new(changes: Vec<PresetChange>, result: Result<T, PresetMutationError>) -> Self {
        let saved = result.is_ok();
        let partial = matches!(
            &result,
            Err(PresetMutationError::Store(
                tasty_presets::PresetError::RenamePartial { .. }
            ))
        );
        Self {
            changes,
            result,
            saved,
            partial,
            completed_edits: 0,
        }
    }
}
fn preset_bytes(preset: &OwnedPreset) -> usize {
    match preset {
        OwnedPreset::Workspace(value) => command::value_bytes(value),
        OwnedPreset::Tab(value) => command::value_bytes(value),
        OwnedPreset::Pane(value) => command::value_bytes(value),
    }
}
fn output_bytes<T: serde::Serialize>(output: &Outcome<T>) -> usize {
    let bytes = match &output.result {
        Ok(value) => command::value_bytes(value),
        Err(error) => error.to_string().len().saturating_add(128),
    };
    output.changes.iter().fold(
        bytes.saturating_add(std::mem::size_of::<Outcome<T>>()),
        |bytes, change| {
            bytes.saturating_add(match change {
                PresetChange::Saved(value) => preset_bytes(value),
                PresetChange::Removed { name, .. } => name.len().saturating_add(64),
                PresetChange::Renamed { from, to, .. } => {
                    from.len().saturating_add(to.len()).saturating_add(128)
                }
            })
        },
    )
}
trait Pending: Send {
    fn collect(&mut self, cache: &mut PresetStore) -> Option<bool>;
}
struct Accepted<T> {
    raw: command::Ticket<Outcome<T>>,
    reply: Ticket<T>,
}
impl<T: Send + 'static> Pending for Accepted<T> {
    fn collect(&mut self, cache: &mut PresetStore) -> Option<bool> {
        let result = self.raw.poll()?;
        let (result, unknown) = match result {
            Ok(output) => {
                let saved = output.saved;
                let partial = output.partial;
                let completed_edits = output.completed_edits;
                let mut cache_error = None;
                for change in output.changes {
                    if let Err(error) = cache.apply_change(change) {
                        tracing::error!(%error, "committed preset cache change failed");
                        cache_error = Some(PresetMutationError::Cache {
                            message: error.to_string(),
                            saved,
                            partial,
                            completed_edits,
                        });
                        break;
                    }
                }
                (cache_error.map_or(output.result, Err), false)
            }
            Err(error) => (
                Err(PresetMutationError::Transport {
                    message: error.message,
                    unknown: error.unknown,
                }),
                error.unknown,
            ),
        };
        self.reply.finish(result);
        Some(unknown)
    }
}
#[derive(Default, Clone, Copy, serde::Serialize)]
pub(crate) struct Status {
    pub(crate) accepted: u64,
    pub(crate) rejected: u64,
    /// A returned command output, including a known business error; not a successful file count.
    pub(crate) completed: u64,
    pub(crate) unknown: u64,
    pub(crate) pending: usize,
}
#[derive(Default)]
struct State {
    pending: VecDeque<Box<dyn Pending>>,
    status: Status,
}
pub(crate) struct PresetCommands {
    worker: Arc<Mutex<PresetStore>>,
    cache: Arc<Mutex<PresetStore>>,
    queue: Option<Handle<FileExecutor>>,
    state: Mutex<State>,
}
impl PresetCommands {
    pub(crate) fn new(cache: Arc<Mutex<PresetStore>>, queue: Option<Handle<FileExecutor>>) -> Self {
        let worker = crate::poison::recover_mutex(
            cache.lock(),
            crate::core::PRESET_STORE_WHAT,
            &crate::core::PRESET_STORE_POISONED,
        )
        .clone();
        Self {
            worker: Arc::new(Mutex::new(worker)),
            cache,
            queue,
            state: Mutex::new(State::default()),
        }
    }
    pub(crate) fn submit<T: serde::Serialize + Send + 'static>(
        &self,
        bytes: usize,
        work: impl FnOnce(&mut PresetStore) -> Outcome<T> + Send + 'static,
    ) -> Ticket<T> {
        let reply = Ticket::new();
        let worker = self.worker.clone();
        let mut state = self.state.lock().unwrap();
        let work = move || match worker.lock() {
            Ok(mut store) => work(&mut store),
            Err(_) => Outcome::new(
                Vec::new(),
                Err(PresetMutationError::Transport {
                    message: "preset worker state poisoned; command not executed".into(),
                    unknown: false,
                }),
            ),
        };
        let raw = match self.queue.as_ref() {
            Some(queue) => {
                command::submit_classified(Some(queue), bytes, work, output_bytes, |output| {
                    command::CommandDisposition {
                        outcome: match &output.result {
                            Err(PresetMutationError::Transport { unknown: true, .. }) => {
                                command::CommandOutcome::Unknown
                            }
                            _ if output.saved => command::CommandOutcome::KnownSuccess,
                            _ => command::CommandOutcome::KnownFailure,
                        },
                        partial: output.partial,
                    }
                })
            }
            // No actual writer exists to measure here. This preserves the shipping refusal and
            // the legacy test-fixture adapter without introducing a runtime disk fallback.
            None => command::submit(None, bytes, work, output_bytes),
        };
        match raw {
            Ok(raw) => {
                state.status.accepted = state.status.accepted.saturating_add(1);
                state.pending.push_back(Box::new(Accepted {
                    raw,
                    reply: reply.clone(),
                }));
            }
            Err(error) => {
                state.status.rejected = state.status.rejected.saturating_add(1);
                reply.finish(Err(PresetMutationError::Transport {
                    message: error.message,
                    unknown: error.unknown,
                }));
            }
        }
        drop(state);
        self.poll();
        reply
    }
    pub(crate) fn poll(&self) {
        let mut state = self.state.lock().unwrap();
        if state.pending.is_empty() {
            return;
        }
        let mut cache = crate::poison::recover_mutex(
            self.cache.lock(),
            crate::core::PRESET_STORE_WHAT,
            &crate::core::PRESET_STORE_POISONED,
        );
        while let Some(first) = state.pending.front_mut() {
            let Some(unknown) = first.collect(&mut cache) else {
                break;
            };
            state.pending.pop_front();
            state.status.completed = state.status.completed.saturating_add(1);
            state.status.unknown = state.status.unknown.saturating_add(u64::from(unknown));
        }
    }
    pub(crate) fn status(&self) -> Status {
        let state = self.state.lock().unwrap();
        Status {
            pending: state.pending.len(),
            ..state.status
        }
    }
    pub(crate) fn save(
        &self,
        base: String,
        explicit: Option<String>,
        overwrite: bool,
        preset: OwnedPreset,
    ) -> Ticket<SaveOutcome> {
        let bytes = preset_bytes(&preset)
            .saturating_add(base.len())
            .saturating_add(explicit.as_ref().map_or(0, String::len))
            .saturating_add(256);
        self.submit(bytes, move |store| {
            match store.save_command(&base, explicit.as_deref(), overwrite, preset) {
                Ok(Some(change)) => {
                    let PresetChange::Saved(value) = &change else {
                        unreachable!("save returns a saved value")
                    };
                    let name = value.name().to_owned();
                    Outcome::new(vec![change], Ok(SaveOutcome::Saved(name)))
                }
                Ok(None) => {
                    let mut output = Outcome::new(Vec::new(), Ok(SaveOutcome::SkippedExists));
                    output.saved = false;
                    output
                }
                Err(error) => Outcome::new(Vec::new(), Err(PresetMutationError::Store(error))),
            }
        })
    }
    pub(crate) fn delete(&self, kind: PresetKind, name: String) -> Ticket<()> {
        self.submit(name.len().saturating_add(128), move |store| {
            let output = store.delete_command(kind, &name);
            Outcome::new(
                output.changes,
                output.result.map_err(PresetMutationError::Store),
            )
        })
    }
    pub(crate) fn rename(&self, kind: PresetKind, from: String, to: String) -> Ticket<()> {
        self.submit(
            from.len().saturating_add(to.len()).saturating_add(128),
            move |store| {
                let output = store.rename_command(kind, &from, &to);
                Outcome::new(
                    output.changes,
                    output.result.map_err(PresetMutationError::Store),
                )
            },
        )
    }
}
