//! Ephemeral, owner-published presence. Absence in one window never closes another's child.
use super::*;
use std::collections::HashSet;
impl Completion {
    pub fn publish_owner(&self, owner: &Arc<()>, live: HashSet<u32>) -> Result<()> {
        let mut owners = self
            .live_owners
            .lock()
            .map_err(|_| anyhow::anyhow!("completion live ownership unavailable"))?;
        owners.retain(|_, (owner, _)| owner.strong_count() > 0);
        owners.insert(Arc::as_ptr(owner) as usize, (Arc::downgrade(owner), live));
        Ok(())
    }
    pub fn subscribe_live(
        &self,
        parent: u32,
        child: u32,
        kind: &str,
        mode: &str,
        await_session: bool,
    ) -> Result<u64> {
        // Hold presence through the journal commit. A concurrent explicit close
        // either removes presence first, or captures this newly committed watch.
        let owners = self
            .live_owners
            .lock()
            .map_err(|_| anyhow::anyhow!("completion live ownership unavailable"))?;
        if !owners
            .values()
            .any(|(owner, live)| owner.strong_count() > 0 && live.contains(&child))
        {
            anyhow::bail!("target_surface_not_live");
        }
        self.subscribe_wait(parent, child, kind, mode, await_session)
    }
    #[cfg(test)]
    pub fn target_live(&self, surface: u32) -> Result<bool> {
        Ok(self
            .live_owners
            .lock()
            .map_err(|_| anyhow::anyhow!("completion live ownership unavailable"))?
            .values()
            .any(|(owner, live)| owner.strong_count() > 0 && live.contains(&surface)))
    }
    pub fn forget_live(&self, surface: u32) -> Result<()> {
        for (_, live) in self
            .live_owners
            .lock()
            .map_err(|_| anyhow::anyhow!("completion live ownership unavailable"))?
            .values_mut()
        {
            live.remove(&surface);
        }
        Ok(())
    }
}
impl crate::core::CoreState {
    pub(crate) fn publish_completion_ownership(&self) -> Result<()> {
        self.completion
            .publish_owner(&self.completion_view, self.live_surface_ids())
    }
}
