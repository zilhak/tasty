//! Nonblocking application handle for the data-home journal worker.
//!
//! SQLite and canonical models stay on the worker. A committed batch must be acknowledged by
//! the application projection before a successful response or another mutation is released.

mod binding;
mod decider;
mod identity;
mod preparation;
mod response;
pub(crate) mod view_record;
mod worker;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;

use tasty_core::{IdKind, JournalModel, StreamBatch, StructureModels};
use tasty_event_store::{CommandKey, CommandRecord, IdRange};

use super::command_executor::Executed;
pub(crate) use binding::{BoundEngine, EngineBinding, EngineSelection};
pub(crate) use decider::StreamCommand;
pub(crate) use preparation::{AdoptRecipe, ChildRecipe,ClaimedPreparation,ClaimedRetirement,EffectLease, PreparationInput, ShellRecipe};
pub(crate) use response::{CompletionView,ResponsePlan, ResponseProgress};

const QUEUE_CAPACITY: usize = 64;
const MAX_REQUEST_BYTES: usize =
    crate::adapters::production::tcp_ipc_server::MAX_REQUEST_LINE_BYTES;
const MAX_QUEUED_BYTES: usize = tasty_ipc::admission::QUEUED_BYTES_LIMIT;

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
    ReconcilePreparation {lease:EffectLease,evidence:Vec<u8>,discarded:Option<String>,view:CompletionView},
    ReconcileRetirement {lease:EffectLease,evidence:Vec<u8>},
    ClaimForward {stream:String,operation:tasty_core::OperationId},
    ForwardFinished {lease:EffectLease,outcome:tasty_core::OperationOutcome},
    PrepareSubtree {binding:EngineBinding,draft:crate::runtime::preset_plan::AssemblyDraft},
    PrepareUndo {binding:EngineBinding,target_pane:Option<u32>,scope:Option<u32>,shell:ShellRecipe},
    ReserveExecutionIds {binding:EngineBinding,kinds:Vec<(IdKind,u32)>},
    CaptureClosed {view:CompletionView,binding:EngineBinding,target:tasty_core::CloseTarget,display_name:Option<String>,surfaces:Vec<crate::runtime::surface_capture::CapturedSurface>},
    Capture {binding:EngineBinding,surfaces:Vec<crate::runtime::surface_capture::CapturedSurface>},
    CapturePreset {draft:crate::intent::preset_capture::PresetCaptureDraft},
    RetireEngine(EngineBinding),
    OpenEngine {
        selection: EngineSelection,
        normal_category_name: String,
        surface_floor: u32,
    },
    /// Current permissions are checked by App before this lookup. Targets are not resolved yet.
    Admit(Admission),
    Resolve {
        changes: Vec<StreamCommand>,
        response: Option<ResponsePlan>,
    },
    Reserve(Vec<(IdKind, u32)>),
    #[cfg(feature = "gui")]
    SaveView(view_record::StoredView),
    ReadEngine(String),
    ReadCommand(String),
    ReadPayload(tasty_core::DataRef),
    PutPreparation(PreparationInput),
    PutPayload(Vec<u8>),
    ClaimRetirement {stream:String,operation:tasty_core::OperationId},
    RetirementFinished {lease:EffectLease,outcome:tasty_core::OperationOutcome},
    ClaimPreparation {
        stream: String,
        operation: tasty_core::OperationId,
    },
    Prepared {
        lease: EffectLease,
        result: tasty_core::PreparationResult,
    },
    InstallationRejected {
        lease: EffectLease,
        reason: String,
    },
    CleanupFinished {
        lease: EffectLease,
        view:CompletionView,
    },
    PreparationUncertain {lease:EffectLease,reason:String},
    CancelAdmission,
}

#[derive(Debug)]
pub(crate) struct Request {
    pub(crate) ticket: u64,
    pub(crate) work: Work,
}

#[derive(Debug, Clone)]
pub(crate) enum ResultValue {
    CapturedPreset {preset:crate::intent::ClonedPreset,base_name:String},
    RecoveryRequired {command_id:String,reason:String,replay:bool},
    ForwardClaimed {lease:EffectLease,payload:Vec<u8>},
    AssemblyResolved {stream:String,input:Option<tasty_core::DataRef>,plan:Option<tasty_core::CreationAssembly>},
    #[cfg(feature = "gui")]
    ViewSaved,
    Bound(BoundEngine),
    NeedsResolution,
    JoinedAdmission {
        leader_ticket: u64,
    },
    Stored(CommandRecord),
    Command(CommandRecord),
    Executed(Executed),
    Reserved(Vec<IdRange>),
    ExecutionIds {binding:EngineBinding,ranges:Vec<IdRange>},
    Engine(JournalModel),
    Payload {
        reference: tasty_core::DataRef,
        bytes: Vec<u8>,
    },
    InputStored(tasty_core::DataRef),
    ClosedCaptured {input:tasty_core::DataRef,undo:Option<tasty_core::UndoCapture>},
    Claimed(ClaimedPreparation),
    RetirementClaimed(ClaimedRetirement),
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
    /// Canonical/publication cut is no longer available. Every App reader and writer must halt.
    Halted(String),
    /// All affected local engines must apply this as one publication before acknowledging.
    Publish {
        /// Explicit bootstrap destination, allocated on the worker before this batch.
        engine_binding: Option<EngineBinding>,
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

struct QueuedRequest {
    request: Request,
    _bytes: QueuedBytes,
}

struct QueuedBytes {
    count: usize,
    held: Arc<AtomicUsize>,
}

impl Drop for QueuedBytes {
    fn drop(&mut self) {
        self.held.fetch_sub(self.count, Ordering::AcqRel);
    }
}

pub(crate) struct JournalWorker {
    readers: Arc<super::journal_payload::PayloadReaders>,
    requests: Option<mpsc::SyncSender<QueuedRequest>>,
    queued_bytes: Arc<AtomicUsize>,
    completions: Option<mpsc::Receiver<Completion>>,
    acknowledgements: Option<mpsc::SyncSender<(u64, Result<(), String>)>>,
    closed: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    #[cfg(test)]
    pub(crate) fail_next_publication: Arc<AtomicBool>,
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
        let readers=Arc::new(super::journal_payload::PayloadReaders::default());
        let worker_readers=readers.clone();
        #[cfg(test)]
        let fail_next_publication = Arc::new(AtomicBool::new(false));
        #[cfg(test)]
        let publication_fault = fail_next_publication.clone();
        let thread = std::thread::Builder::new()
            .name("structure-journal".into())
            .spawn(move || {
                worker::run(
                    home,
                    worker_readers,
                    incoming,
                    outgoing,
                    acks,
                    stopped,
                    wake,
                    #[cfg(test)]
                    publication_fault,
                );
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            readers,
            requests: Some(requests),
            queued_bytes: Arc::new(AtomicUsize::new(0)),
            completions: Some(completions),
            acknowledgements: Some(acknowledgements),
            closed,
            thread: Some(thread),
            #[cfg(test)]
            fail_next_publication,
        })
    }

    /// Retain references synchronously with draft freezing, before another projection can ACK.
    /// The storage worker holds the same readers lock while transferring snapshot pins and GC.
    pub(crate) fn retain_payloads(&self,refs:Vec<tasty_core::DataRef>)->Result<super::journal_payload::PayloadReadLease,String> {
        if self.closed.load(Ordering::Acquire) {return Err("structure journal stopped".into());}
        self.readers.lease(refs)
    }

    pub(crate) fn submit(&self, request: Request) -> Result<(), SubmitError> {
        self.submit_owned(request).map_err(|(error, _)| error)
    }

    pub(crate) fn submit_owned(&self, request: Request) -> Result<(), (SubmitError, Request)> {
        if self.closed.load(Ordering::Acquire) {
            return Err((SubmitError::Stopped, request));
        }
        let bytes = request_size(&request.work);
        if bytes > MAX_QUEUED_BYTES || request_payload_too_large(&request.work) {
            return Err((SubmitError::TooLarge, request));
        }
        let Some(requests) = self.requests.as_ref() else {
            return Err((SubmitError::Stopped, request));
        };
        if self
            .queued_bytes
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |held| {
                held.checked_add(bytes)
                    .filter(|next| *next <= MAX_QUEUED_BYTES)
            })
            .is_err()
        {
            return Err((SubmitError::Busy, request));
        }
        let queued = QueuedRequest {
            request,
            _bytes: QueuedBytes {
                count: bytes,
                held: self.queued_bytes.clone(),
            },
        };
        match requests.try_send(queued) {
            Ok(()) => Ok(()),
            Err(mpsc::TrySendError::Full(queued)) => Err((SubmitError::Busy, queued.request)),
            Err(mpsc::TrySendError::Disconnected(queued)) => {
                Err((SubmitError::Stopped, queued.request))
            }
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

// Internal resolution may carry both a fixed decision input and its frozen response. Its total
// bytes share the queue budget; the public request limit applies to the admitted user payload.
fn request_payload_too_large(work: &Work) -> bool {
    match work {
        Work::Admit(header) => header.original_digest.len() > MAX_REQUEST_BYTES,
        Work::PutPreparation(input) => {
            serde_json::to_vec(&input.params).map_or(true, |bytes| bytes.len() > MAX_REQUEST_BYTES)
        }
        _ => false,
    }
}

pub(crate) fn request_size(work: &Work) -> usize {
    match work {
        Work::ReconcilePreparation {lease,evidence,discarded,view}=>serde_json::to_vec(&(lease,view)).map_or(usize::MAX,|bytes|bytes.len()).saturating_add(evidence.capacity()).saturating_add(discarded.as_ref().map_or(0,String::capacity)),
        Work::CapturePreset {draft}=>draft.weight(),
        Work::ReconcileRetirement {lease,evidence}=>serde_json::to_vec(lease).map_or(usize::MAX,|bytes|bytes.len()).saturating_add(evidence.capacity()),
        Work::PrepareSubtree {binding,draft}=>serde_json::to_vec(&(binding,draft)).map_or(usize::MAX,|bytes|bytes.len()),
        Work::PrepareUndo {binding,target_pane,scope,shell}=>serde_json::to_vec(&(binding,target_pane,scope,shell)).map_or(usize::MAX,|bytes|bytes.len()),
        Work::ReserveExecutionIds {binding,kinds}=>binding.stream.len()+binding.journal_id.len()+kinds.capacity()*std::mem::size_of::<(IdKind,u32)>(),
        Work::CaptureClosed {view,binding,surfaces,display_name,..}=>surfaces.iter().fold(serde_json::to_vec(view).map_or(usize::MAX,|bytes|bytes.len()).saturating_add(binding.stream.len())+binding.journal_id.len()+display_name.as_ref().map_or(0,String::len)+96,|sum,surface|sum.saturating_add(surface.weight())),
        Work::Capture {binding,surfaces}=>surfaces.iter().fold(binding.stream.len()+binding.journal_id.len()+96,|sum,surface|sum.saturating_add(surface.weight())),
        Work::RetireEngine(binding) => binding.stream.len() + binding.journal_id.len() + 64,
        #[cfg(feature = "gui")]
        Work::SaveView(view) => serde_json::to_vec(view).map_or(usize::MAX, |bytes| bytes.len()),
        Work::OpenEngine {
            selection,
            normal_category_name,
            surface_floor,
        } => {
            let serialized=serde_json::to_vec(&(selection, normal_category_name, surface_floor)).map_or(usize::MAX, |bytes| bytes.len());
            match selection {EngineSelection::ImportedSlot {source}=>serialized.max(source.weight().saturating_add(normal_category_name.len()).saturating_add(64)),_=>serialized}
        },
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
        Work::Resolve { changes, response } => {
            serde_json::to_vec(&(changes, response)).map_or(usize::MAX, |bytes| bytes.len())
        }
        Work::Reserve(kinds) => kinds
            .len()
            .saturating_mul(std::mem::size_of::<(IdKind, u32)>()),
        Work::ReadEngine(stream) | Work::ReadCommand(stream) => stream.len(),
        Work::ReadPayload(_) => std::mem::size_of::<tasty_core::DataRef>(),
        Work::PutPayload(bytes)=>bytes.len(),
        Work::PutPreparation(input) => {
            serde_json::to_vec(input).map_or(usize::MAX, |bytes| bytes.len())
        }
        Work::ClaimForward {stream,operation}|Work::ClaimPreparation { stream, operation } | Work::ClaimRetirement {stream,operation} => {
            stream.len().saturating_add(operation.0.len())
        }
        Work::Prepared { lease, result } => {
            serde_json::to_vec(&(lease, result)).map_or(usize::MAX, |bytes| bytes.len())
        }
        Work::InstallationRejected { lease, reason } | Work::PreparationUncertain {lease,reason} => {
            serde_json::to_vec(&(lease, reason)).map_or(usize::MAX, |bytes| bytes.len())
        }
        Work::CleanupFinished {lease,view} => serde_json::to_vec(&(lease,view)).map_or(usize::MAX, |bytes| bytes.len()),
        Work::ForwardFinished {lease,outcome}|Work::RetirementFinished {lease,outcome}=>serde_json::to_vec(&(lease,outcome)).map_or(usize::MAX,|bytes|bytes.len()),
        Work::CancelAdmission => 0,
    }
}

#[cfg(test)]
mod tests;
