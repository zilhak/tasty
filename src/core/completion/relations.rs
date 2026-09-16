//! Live relationship generations are independent of delayed agent SessionStart.
use super::*;
use anyhow::Result;
impl Completion {
    /// Called only when the host creates a new spawn/adopt relationship.
    pub fn begin_relation(&self, parent: u32, child: u32) -> Result<()> {
        self.change(|j| {
            if let Some(previous) = j.live_relations.remove(&(parent, child)) {
                let ids: Vec<_> = j
                    .subscriptions
                    .values()
                    .filter(|s| {
                        s.parent == parent
                            && s.child == child
                            && s.mode == "spawn"
                            && s.relation_generation == Some(previous)
                    })
                    .map(|s| s.id)
                    .collect();
                for id in ids {
                    j.close_subscription(id, "relation_replaced");
                }
            }
            j.current_relation(parent, child);
            Ok(())
        })
    }
}
impl Journal {
    pub fn current_relation(&mut self, parent: u32, child: u32) -> u64 {
        if let Some(generation) = self.live_relations.get(&(parent, child)) {
            return *generation;
        }
        // The host validates registry membership before subscribe. This also
        // establishes a lease for an existing relation without an agent session.
        let generation = self.next();
        self.live_relations.insert((parent, child), generation);
        generation
    }
}
