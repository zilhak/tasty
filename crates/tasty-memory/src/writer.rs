//! memory.db 전담 writer. 호출자는 요청을 넘기고 receipt를 받아 바로 진행하며, worker 한 개가
//! 저장소를 소유하고 여러 요청을 실행 단위 하나(SQLite transaction 하나)로 묶어 commit한다.
//!
//! 접수·수집·예산·재시도·종료 규칙은 `tasty_utils::batch_writer`가 정하고 이 모듈은 요청 종류와
//! 묶음 실행을 정한다.
//!
//! - 묶음 하나는 `run_unit` 하나다. 요청은 접수 순서대로 앞 요청의 반영을 보고 판정한다. 거절된
//!   요청은 그 요청만 되돌리고 다른 요청은 확정한다. 저장 실패는 묶음 전체를 되돌린다.
//! - 쓰기 요청(`Write`)은 단독 연산과 같은 판정을 받는다. 원자 묶음(`Batch`)은 그 묶음만 되돌린다.
//! - 복합 연산(`Unit`)은 읽기-판단-쓰기 전체를 writer 안에서 한 번에 실행한다. 읽기와 쓰기를 따로
//!   접수하지 않는다. 본문은 `'static` 값만 잡으므로 호출 스레드의 빌린 상태나 잠금 guard를 실을 수
//!   없다. busy 재시도나 다른 요청의 panic 뒤 다시 실행될 수 있으므로 저장소 밖의 효과를 내지 않는다.
//! - 강한 읽기(`Read`)는 barrier다. 앞선 요청이 확정된 뒤 혼자 실행하고 transaction을 열지 않는다.
//! - busy는 단위 전체가 되돌려진 뒤라 다시 실행한다. 원인 요청이 있는 저장 실패는 확정하지 않은
//!   것(`Failed`)이고, commit 단계처럼 원인 요청을 특정할 수 없는 저장 실패는 결과 불명(`Unknown`)으로
//!   다룬다. 요청 본문의 panic은 단위를 되돌린 뒤 그 요청만 실패시키고 나머지를 다시 실행한다.
//!   이 재실행은 저장소가 panic한 단위를 되돌린다는 전제이며 `MemoryStore`가 그렇게 동작한다.
//!
//! 계약 전체는 docs/design/systems/storage-contract.md의 "저장 실행기" 절에 있다.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex};

use tasty_utils::batch_writer::{self, Clock, Execution, Executor, Limits};

use crate::{
    BatchFailure, BatchMode, MemoryError, MemoryStorage, MemoryStore, MemoryValue, Result,
    StorageFailure, WriteOp, WriteOutput,
};

/// 복합 연산·강한 읽기가 돌려주는 값.
pub type Value = Box<dyn Any + Send>;
type UnitBody = dyn FnMut(&mut dyn MemoryStorage) -> Result<Value> + Send;
type ReadBody = dyn FnMut(&dyn MemoryStorage) -> Result<Value> + Send;

/// writer에 넣는 요청.
pub enum MemoryRequest {
    /// 쓰기 하나.
    Write(WriteOp),
    /// 쓰기 묶음. 원자 묶음은 거절이 하나라도 있으면 이 묶음만 되돌린다.
    Batch { mode: BatchMode, ops: Vec<WriteOp> },
    /// 읽기-판단-쓰기를 한 번에 실행하는 복합 연산. `bytes`는 예산에 셀 요청·결과 크기다.
    Unit { body: Box<UnitBody>, bytes: usize },
    /// 앞선 요청이 확정된 뒤 혼자 실행하는 읽기. `bytes`는 예산에 셀 결과 크기다.
    Read { body: Box<ReadBody>, bytes: usize },
}

impl MemoryRequest {
    /// 값을 돌려주는 복합 연산. 결과는 [`MemoryOutput::into_value`]로 꺼낸다.
    /// 본문은 호출 스레드의 빌린 값을 잡을 수 없다.
    ///
    /// ```compile_fail,E0597
    /// let local = String::from("x");
    /// let borrowed = &local;
    /// tasty_memory::writer::MemoryRequest::unit(0, move |_s| Ok(borrowed.len()));
    /// ```
    pub fn unit<T: Send + 'static>(
        bytes: usize,
        mut body: impl FnMut(&mut dyn MemoryStorage) -> Result<T> + Send + 'static,
    ) -> Self {
        Self::Unit {
            body: Box::new(move |s| body(s).map(|v| Box::new(v) as Value)),
            bytes,
        }
    }

    /// 값을 돌려주는 강한 읽기.
    pub fn read<T: Send + 'static>(
        bytes: usize,
        mut body: impl FnMut(&dyn MemoryStorage) -> Result<T> + Send + 'static,
    ) -> Self {
        Self::Read {
            body: Box::new(move |s| body(s).map(|v| Box::new(v) as Value)),
            bytes,
        }
    }
}

impl std::fmt::Debug for MemoryRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Write(op) => f.debug_tuple("Write").field(op).finish(),
            Self::Batch { mode, ops } => f
                .debug_struct("Batch")
                .field("mode", mode)
                .field("ops", &ops.len())
                .finish(),
            Self::Unit { bytes, .. } => f.debug_struct("Unit").field("bytes", bytes).finish(),
            Self::Read { bytes, .. } => f.debug_struct("Read").field("bytes", bytes).finish(),
        }
    }
}

/// 직렬화하지 않고 JSON 길이만 센다.
struct ByteCounter(usize);

impl std::io::Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 += buf.len();
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn value_bytes(value: &MemoryValue) -> usize {
    match value {
        MemoryValue::Text(s) => s.len(),
        MemoryValue::Binary(b) => b.len(),
        MemoryValue::Json(v) => {
            let mut counter = ByteCounter(0);
            // 직렬화하지 못하는 값은 저장 단계에서 거절된다. 그때까지는 센 만큼만 잡는다.
            if serde_json::to_writer(&mut counter, v).is_err() {
                tracing::debug!("memory writer could not measure a JSON value");
            }
            counter.0
        }
    }
}

fn op_bytes(op: &WriteOp) -> usize {
    match op {
        WriteOp::Put {
            owner, key, value, ..
        }
        | WriteOp::PutSecret {
            owner, key, value, ..
        } => owner.len() + key.len() + value_bytes(value),
        WriteOp::Delete { owner, key, .. } | WriteOp::DeleteSecret { owner, key, .. } => {
            owner.len() + key.len()
        }
    }
}

impl batch_writer::Request for MemoryRequest {
    fn bytes(&self) -> usize {
        match self {
            Self::Write(op) => op_bytes(op),
            Self::Batch { ops, .. } => ops.iter().map(op_bytes).sum(),
            Self::Unit { bytes, .. } | Self::Read { bytes, .. } => *bytes,
        }
    }

    fn barrier(&self) -> bool {
        matches!(self, Self::Read { .. })
    }
}

/// 요청 하나의 성공 결과.
pub enum MemoryOutput {
    Written(WriteOutput),
    /// 쓰기 묶음의 요청별 결과.
    Batch(Vec<Result<WriteOutput>>),
    /// 복합 연산·강한 읽기의 값과 예산에 세는 크기.
    Value {
        value: Value,
        bytes: usize,
    },
}

impl MemoryOutput {
    /// 복합 연산·강한 읽기의 값을 꺼낸다. 다른 종류이거나 타입이 다르면 None이다.
    pub fn into_value<T: 'static>(self) -> Option<T> {
        match self {
            Self::Value { value, .. } => value.downcast::<T>().ok().map(|v| *v),
            _ => None,
        }
    }
}

impl std::fmt::Debug for MemoryOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Written(o) => f.debug_tuple("Written").field(o).finish(),
            Self::Batch(v) => f.debug_tuple("Batch").field(v).finish(),
            Self::Value { bytes, .. } => f.debug_struct("Value").field("bytes", bytes).finish(),
        }
    }
}

/// 요청이 효과를 내지 못한 이유.
#[derive(Debug, Clone)]
pub enum WriterError {
    /// 요청 자신이 거절됐다(CAS·owner·quota·형식, 본문이 돌려준 오류). 원자 묶음은 거절된 요청의
    /// 위치를 싣는다.
    Rejected {
        error: Arc<MemoryError>,
        index: Option<usize>,
    },
    /// 저장 실패.
    Storage(Arc<MemoryError>),
    /// 요청 본문이 panic했다. 그 요청의 쓰기는 확정되지 않았다.
    Panicked,
}

impl WriterError {
    fn of(error: MemoryError) -> Self {
        if error.storage_failure().is_some() {
            Self::Storage(Arc::new(error))
        } else {
            Self::Rejected {
                error: Arc::new(error),
                index: None,
            }
        }
    }

    /// 원래 저장소 오류. panic이면 None이다.
    pub fn memory_error(&self) -> Option<&MemoryError> {
        match self {
            Self::Rejected { error, .. } | Self::Storage(error) => Some(error),
            Self::Panicked => None,
        }
    }
}

/// 요청 하나의 결과.
pub type MemoryCompletion = batch_writer::Completion<MemoryOutput, WriterError>;

/// worker가 저장소에 닿는 방법. 저장소를 소유하거나(최종 형태) 이행 중에는 공유 잠금을 잡는다.
pub trait StoreAccess: Send + 'static {
    fn with_store<R>(&mut self, f: impl FnOnce(&mut dyn MemoryStorage) -> R) -> R;
}

impl StoreAccess for MemoryStore {
    fn with_store<R>(&mut self, f: impl FnOnce(&mut dyn MemoryStorage) -> R) -> R {
        f(self)
    }
}

/// 다른 소비자가 아직 같은 `Arc<Mutex<dyn MemoryStorage>>`를 직접 잠그는 이행 기간의 호환 경계.
/// worker도 묶음마다 같은 잠금을 잡으므로 직접 잠그는 소비자와 직렬화된다.
pub struct SharedStore(pub Arc<Mutex<dyn MemoryStorage>>);

impl StoreAccess for SharedStore {
    fn with_store<R>(&mut self, f: impl FnOnce(&mut dyn MemoryStorage) -> R) -> R {
        let mut guard = tasty_utils::poison::recover_mutex(
            self.0.lock(),
            crate::STORE_LOCK_WHAT,
            &crate::STORE_LOCK_POISONED,
        );
        f(&mut *guard)
    }
}

/// memory.db 묶음 실행기. 묶음이 끝날 때마다 같은 잠금 안에서 저장소의 변경 버퍼를 `changes`로 옮긴다.
pub struct MemoryExecutor<A: StoreAccess> {
    access: A,
    changes: Arc<ChangeFeed>,
}

impl<A: StoreAccess> MemoryExecutor<A> {
    pub fn new(access: A, changes: Arc<ChangeFeed>) -> Self {
        Self { access, changes }
    }
}

impl<A: StoreAccess> Executor for MemoryExecutor<A> {
    type Request = MemoryRequest;
    type Output = MemoryOutput;
    type Error = WriterError;

    fn execute(&mut self, batch: &mut [MemoryRequest]) -> Execution<MemoryOutput, WriterError> {
        let changes = &self.changes;
        self.access.with_store(|store| {
            let execution = execute_on(store, batch);
            changes.extend(store.take_pending_changes());
            execution
        })
    }

    fn output_bytes(output: &MemoryOutput) -> usize {
        match output {
            MemoryOutput::Written(_) => 0,
            MemoryOutput::Batch(v) => v.len() * std::mem::size_of::<Result<WriteOutput>>(),
            MemoryOutput::Value { bytes, .. } => *bytes,
        }
    }
}

fn is_busy(error: &MemoryError) -> bool {
    error.storage_failure() == Some(StorageFailure::Busy)
}

/// 묶음 하나를 실행한다. 저장소 잠금을 잡은 채 panic이 밖으로 나가지 않게 여기서 받는다.
fn execute_on(
    store: &mut dyn MemoryStorage,
    batch: &mut [MemoryRequest],
) -> Execution<MemoryOutput, WriterError> {
    if let [MemoryRequest::Read { body, bytes }] = batch {
        let bytes = *bytes;
        return match catch_unwind(AssertUnwindSafe(|| body(&*store))) {
            Err(_) => Execution::Poisoned {
                index: 0,
                error: WriterError::Panicked,
            },
            Ok(Ok(value)) => Execution::Committed(vec![Ok(MemoryOutput::Value { value, bytes })]),
            Ok(Err(e)) if is_busy(&e) => Execution::Busy(WriterError::of(e)),
            Ok(Err(e)) if e.storage_failure().is_some() => Execution::Failed(WriterError::of(e)),
            Ok(Err(e)) => Execution::Committed(vec![Err(WriterError::of(e))]),
        };
    }
    let mut results = Vec::with_capacity(batch.len());
    let mut current = None;
    let mut started = false;
    let mut saw_storage_failure = false;
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        store.run_unit(&mut |s| {
            started = true;
            results.clear();
            saw_storage_failure = false;
            for (i, request) in batch.iter_mut().enumerate() {
                current = Some(i);
                let result = run_one(s, request);
                saw_storage_failure |= matches!(result, Err(WriterError::Storage(_)));
                results.push(result);
            }
            current = None;
            Ok(())
        })
    }));
    match outcome {
        Err(_) => match current {
            Some(index) => Execution::Poisoned {
                index,
                error: WriterError::Panicked,
            },
            None => Execution::Failed(WriterError::Panicked),
        },
        Ok(Ok(())) => Execution::Committed(results),
        Ok(Err(e)) if is_busy(&e) => Execution::Busy(WriterError::of(e)),
        // 원인 요청이 저장 실패를 받았거나 본문 전에 실패했다. 단위는 되돌려졌다.
        Ok(Err(e)) if !started || saw_storage_failure => Execution::Failed(WriterError::of(e)),
        // 모든 요청이 저장 실패 없이 끝났는데 단위가 실패했다. commit 단계일 수 있어 결과를 모른다.
        Ok(Err(e)) => Execution::Unknown(WriterError::of(e)),
    }
}

fn run_one(
    s: &mut dyn MemoryStorage,
    request: &mut MemoryRequest,
) -> std::result::Result<MemoryOutput, WriterError> {
    match request {
        MemoryRequest::Write(op) => apply(s, op)
            .map(MemoryOutput::Written)
            .map_err(WriterError::of),
        MemoryRequest::Batch { mode, ops } => match s.write_batch(*mode, ops) {
            Ok(results) => Ok(MemoryOutput::Batch(results)),
            Err(BatchFailure { index, error }) => Err(match WriterError::of(error) {
                WriterError::Rejected { error, .. } => WriterError::Rejected { error, index },
                other => other,
            }),
        },
        MemoryRequest::Unit { body, bytes } => {
            let bytes = *bytes;
            body(s)
                .map(|value| MemoryOutput::Value { value, bytes })
                .map_err(WriterError::of)
        }
        MemoryRequest::Read { body, bytes } => {
            let bytes = *bytes;
            body(&*s)
                .map(|value| MemoryOutput::Value { value, bytes })
                .map_err(WriterError::of)
        }
    }
}

fn apply(s: &mut dyn MemoryStorage, op: &WriteOp) -> Result<WriteOutput> {
    match op {
        WriteOp::Put {
            owner,
            scope,
            key,
            value,
            opts,
        } => s
            .put(owner, scope, key, value, opts)
            .map(WriteOutput::Version),
        WriteOp::Delete {
            owner,
            scope,
            key,
            cas,
        } => s
            .delete(owner, scope, key, *cas)
            .map(|()| WriteOutput::Deleted),
        WriteOp::PutSecret {
            owner,
            scope,
            key,
            value,
            opts,
        } => s
            .put_secret(owner, scope, key, value, opts)
            .map(WriteOutput::Version),
        WriteOp::DeleteSecret {
            owner,
            scope,
            key,
            cas,
        } => s
            .delete_secret(owner, scope, key, *cas)
            .map(|()| WriteOutput::Deleted),
    }
}

mod access;
pub use access::{ChangeFeed, Failure, MemoryAccess, MemoryTicket, settle};

pub type MemoryWriter<A> = batch_writer::Writer<MemoryExecutor<A>>;
pub type MemoryHandle<A> = batch_writer::Handle<MemoryExecutor<A>>;

/// `memory-writer` 스레드를 시작한다. commit된 변경은 `changes`에 쌓인다.
pub fn spawn<A: StoreAccess>(
    access: A,
    changes: Arc<ChangeFeed>,
    limits: Limits,
    clock: Arc<dyn Clock>,
    notify: Arc<dyn Fn() + Send + Sync>,
) -> std::io::Result<MemoryWriter<A>> {
    batch_writer::Writer::spawn(
        "memory-writer",
        MemoryExecutor::new(access, changes),
        limits,
        clock,
        notify,
    )
}

#[cfg(test)]
mod tests;
