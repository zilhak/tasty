//! Execution-scoped leases for asynchronous error hooks.
use super::*;
use anyhow::{Result, bail};

impl Completion {
    pub fn watch_error(&self, parent: u32, child: u32) -> Result<u64> {
        self.change(|j| {
            let sub = j
                .subscriptions
                .values()
                .rev()
                .find(|s| s.active && s.child == child && j.owns_subscription(s, parent))
                .ok_or_else(|| anyhow::anyhow!("active_subscription_required"))?;
            let subscription = sub.id;
            let generation = sub.child_generation;
            if let Some((id, _)) = j.error_observers.iter().find(|(_, observer)| {
                observer.subscription == subscription && observer.generation == generation
            }) {
                return Ok(*id);
            }
            let id = j.next();
            j.error_observers.insert(
                id,
                ErrorObserver {
                    subscription,
                    generation,
                    notified_epoch: None,
                },
            );
            Ok(id)
        })
    }
    pub fn observe_error(
        &self,
        observer: u64,
        parent: u32,
        child: u32,
        summary: &str,
    ) -> Result<bool> {
        self.change(|j| {
            if j.close_blocks_surface(child) || j.close_blocks_surface(parent) {
                return Ok(false);
            }
            let observer_id = observer;
            let Some(observer) = j.error_observers.get(&observer) else {
                return Ok(false);
            };
            let Some(sub) = j.subscriptions.get(&observer.subscription) else {
                return Ok(false);
            };
            let Some(current) = j.sessions.get(&child) else {
                return Ok(false);
            };
            if !sub.active
                || sub.await_session
                || sub.child != child
                || !j.owns_subscription(sub, parent)
                || observer.generation != Some(current.generation)
                || !sub.matches_child(current)
            {
                return Ok(false);
            }
            // Notify each logical parent once per observed epoch, even when its
            // spawn and tell hooks overlap. Other parents still receive their log.
            let epoch = current.epoch;
            let notified = j.error_observers.values().any(|other| {
                other.generation == observer.generation
                    && other.notified_epoch == Some(epoch)
                    && j.subscriptions
                        .get(&other.subscription)
                        .is_some_and(|owner| {
                            owner.child == child
                                && owner.parent == parent
                                && owner.parent_session == sub.parent_session
                        })
            });
            if notified {
                return Ok(false);
            }
            if current.kind != "claude" {
                bail!("error_observer_kind_mismatch");
            }
            actions::record_observation(j, child, "stalled", "claude-error-stalled", summary)?;
            j.error_observers
                .get_mut(&observer_id)
                .expect("validated above")
                .notified_epoch = Some(epoch);
            Ok(true)
        })
    }
}
