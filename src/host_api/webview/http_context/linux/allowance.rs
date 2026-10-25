//! Native exceptions live as long as the manager; every successful leaf remains charged.
use super::*;
use std::collections::BTreeSet;
// Eight pending decisions, with at most eight such batches retained by a mirror manager.
pub(super) const LIMIT: usize = 64;
#[derive(Default)]
pub(super) struct History {
    allowed: BTreeSet<[u8; 32]>,
    reserved: BTreeSet<[u8; 32]>,
}
pub(super) struct Permit {
    history: Rc<RefCell<History>>,
    leaf: [u8; 32],
    committed: bool,
}
impl History {
    pub fn full(&self) -> bool {
        self.allowed.len() + self.reserved.len() >= LIMIT
    }
    pub fn check(&self, leaf: [u8; 32]) -> Result<(), ContextError> {
        if self.allowed.contains(&leaf) || self.reserved.contains(&leaf) {
            return Err(ContextError::Tls);
        }
        if self.full() {
            return Err(ContextError::Capacity);
        }
        Ok(())
    }
    pub fn reserve(history: &Rc<RefCell<Self>>, leaf: [u8; 32]) -> Result<Permit, ContextError> {
        let mut state = history.borrow_mut();
        state.check(leaf)?;
        state.reserved.insert(leaf);
        Ok(Permit {
            history: history.clone(),
            leaf,
            committed: false,
        })
    }
    #[cfg(test)]
    pub fn used(&self) -> usize {
        self.allowed.len()
    }
}
impl Permit {
    pub fn commit(&mut self) {
        self.history.borrow_mut().reserved.remove(&self.leaf);
        self.history.borrow_mut().allowed.insert(self.leaf);
        self.committed = true;
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        if !self.committed {
            self.history.borrow_mut().reserved.remove(&self.leaf);
        }
    }
}
