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
                            && s.relation_generation == Some(previous.generation)
                    })
                    .map(|s| s.id)
                    .collect();
                for id in ids {
                    j.close_subscription(id, "relation_replaced");
                }
            }
            // A new spawn/adopt must not import subscriptions from any earlier relation.
            let generation = j.next();
            j.live_relations.insert(
                (parent, child),
                LiveRelation {
                    generation,
                    restored: false,
                },
            );
            Ok(())
        })
    }
}
impl Journal {
    pub fn current_relation(&mut self, parent: u32, child: u32) -> u64 {
        let generation = match self.live_relations.get(&(parent, child)) {
            Some(relation) if !relation.restored => return relation.generation,
            Some(relation) => relation.generation,
            None => self.next(),
        };
        // Only a restored, positively identified logical pair may share its old
        // subscriptions with a new watch. begin_relation never takes this path.
        let restored: Vec<_> = self
            .subscriptions
            .values()
            .filter(|sub| {
                sub.accepts_pending()
                    && sub.mode == "spawn"
                    && sub.parent == parent
                    && sub.child == child
                    && self.owns_subscription(sub, parent)
                    && sub
                        .child_session
                        .as_deref()
                        .is_some_and(|id| !id.is_empty())
                    && self
                        .sessions
                        .get(&child)
                        .or_else(|| self.ended_sessions.get(&child))
                        .is_some_and(|session| {
                            sub.child_kind == session.kind && sub.matches_child(session)
                        })
            })
            .map(|sub| sub.id)
            .collect();
        for id in restored {
            self.subscriptions
                .get_mut(&id)
                .expect("collected above")
                .relation_generation = Some(generation);
        }
        self.live_relations.insert(
            (parent, child),
            LiveRelation {
                generation,
                restored: true,
            },
        );
        generation
    }
}
