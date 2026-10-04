//! Bounded read requests and receipts. No store, worker or mutable engine escapes to a View.
use super::journal_product::EngineBinding;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Weak, mpsc};

const MAX_QUEUED: usize = 16;

pub(crate) struct WorkspaceSnapshot {
    pub(crate) id: u32,
    pub(crate) tasks: Vec<tasty_agent::Task>,
    pub(crate) dags: Vec<tasty_agent::DagSummary>,
    pub(crate) runner: (bool, bool),
}
pub(crate) type Snapshot = Vec<WorkspaceSnapshot>;

#[derive(Debug)]
pub(crate) enum ReadError {
    Retired,
    Failed(String),
}
impl From<String> for ReadError {
    fn from(value: String) -> Self {
        Self::Failed(value)
    }
}
impl From<&str> for ReadError {
    fn from(value: &str) -> Self {
        Self::Failed(value.into())
    }
}
impl std::fmt::Display for ReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Retired => f.write_str("DAG query target retired"),
            Self::Failed(error) => f.write_str(error),
        }
    }
}
pub(crate) type ReadResult = Result<Snapshot, ReadError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BindingKey(Option<(String, String, u64, u64)>);
impl BindingKey {
    pub(crate) fn capture(binding: Option<&EngineBinding>) -> Self {
        Self(binding.map(|b| {
            (
                b.journal_id.clone(),
                b.stream.clone(),
                b.incarnation,
                b.runtime_epoch,
            )
        }))
    }
}

#[derive(Default)]
pub(crate) struct DagReadQueue {
    identity: Arc<()>,
    pending: RefCell<VecDeque<PendingRead>>,
}
impl DagReadQueue {
    pub(crate) fn source<'a>(&'a self, binding: Option<&'a EngineBinding>) -> DagSource<'a> {
        DagSource {
            queue: self,
            binding,
        }
    }
    fn enqueue(&self, request: PendingRead) -> Option<PendingRead> {
        let mut pending = self.pending.borrow_mut();
        pending.retain(PendingRead::is_live);
        if pending.len() == MAX_QUEUED {
            Some(request)
        } else {
            pending.push_back(request);
            None
        }
    }
    pub(crate) fn take(&self, binding: Option<&EngineBinding>) -> Option<PendingRead> {
        let mut pending = self.pending.borrow_mut();
        while let Some(request) = pending.pop_front() {
            if request.is_live() && request.binding == BindingKey::capture(binding) {
                return Some(request);
            }
        }
        None
    }
    pub(crate) fn clear(&self) {
        self.pending.borrow_mut().clear();
    }
}

#[derive(Clone, Copy)]
pub(crate) struct DagSource<'a> {
    queue: &'a DagReadQueue,
    binding: Option<&'a EngineBinding>,
}
impl DagSource<'_> {
    pub(crate) fn matches(&self, query: &DagQuery) -> bool {
        query.source.ptr_eq(&Arc::downgrade(&self.queue.identity))
            && query.binding == BindingKey::capture(self.binding)
    }
    pub(crate) fn request(&self, workspaces: Vec<u32>) -> DagQuery {
        let (reply, receiver) = mpsc::sync_channel(1);
        let lifetime = Arc::new(());
        let source = Arc::downgrade(&self.queue.identity);
        let binding = BindingKey::capture(self.binding);
        let pending = self.queue.enqueue(PendingRead {
            workspaces,
            reply,
            lifetime: Arc::downgrade(&lifetime),
            source: source.clone(),
            binding: binding.clone(),
            #[cfg(test)]
            waiting: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });
        DagQuery {
            source,
            binding,
            lifetime: Some(lifetime),
            receiver,
            pending,
            finished: false,
        }
    }
}

pub(crate) struct DagQuery {
    source: Weak<()>,
    binding: BindingKey,
    lifetime: Option<Arc<()>>,
    receiver: mpsc::Receiver<ReadResult>,
    pending: Option<PendingRead>,
    finished: bool,
}
impl DagQuery {
    pub(crate) fn is_pending(&self) -> bool {
        !self.finished
    }
    pub(crate) fn poll(&mut self, source: DagSource<'_>) -> Option<ReadResult> {
        if self.finished {
            return None;
        }
        if !source.matches(self) {
            self.finished = true;
            self.lifetime = None;
            self.pending = None;
            return Some(Err(ReadError::Retired));
        }
        if let Some(request) = self.pending.take() {
            self.pending = source.queue.enqueue(request);
        }
        let result = match self.receiver.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return None,
            Err(mpsc::TryRecvError::Disconnected) => Err("DAG query worker disconnected".into()),
        };
        self.finished = true;
        self.lifetime = None;
        Some(result)
    }
}

pub(crate) struct PendingRead {
    #[cfg(test)]
    pub(crate) waiting: Arc<std::sync::atomic::AtomicBool>,
    pub(crate) workspaces: Vec<u32>,
    reply: mpsc::SyncSender<ReadResult>,
    lifetime: Weak<()>,
    source: Weak<()>,
    binding: BindingKey,
}

#[derive(Clone)]
pub(crate) struct ReadTicket {
    lifetime: Weak<()>,
    source: Weak<()>,
    #[cfg(test)]
    pub(crate) waiting: Arc<std::sync::atomic::AtomicBool>,
}
impl ReadTicket {
    pub(crate) fn is_live(&self) -> bool {
        self.lifetime.strong_count() != 0 && self.source.strong_count() != 0
    }
}
impl PendingRead {
    pub(crate) fn ticket(&self) -> ReadTicket {
        ReadTicket {
            lifetime: self.lifetime.clone(),
            source: self.source.clone(),
            #[cfg(test)]
            waiting: self.waiting.clone(),
        }
    }

    pub(crate) fn is_live(&self) -> bool {
        self.lifetime.strong_count() != 0 && self.source.strong_count() != 0
    }
    pub(crate) fn complete(self, result: ReadResult) {
        if self.is_live() && self.reply.send(result).is_err() {
            tracing::debug!("DAG result discarded after its receipt closed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn binding(incarnation: u64) -> EngineBinding {
        EngineBinding {
            journal_id: "journal".into(),
            stream: "slot".into(),
            incarnation,
            runtime_epoch: 1,
            published_cut: Some(1),
            revision: Some(1),
        }
    }
    #[test]
    fn an_incarnation_change_rejects_the_old_receipt_and_cancels_its_work() {
        let queue = DagReadQueue::default();
        let old = binding(1);
        let current = binding(2);
        let mut query = queue.source(Some(&old)).request(vec![1]);
        let work = queue.take(Some(&old)).unwrap();
        assert!(work.is_live());
        assert!(query.poll(queue.source(Some(&current))).unwrap().is_err());
        assert!(!work.is_live());
        work.complete(Ok(Vec::new()));
        assert!(query.poll(queue.source(Some(&current))).is_none());
    }
    #[test]
    fn a_replaced_engine_queue_cannot_deliver_even_with_the_same_binding() {
        let old = DagReadQueue::default();
        let new = DagReadQueue::default();
        let binding = binding(1);
        let mut query = old.source(Some(&binding)).request(vec![1]);
        let work = old.take(Some(&binding)).unwrap();
        assert!(!new.source(Some(&binding)).matches(&query));
        drop(old);
        assert!(!work.is_live());
        assert!(query.poll(new.source(Some(&binding))).unwrap().is_err());
    }
    #[test]
    fn full_queue_retries_without_losing_the_receipt_and_dropped_queries_free_capacity() {
        let queue = DagReadQueue::default();
        let source = queue.source(None);
        let mut queries: Vec<_> = (0..MAX_QUEUED).map(|_| source.request(vec![1])).collect();
        let mut extra = source.request(vec![2]);
        assert!(extra.pending.is_some());
        assert_eq!(queue.pending.borrow().len(), MAX_QUEUED);
        queries.clear();
        assert!(extra.poll(source).is_none());
        assert!(extra.pending.is_none());
        let work = queue.take(None).unwrap();
        assert_eq!(work.workspaces, vec![2]);
        work.complete(Ok(Vec::new()));
        assert!(extra.poll(source).unwrap().unwrap().is_empty());
        assert!(extra.poll(source).is_none());
    }
}
