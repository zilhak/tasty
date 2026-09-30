//! Nonblocking application handle for the data-home journal worker.
//!
//! SQLite and canonical models stay on the worker. A committed batch must be acknowledged by
//! the application projection before a successful response or another mutation is released.

mod decider;
mod identity;
mod preparation;
mod worker;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use tasty_domain::{IdKind, JournalModel, StreamBatch, StructureModels};
use tasty_event_store::{CommandKey, CommandRecord, IdRange};

use super::command_executor::Executed;
pub(crate) use decider::StreamCommand;
pub(crate) use preparation::{ClaimedPreparation, EffectLease, PreparationInput, ShellRecipe};

const QUEUE_CAPACITY: usize = 64;
const MAX_REQUEST_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub(crate) struct Admission {
    pub(crate) key: Option<CommandKey>,
    pub(crate) original_digest: Vec<u8>,
    pub(crate) actor: String,
    pub(crate) origin: String,
    pub(crate) causation_id: Option<String>,
}

#[derive(Debug)]
pub(crate) enum Work {
    /// Current permissions are checked by App before this lookup. Targets are not resolved yet.
    Admit(Admission),
    Resolve(Vec<StreamCommand>),
    Reserve(Vec<(IdKind, u32)>),
    ReadEngine(String),
    PutPreparation(PreparationInput),
    ClaimPreparation {
        stream: String,
        operation: tasty_domain::OperationId,
    },
    Prepared {
        lease: EffectLease,
        result: tasty_domain::PreparationResult,
    },
    CleanupFinished {
        lease: EffectLease,
    },
    CancelAdmission,
}

#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) ticket: u64,
    pub(crate) work: Work,
}

#[derive(Debug, Clone)]
pub(crate) enum ResultValue {
    NeedsResolution,
    JoinedAdmission { leader_ticket: u64 },
    Stored(CommandRecord),
    Executed(Executed),
    Reserved(Vec<IdRange>),
    Engine(JournalModel),
    InputStored(tasty_domain::DataRef),
    Claimed(ClaimedPreparation),
    Cancelled,
}

#[derive(Debug)]
pub(crate) enum Completion {
    Ready {
        journal_id: String,
        runtime_epoch: u64,
        cut: Option<u64>,
        bootstrap: StructureModels,
    },
    StartupFailed(String),
    /// All affected local engines must apply this as one publication before acknowledging.
    Publish {
        batch: StreamBatch,
        /// One-use predecessor for live preflight; App discards it after publication.
        before: std::collections::BTreeMap<String, JournalModel>,
    },
    Finished {
        ticket: u64,
        result: Result<ResultValue, String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SubmitError {
    Busy,
    TooLarge,
    Stopped,
}

pub(crate) struct JournalWorker {
    requests: Option<mpsc::SyncSender<Request>>,
    completions: Option<mpsc::Receiver<Completion>>,
    acknowledgements: Option<mpsc::SyncSender<(u64, Result<(), String>)>>,
    closed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl JournalWorker {
    /// Filesystem work begins on the new thread, including opening and recovering the database.
    pub(crate) fn spawn(home: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> Result<Self, String> {
        let (requests, incoming) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (outgoing, completions) = mpsc::sync_channel(QUEUE_CAPACITY);
        // A separate channel leaves room for the publication barrier even when admission is full.
        let (acknowledgements, acks) = mpsc::sync_channel(1);
        let closed = Arc::new(AtomicBool::new(false));
        let stopped = closed.clone();
        let thread = std::thread::Builder::new()
            .name("structure-journal".into())
            .spawn(move || {
                worker::run(home, incoming, outgoing, acks, stopped, wake);
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            requests: Some(requests),
            completions: Some(completions),
            acknowledgements: Some(acknowledgements),
            closed,
            thread: Some(thread),
        })
    }

    pub(crate) fn submit(&self, request: Request) -> Result<(), SubmitError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(SubmitError::Stopped);
        }
        if request_size(&request.work) > MAX_REQUEST_BYTES {
            return Err(SubmitError::TooLarge);
        }
        match self
            .requests
            .as_ref()
            .ok_or(SubmitError::Stopped)?
            .try_send(request)
        {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(SubmitError::Busy),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(SubmitError::Stopped),
        }
    }

    pub(crate) fn try_recv(&self) -> Result<Completion, mpsc::TryRecvError> {
        self.completions
            .as_ref()
            .ok_or(mpsc::TryRecvError::Disconnected)?
            .try_recv()
    }

    pub(crate) fn acknowledge(
        &self,
        batch: u64,
        applied: Result<(), String>,
    ) -> Result<(), SubmitError> {
        match self
            .acknowledgements
            .as_ref()
            .ok_or(SubmitError::Stopped)?
            .try_send((batch, applied))
        {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(_)) => Err(SubmitError::Busy),
            Err(mpsc::TrySendError::Disconnected(_)) => Err(SubmitError::Stopped),
        }
    }

    /// Disconnecting both outbound paths unblocks a worker waiting for publication or completion.
    pub(crate) fn stop(&mut self) {
        self.closed.store(true, Ordering::Release);
        self.requests.take();
        self.acknowledgements.take();
        self.completions.take();
    }
}

impl Drop for JournalWorker {
    fn drop(&mut self) {
        self.stop();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::error!("structure journal worker panicked during shutdown");
        }
    }
}

fn request_size(work: &Work) -> usize {
    match work {
        Work::Admit(header) => header
            .original_digest
            .len()
            .saturating_add(header.actor.len())
            .saturating_add(header.origin.len())
            .saturating_add(header.causation_id.as_ref().map_or(0, String::len))
            .saturating_add(header.key.as_ref().map_or(0, |key| {
                key.caller_scope
                    .len()
                    .saturating_add(key.idempotency_key.len())
            })),
        Work::Resolve(commands) => {
            serde_json::to_vec(commands).map_or(usize::MAX, |bytes| bytes.len())
        }
        Work::Reserve(kinds) => kinds
            .len()
            .saturating_mul(std::mem::size_of::<(IdKind, u32)>()),
        Work::ReadEngine(stream) => stream.len(),
        Work::PutPreparation(input) => {
            serde_json::to_vec(input).map_or(usize::MAX, |bytes| bytes.len())
        }
        Work::ClaimPreparation { stream, operation } => {
            stream.len().saturating_add(operation.0.len())
        }
        Work::Prepared { lease, result } => {
            serde_json::to_vec(&(lease, result)).map_or(usize::MAX, |bytes| bytes.len())
        }
        Work::CleanupFinished { lease } => {
            serde_json::to_vec(lease).map_or(usize::MAX, |bytes| bytes.len())
        }
        Work::CancelAdmission => 0,
    }
}

#[cfg(test)]
mod tests;
