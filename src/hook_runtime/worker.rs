//! 발화한 훅과 수동 dispatch의 IpcSequence를 접수 순서대로 실행하는 전용 worker.
//! worker 스레드와 대기열은 이 모듈의 private 자원이다. webhook은 요청별 스레드에서
//! `execute_sequence`를 직접 실행하므로 이 대기열을 쓰지 않는다.

use std::sync::OnceLock;
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};

use crate::hook_handler::{IpcCall, SequenceOrigin, SubstitutionContext, execute_sequence};
use tasty_ipc::host_call::HostIpcInjector;

/// 실행 중인 한 건 외에 대기할 시퀀스 수. host 명령 큐와 같은 수를 쓰지만 단위는 시퀀스다.
const PENDING_SEQUENCE_LIMIT: usize = tasty_ipc::admission::INJECTED_DEPTH_LIMIT;

struct SequenceJob {
    origin: SequenceOrigin,
    handler_id: String,
    injector: HostIpcInjector,
    calls: Vec<IpcCall>,
    ctx: SubstitutionContext,
}

/// 최초 스레드 생성 실패도 OnceLock에 남겨 이후 자동 재시도하지 않는다.
fn sequence_worker() -> Option<&'static SyncSender<SequenceJob>> {
    static WORKER: OnceLock<Option<SyncSender<SequenceJob>>> = OnceLock::new();
    WORKER
        .get_or_init(|| {
            let (tx, rx) = sync_channel::<SequenceJob>(PENDING_SEQUENCE_LIMIT);
            let spawned = std::thread::Builder::new()
                .name("hook-sequence".into())
                .spawn(move || {
                    for job in rx {
                        tracing::debug!("{} IpcSequence '{}' running", job.origin, job.handler_id);
                        execute_sequence(job.origin, &job.injector, &job.calls, &job.ctx);
                    }
                });
            match spawned {
                Ok(_) => Some(tx),
                Err(e) => {
                    tracing::error!("hook IpcSequence worker thread spawn failed: {e}");
                    None
                }
            }
        })
        .as_ref()
}

/// 명령 큐를 처리하는 스레드에서 응답을 기다리지 않도록 전용 실행기에 넘긴다.
/// 수락 순서로 시퀀스를 처리하며 가득 찼거나 worker가 없으면 실행 전에 오류를 반환한다.
/// 개별 스텝의 대기 종료 뒤 실제 명령은 여전히 실행 중일 수 있다.
pub fn enqueue_sequence(
    origin: SequenceOrigin,
    handler_id: &str,
    injector: &HostIpcInjector,
    calls: &[IpcCall],
    ctx: SubstitutionContext,
) -> Result<(), SequenceNotQueued> {
    let worker = sequence_worker().ok_or(SequenceNotQueued::WorkerUnavailable)?;
    let job = SequenceJob {
        origin,
        handler_id: handler_id.to_string(),
        injector: injector.clone(),
        calls: calls.to_vec(),
        ctx,
    };
    worker.try_send(job).map_err(|e| match e {
        TrySendError::Full(_) => SequenceNotQueued::Full,
        TrySendError::Disconnected(_) => SequenceNotQueued::WorkerStopped,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceNotQueued {
    Full,
    WorkerUnavailable,
    WorkerStopped,
}

impl std::fmt::Display for SequenceNotQueued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Full => write!(f, "{PENDING_SEQUENCE_LIMIT} sequences are already waiting"),
            Self::WorkerUnavailable => f.write_str("the sequence worker thread is unavailable"),
            Self::WorkerStopped => f.write_str("the sequence worker thread has stopped"),
        }
    }
}
