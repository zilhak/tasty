//! Secondary host continuations use the existing admission ticket and writer byte policy.
use super::Failure;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    sync::Arc,
};
use tasty_ipc::admission::{AdmissionTicket, CommandAdmission, Origin, QueueLimits};

pub(crate) type Held = Rc<Reservation>;
pub(crate) struct Budget {
    ledger: Arc<CommandAdmission>,
    limit: usize,
    state: RefCell<State>,
}
#[derive(Default)]
struct State {
    next: u64,
    weights: BTreeMap<u64, usize>,
    aggregate: Option<AdmissionTicket>,
}
pub(crate) struct Reservation {
    budget: Rc<Budget>,
    id: u64,
    bytes: Cell<usize>,
}
impl Default for Budget {
    fn default() -> Self {
        let limit = tasty_utils::batch_writer::Limits::default().queue_bytes;
        Self {
            ledger: CommandAdmission::new(QueueLimits {
                queued_bytes: limit,
                ..QueueLimits::DEFAULT
            }),
            limit,
            state: RefCell::new(State::default()),
        }
    }
}
impl Budget {
    pub(crate) fn reserve(self: &Rc<Self>, bytes: usize) -> Result<Held, Failure> {
        self.reserve_inner(bytes, false)
    }
    #[cfg(feature = "gui")]
    pub(crate) fn retain_result(self: &Rc<Self>, bytes: usize) -> Held {
        self.reserve_inner(bytes, true)
            .expect("completed result accounting does not reject")
    }
    fn reserve_inner(self: &Rc<Self>, bytes: usize, result: bool) -> Result<Held, Failure> {
        let mut state = self.state.borrow_mut();
        let total = state
            .weights
            .values()
            .copied()
            .fold(bytes, usize::saturating_add);
        self.check(total, result)?;
        state.next = state
            .next
            .checked_add(1)
            .expect("retention identity exhausted");
        let id = state.next;
        state.weights.insert(id, bytes);
        self.rebook(&mut state);
        Ok(Rc::new(Reservation {
            budget: self.clone(),
            id,
            bytes: Cell::new(bytes),
        }))
    }
    fn check(&self, total: usize, result: bool) -> Result<(), Failure> {
        if !result && total > self.limit {
            Err(Failure::before(
                "plugin lifecycle retained-byte budget full",
            ))
        } else {
            Ok(())
        }
    }
    fn rebook(&self, state: &mut State) {
        // This ledger is private to the host thread. Rebooking never admits a new command.
        // Its empty-ledger oversized rule accounts already-produced results, as writer slots do.
        // New request/coalescing admission still strictly checks the unchanged writer byte limit.
        state.aggregate = None;
        if !state.weights.is_empty() {
            let total = state
                .weights
                .values()
                .copied()
                .fold(0, usize::saturating_add);
            state.aggregate = Some(
                self.ledger
                    .admit(total, Origin::Socket)
                    .expect("private aggregate ledger is empty"),
            );
        }
    }
    #[cfg(test)]
    pub(crate) fn bytes(&self) -> usize {
        self.ledger.snapshot().queued_bytes as usize
    }
}
impl Reservation {
    pub(crate) fn bytes(&self) -> usize {
        self.bytes.get()
    }
    pub(crate) fn resize(&self, bytes: usize) -> Result<(), Failure> {
        self.change(bytes, false)
    }
    pub(crate) fn result(&self, bytes: usize) {
        self.change(bytes, true)
            .expect("completed result accounting does not reject");
    }
    fn change(&self, bytes: usize, result: bool) -> Result<(), Failure> {
        let mut state = self.budget.state.borrow_mut();
        let total = state
            .weights
            .iter()
            .filter(|(id, _)| **id != self.id)
            .map(|(_, bytes)| *bytes)
            .fold(bytes, usize::saturating_add);
        self.budget.check(total, result)?;
        state.weights.insert(self.id, bytes);
        self.bytes.set(bytes);
        self.budget.rebook(&mut state);
        Ok(())
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        let mut state = self.budget.state.borrow_mut();
        state.weights.remove(&self.id);
        self.budget.rebook(&mut state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_resize_preserves_admitted_values_and_drop_releases_accounting() {
        let budget = Rc::new(Budget::default());
        let limit = tasty_utils::batch_writer::Limits::default().queue_bytes;
        let held = budget.reserve(limit - 8).unwrap();
        assert!(budget.reserve(9).is_err());
        assert!(held.resize(limit + 1).is_err());
        assert_eq!(held.bytes(), limit - 8);
        assert_eq!(budget.bytes(), limit - 8);
        drop(held);
        assert_eq!(budget.bytes(), 0);
        assert!(budget.reserve(limit).is_ok());
    }
    #[test]
    fn completed_output_is_charged_until_delivery_and_blocks_new_admission() {
        let budget = Rc::new(Budget::default());
        let limit = tasty_utils::batch_writer::Limits::default().queue_bytes;
        let held = budget.reserve(1).unwrap();
        held.result(limit + 17);
        assert_eq!(budget.bytes(), limit + 17);
        assert!(budget.reserve(1).is_err());
        drop(held);
        assert_eq!(budget.bytes(), 0);
        assert!(budget.reserve(1).is_ok());
    }
}
