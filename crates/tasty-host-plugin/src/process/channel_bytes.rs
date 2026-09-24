//! 플러그인 채널의 큐별·전체 바이트 사용량을 제한하고 기록한다.
//! 빈 큐에는 상한보다 큰 메시지도 하나 받는다. 그렇지 않으면 그 메시지를 기다리는
//! reader가 비울 항목도 없는 큐에서 멈출 수 있다. 단일 메시지 크기는 제한하지 않는다.
//! 여러 큐의 동시 입장을 한 잠금 안에서 계산해 전체 합계가 어긋나지 않게 한다.
//! 정책은 docs/architecture/ipc-server.md#플러그인-채널의-상한 참고.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// 큐별 바이트 상한. 정상 부하를 측정해 계산한 값은 아니다.
pub const QUEUE_BYTES_LIMIT: usize = 16 * 1024 * 1024;
/// 전체 플러그인 채널의 바이트 상한. 정상 부하를 측정해 계산한 값은 아니다.
pub const TOTAL_BYTES_LIMIT: usize = 64 * 1024 * 1024;

/// 대기 중 큐가 닫혔는지 다시 확인하는 주기. 여유가 생기면 Condvar가 먼저 깨운다.
const WAIT_SLICE: Duration = Duration::from_millis(50);

// poison을 처음 한 번 알리고 기존 집계를 재사용한다. 부분 갱신을 되돌리지는 않는다.
static LEDGER_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const LEDGER_WHAT: &str = "plugin channel byte ledger";

/// ping·shutdown은 다른 플러그인의 포화 때문에 막히지 않도록 전체 상한만 면제한다.
/// 면제한 요청도 실제 바이트 합계에는 넣고 꺼낼 때 뺀다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// 일반 요청·응답·이벤트 — 두 상한을 다 본다.
    Data,
    /// ping · shutdown — 큐 상한만 본다.
    Control,
}

/// 두 상한. 시험이 작은 값을 주입할 수 있도록 상수가 아니라 값으로 든다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct ChannelLimits {
    pub queue_bytes: usize,
    pub total_bytes: usize,
}

impl Default for ChannelLimits {
    fn default() -> Self {
        Self {
            queue_bytes: QUEUE_BYTES_LIMIT,
            total_bytes: TOTAL_BYTES_LIMIT,
        }
    }
}

/// 큐의 방향. 포화의 답이 방향마다 다르다(docs/architecture/ipc-server.md#플러그인-채널의-상한) — 요청은 거절, 응답·이벤트는 대기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// 호스트 → plugin 요청. 포화면 거절한다.
    Request,
    /// plugin → 호스트 응답. 포화면 기다린다.
    Response,
    /// plugin → 호스트 이벤트. 포화면 기다린다.
    Event,
}

/// 바이트 상한이 한 건을 못 들인 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 이 큐의 누적이 [`ChannelLimits::queue_bytes`] 를 넘는다.
    Queue,
    /// 모든 채널의 합계가 [`ChannelLimits::total_bytes`] 를 넘는다.
    Total,
    /// 받는 쪽이 사라졌다 — 채널이 닫혔다.
    Closed,
}

#[derive(Debug)]
struct QueueState {
    plugin_id: String,
    direction: Direction,
    queued: usize,
    messages: usize,
    peak: usize,
}

#[derive(Debug, Default)]
struct State {
    total: usize,
    peak_total: usize,
    next_queue: u64,
    queues: HashMap<u64, QueueState>,
    refused_queue: u64,
    refused_total: u64,
    waits: u64,
    wait_us: u64,
}

impl State {
    /// 빈 큐는 늘 받는다 — 모듈 doc 의 "한 건" 규칙. 그 외에는 두 상한을 다 보되, 제어는
    /// 합계를 면제한다([`Admission`]).
    fn admits(
        &self,
        limits: ChannelLimits,
        q: &QueueState,
        bytes: usize,
        admission: Admission,
    ) -> Result<(), Refusal> {
        if q.queued == 0 {
            return Ok(());
        }
        if q.queued.saturating_add(bytes) > limits.queue_bytes {
            return Err(Refusal::Queue);
        }
        if admission == Admission::Data && self.total.saturating_add(bytes) > limits.total_bytes {
            return Err(Refusal::Total);
        }
        Ok(())
    }

    fn charge(&mut self, id: u64, bytes: usize) {
        let Some(q) = self.queues.get_mut(&id) else {
            return;
        };
        q.queued += bytes;
        q.messages += 1;
        q.peak = q.peak.max(q.queued);
        self.total += bytes;
        self.peak_total = self.peak_total.max(self.total);
    }
}

/// 프로세스 전체의 채널 바이트 집계. 제품은 process_wide를 공유하고
/// 시험은 서로의 사용량에 영향을 주지 않도록 new로 별도 집계를 만든다.
#[derive(Debug)]
pub struct ChannelLedger {
    limits: ChannelLimits,
    state: Mutex<State>,
    freed: Condvar,
}

impl ChannelLedger {
    pub fn new(limits: ChannelLimits) -> Arc<Self> {
        Arc::new(Self {
            limits,
            state: Mutex::new(State::default()),
            freed: Condvar::new(),
        })
    }

    /// 이 프로세스의 장부 — 운영 경로의 유일한 인스턴스.
    pub fn process_wide() -> Arc<Self> {
        static LEDGER: OnceLock<Arc<ChannelLedger>> = OnceLock::new();
        LEDGER
            .get_or_init(|| Self::new(ChannelLimits::default()))
            .clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        tasty_utils::poison::recover_mutex(self.state.lock(), LEDGER_WHAT, &LEDGER_POISONED)
    }

    /// 큐 하나를 장부에 올린다. 받는 쪽이 사라지면([`MeteredReceiver`] drop) 내려간다.
    pub fn open_queue(self: &Arc<Self>, plugin_id: &str, direction: Direction) -> QueueMeter {
        let mut st = self.lock();
        let id = st.next_queue;
        st.next_queue += 1;
        st.queues.insert(
            id,
            QueueState {
                plugin_id: plugin_id.to_string(),
                direction,
                queued: 0,
                messages: 0,
                peak: 0,
            },
        );
        QueueMeter {
            ledger: self.clone(),
            id,
        }
    }

    /// 현재 집계값. 아직 IPC 조회에는 포함하지 않는다.
    pub fn snapshot(&self) -> ChannelBytesSnapshot {
        let st = self.lock();
        let mut queues: Vec<QueueBytesSnapshot> = st
            .queues
            .values()
            .map(|q| QueueBytesSnapshot {
                plugin_id: q.plugin_id.clone(),
                direction: q.direction,
                queued_bytes: q.queued,
                queued_messages: q.messages,
                peak_bytes: q.peak,
            })
            .collect();
        queues.sort_by(|a, b| {
            (a.plugin_id.as_str(), a.direction as u8)
                .cmp(&(b.plugin_id.as_str(), b.direction as u8))
        });
        ChannelBytesSnapshot {
            limits: self.limits,
            total_bytes: st.total,
            peak_total_bytes: st.peak_total,
            refused_over_queue: st.refused_queue,
            refused_over_total: st.refused_total,
            waits: st.waits,
            wait_us_total: st.wait_us,
            queues,
        }
    }
}

/// 장부 위의 큐 하나. 보내는 쪽과 받는 쪽이 함께 든다.
#[derive(Debug)]
pub struct QueueMeter {
    ledger: Arc<ChannelLedger>,
    id: u64,
}

impl QueueMeter {
    /// 한 건을 **기다리지 않고** 들인다. 거절하는 방향(요청)이 쓴다.
    pub fn try_reserve(&self, bytes: usize) -> Result<(), Refusal> {
        self.try_reserve_as(bytes, Admission::Data)
    }

    /// [`Self::try_reserve`] 에 갈래를 준 형태 — 제어는 합계를 면제한다([`Admission`]).
    pub fn try_reserve_as(&self, bytes: usize, admission: Admission) -> Result<(), Refusal> {
        let limits = self.ledger.limits;
        let mut st = self.ledger.lock();
        let Some(q) = st.queues.get(&self.id) else {
            return Err(Refusal::Closed);
        };
        match st.admits(limits, q, bytes, admission) {
            Ok(()) => {
                st.charge(self.id, bytes);
                Ok(())
            }
            Err(r) => {
                match r {
                    Refusal::Queue => st.refused_queue += 1,
                    Refusal::Total => st.refused_total += 1,
                    Refusal::Closed => {}
                }
                Err(r)
            }
        }
    }

    /// 바이트 여유가 생기거나 큐가 닫힐 때까지 기다린다.
    /// 첫 대기 전에 before_wait로 호스트를 깨워 큐를 비울 기회를 준다.
    /// 이후 WAIT_SLICE마다 닫힘 여부를 확인한다.
    pub fn reserve_waiting(
        &self,
        bytes: usize,
        mut before_wait: impl FnMut(),
    ) -> Result<(), Refusal> {
        let limits = self.ledger.limits;
        let mut st = self.ledger.lock();
        let mut waited_since: Option<Instant> = None;
        loop {
            let Some(q) = st.queues.get(&self.id) else {
                return Err(Refusal::Closed);
            };
            if st.admits(limits, q, bytes, Admission::Data).is_ok() {
                st.charge(self.id, bytes);
                if let Some(t0) = waited_since {
                    st.waits += 1;
                    st.wait_us = st.wait_us.saturating_add(
                        u64::try_from(t0.elapsed().as_micros()).unwrap_or(u64::MAX),
                    );
                }
                return Ok(());
            }
            if waited_since.is_none() {
                waited_since = Some(Instant::now());
                drop(st);
                before_wait();
                st = self.ledger.lock();
                continue;
            }
            st = match self.ledger.freed.wait_timeout(st, WAIT_SLICE) {
                Ok((g, _)) => g,
                Err(poisoned) => {
                    if !LEDGER_POISONED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                        tracing::error!("{LEDGER_WHAT} lock poisoned — recovering");
                    }
                    poisoned.into_inner().0
                }
            };
        }
    }

    /// 한 건이 큐를 떠났다.
    pub fn release(&self, bytes: usize) {
        let mut st = self.ledger.lock();
        let Some(q) = st.queues.get_mut(&self.id) else {
            // 이미 닫혔다 — 닫을 때 남은 몫을 통째로 돌려줬다. 여기서 또 빼면 두 번 뺀다.
            return;
        };
        q.queued = q.queued.saturating_sub(bytes);
        q.messages = q.messages.saturating_sub(1);
        st.total = st.total.saturating_sub(bytes);
        drop(st);
        self.ledger.freed.notify_all();
    }

    /// 큐를 장부에서 내린다 — 남아 있던 몫은 합계에서 통째로 빠진다. 두 번 불러도 된다.
    pub fn close(&self) {
        let mut st = self.ledger.lock();
        if let Some(q) = st.queues.remove(&self.id) {
            st.total = st.total.saturating_sub(q.queued);
        }
        drop(st);
        self.ledger.freed.notify_all();
    }
}

/// 바이트를 재는 채널을 만든다. 개수 상한(`capacity`)은 그대로 `sync_channel` 이 건다.
pub(crate) fn metered_channel<T>(
    capacity: usize,
    meter: QueueMeter,
) -> (MeteredSender<T>, MeteredReceiver<T>) {
    let meter = Arc::new(meter);
    let (tx, rx) = mpsc::sync_channel(capacity);
    (
        MeteredSender {
            tx,
            meter: meter.clone(),
        },
        MeteredReceiver { rx, meter },
    )
}

/// 보내는 쪽. 채널에 넣기 전에 바이트를 장부에 올린다.
#[derive(Debug)]
pub(crate) struct MeteredSender<T> {
    tx: mpsc::SyncSender<(T, usize)>,
    meter: Arc<QueueMeter>,
}

impl<T> Clone for MeteredSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            meter: self.meter.clone(),
        }
    }
}

/// [`MeteredSender::try_send`] 가 못 넣은 이유.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrySendRefusal {
    /// 개수가 찼다.
    Full,
    /// 바이트 상한 — 어느 쪽인지 함께 든다.
    Bytes(Refusal),
    /// 받는 쪽이 사라졌다.
    Disconnected,
}

impl<T> MeteredSender<T> {
    /// 큐의 여유를 기다리지 않고 바이트·개수 상한을 검사한다.
    /// 개수 상한으로 거절하면 먼저 올린 바이트를 되돌린다.
    pub(crate) fn try_send(
        &self,
        item: T,
        bytes: usize,
        admission: Admission,
    ) -> Result<(), TrySendRefusal> {
        match self.meter.try_reserve_as(bytes, admission) {
            Ok(()) => {}
            Err(Refusal::Closed) => return Err(TrySendRefusal::Disconnected),
            Err(r) => return Err(TrySendRefusal::Bytes(r)),
        }
        match super::try_send_request(&self.tx, (item, bytes)) {
            Ok(()) => Ok(()),
            Err(e) => {
                self.meter.release(bytes);
                Err(match e {
                    super::RequestSendError::Full => TrySendRefusal::Full,
                    _ => TrySendRefusal::Disconnected,
                })
            }
        }
    }

    /// 자리가 날 때까지 **기다린다** — 바이트도 개수도. 받는 쪽이 사라졌으면 `Err` 이고
    /// 그 한 건은 버려진다(더 보낼 곳이 없다).
    pub(crate) fn send_waiting(
        &self,
        item: T,
        bytes: usize,
        before_wait: impl FnMut(),
    ) -> Result<(), Refusal> {
        self.meter.reserve_waiting(bytes, before_wait)?;
        if self.tx.send((item, bytes)).is_err() {
            self.meter.release(bytes);
            return Err(Refusal::Closed);
        }
        Ok(())
    }
}

/// 수신단. 항목을 꺼낼 때 바이트를 빼고, drop되면 남은 큐의 바이트도 모두 뺀다.
#[derive(Debug)]
pub struct MeteredReceiver<T> {
    rx: mpsc::Receiver<(T, usize)>,
    meter: Arc<QueueMeter>,
}

impl<T> MeteredReceiver<T> {
    pub fn try_recv(&self) -> Result<T, mpsc::TryRecvError> {
        let (item, bytes) = self.rx.try_recv()?;
        self.meter.release(bytes);
        Ok(item)
    }

    pub fn recv(&self) -> Result<T, mpsc::RecvError> {
        let (item, bytes) = self.rx.recv()?;
        self.meter.release(bytes);
        Ok(item)
    }

    pub fn recv_timeout(&self, timeout: Duration) -> Result<T, mpsc::RecvTimeoutError> {
        let (item, bytes) = self.rx.recv_timeout(timeout)?;
        self.meter.release(bytes);
        Ok(item)
    }
}

impl<T> Drop for MeteredReceiver<T> {
    fn drop(&mut self) {
        self.meter.close();
    }
}

/// [`ChannelLedger::snapshot`] 의 값.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ChannelBytesSnapshot {
    pub limits: ChannelLimits,
    /// 지금 모든 채널에 쌓인 바이트.
    pub total_bytes: usize,
    /// 프로세스 수명 중 합계의 최댓값 — 상한이 정상 사용에 닿는지 재는 값(docs/architecture/ipc-server.md#플러그인-채널의-상한).
    pub peak_total_bytes: usize,
    /// 요청 방향에서 큐 상한으로 거절한 수.
    pub refused_over_queue: u64,
    /// 요청 방향에서 합계 상한으로 거절한 수.
    pub refused_over_total: u64,
    /// 응답·이벤트 방향에서 바이트 상한 때문에 기다린 수.
    pub waits: u64,
    /// 그 기다림의 누계(µs).
    pub wait_us_total: u64,
    /// 열린 큐 하나하나. 닫힌 큐는 없다.
    pub queues: Vec<QueueBytesSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct QueueBytesSnapshot {
    pub plugin_id: String,
    pub direction: Direction,
    pub queued_bytes: usize,
    pub queued_messages: usize,
    pub peak_bytes: usize,
}

#[cfg(test)]
#[path = "channel_bytes_tests.rs"]
mod tests;
