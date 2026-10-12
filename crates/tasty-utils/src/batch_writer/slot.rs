//! 요청 하나의 결과 칸과 예산. 결과는 요청마다 한 칸이라 worker가 완료를 전할 때 막히지 않는다.
//!
//! 접수가 잡은 항목 수와 바이트는 결과를 꺼내거나 receipt를 버릴 때까지 유지한다. 실행 전에는
//! 요청 바이트를, 완료 뒤에는 결과 바이트를 센다.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use crate::poison::{recover_mutex, recover_poisoned};

static SLOT_POISONED: AtomicBool = AtomicBool::new(false);
const SLOT_WHAT: &str = "batch writer result slot";

/// 접수부터 결과 소비까지 잡혀 있는 항목 수와 바이트.
#[derive(Debug, Default)]
pub(crate) struct Budget {
    pub(crate) items: AtomicUsize,
    pub(crate) bytes: AtomicUsize,
}

impl Budget {
    fn release(&self, bytes: usize) {
        self.items.fetch_sub(1, Ordering::AcqRel);
        self.bytes.fetch_sub(bytes, Ordering::AcqRel);
    }

    fn replace(&self, from: usize, to: usize) {
        if to >= from {
            self.bytes.fetch_add(to - from, Ordering::AcqRel);
        } else {
            self.bytes.fetch_sub(from - to, Ordering::AcqRel);
        }
    }
}

enum State<T> {
    Pending {
        bytes: usize,
    },
    Ready {
        value: T,
        bytes: usize,
    },
    /// 결과 전에 receipt가 버려졌다. 완료 때 예산을 돌려준다.
    Abandoned {
        bytes: usize,
    },
    Taken,
}

pub(crate) struct Slot<T> {
    state: Mutex<State<T>>,
    ready: Condvar,
    /// None이면 예산에 잡히지 않은 칸(flush)이다.
    budget: Option<Arc<Budget>>,
}

impl<T> Slot<T> {
    pub(crate) fn new(budget: Option<Arc<Budget>>, bytes: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State::Pending { bytes }),
            ready: Condvar::new(),
            budget,
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State<T>> {
        recover_mutex(self.state.lock(), SLOT_WHAT, &SLOT_POISONED)
    }

    fn release(&self, bytes: usize) {
        if let Some(budget) = &self.budget {
            budget.release(bytes);
        }
    }

    /// 결과를 넣는다. receipt가 이미 버려졌으면 결과를 버리고 예산을 돌려준다.
    fn complete(&self, value: T, out_bytes: usize) {
        let mut state = self.lock();
        match std::mem::replace(&mut *state, State::Taken) {
            State::Pending { bytes } => {
                if let Some(budget) = &self.budget {
                    budget.replace(bytes, out_bytes);
                }
                *state = State::Ready {
                    value,
                    bytes: out_bytes,
                };
                self.ready.notify_all();
            }
            State::Abandoned { bytes } => self.release(bytes),
            done @ (State::Ready { .. } | State::Taken) => {
                // Ticket은 한 번만 완료하므로 오지 않는다. 먼저 온 결과를 유지한다.
                *state = done;
            }
        }
    }

    fn take_locked(&self, state: &mut State<T>) -> Option<T> {
        match std::mem::replace(state, State::Taken) {
            State::Ready { value, bytes } => {
                self.release(bytes);
                Some(value)
            }
            other => {
                *state = other;
                None
            }
        }
    }

    fn abandon(&self) {
        let mut state = self.lock();
        match std::mem::replace(&mut *state, State::Taken) {
            State::Pending { bytes } => *state = State::Abandoned { bytes },
            State::Ready { bytes, .. } => self.release(bytes),
            other => *state = other,
        }
    }
}

/// worker 쪽 완료 권한. 완료하지 않고 버려지면 `lost` 결과로 끝낸다(worker 종료·panic).
pub(crate) struct Ticket<T> {
    slot: Option<Arc<Slot<T>>>,
    lost: fn() -> T,
}

impl<T> Ticket<T> {
    pub(crate) fn new(slot: Arc<Slot<T>>, lost: fn() -> T) -> Self {
        Self {
            slot: Some(slot),
            lost,
        }
    }

    pub(crate) fn complete(mut self, value: T, out_bytes: usize) {
        if let Some(slot) = self.slot.take() {
            slot.complete(value, out_bytes);
        }
    }
}

impl<T> Drop for Ticket<T> {
    fn drop(&mut self) {
        if let Some(slot) = self.slot.take() {
            slot.complete((self.lost)(), 0);
        }
    }
}

/// 접수한 요청 하나의 결과를 받는 쪽. 버려도 요청은 취소되지 않는다.
pub struct Receipt<T> {
    slot: Arc<Slot<T>>,
    seq: u64,
}

impl<T> Receipt<T> {
    pub(crate) fn new(slot: Arc<Slot<T>>, seq: u64) -> Self {
        Self { slot, seq }
    }

    /// 접수 순서 번호. 같은 writer 안에서 1부터 증가한다.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// 결과가 있으면 꺼낸다. 기다리지 않는다.
    pub fn try_take(&self) -> Option<T> {
        let mut state = self.slot.lock();
        self.slot.take_locked(&mut state)
    }

    /// 결과를 `timeout`까지 기다린다. 메인 루프에서는 `try_take`를 쓴다.
    pub fn wait_timeout(&self, timeout: Duration) -> Option<T> {
        let deadline = Instant::now() + timeout;
        let mut state = self.slot.lock();
        loop {
            if let Some(value) = self.slot.take_locked(&mut state) {
                return Some(value);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return None;
            }
            let (guard, _) = self
                .slot
                .ready
                .wait_timeout(state, left)
                .unwrap_or_else(|poisoned| recover_poisoned(poisoned, SLOT_WHAT, &SLOT_POISONED));
            state = guard;
        }
    }
}

impl<T> Drop for Receipt<T> {
    fn drop(&mut self) {
        self.slot.abandon();
    }
}

impl<T> std::fmt::Debug for Receipt<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Receipt").field("seq", &self.seq).finish()
    }
}
