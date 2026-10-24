//! Weak grant/member lifetimes invalidate queued publication synchronously at occupancy changes.
use super::*;
use std::sync::{Arc, Weak};
impl OccupancyRegistry {
    pub(crate) fn workspace_publication(
        &self,
        surface: SurfaceId,
        client: AttachClientId,
        sequence: u64,
    ) -> Option<(Weak<()>, Weak<()>)> {
        let workspace = self.workspace_of_surface(surface)?;
        let grant = self.workspace_locks.get(&workspace)?;
        if !grant.ready
            || grant.holder != client
            || grant.granted_seq != sequence
            || self.is_dead(client)
        {
            return None;
        }
        Some((
            Arc::downgrade(self.workspace_publications.get(&workspace)?),
            Arc::downgrade(self.member_publications.get(&surface)?),
        ))
    }
    pub(super) fn set_publication_ready(
        &mut self,
        client: AttachClientId,
        sequence: u64,
        ready: bool,
    ) {
        for (workspace, grant) in &self.workspace_locks {
            if grant.holder == client && grant.granted_seq == sequence {
                if ready && !grant.ready {
                    self.workspace_publications.insert(*workspace, Arc::new(()));
                } else if !ready {
                    self.workspace_publications.remove(workspace);
                }
            }
        }
    }
    pub(super) fn retire_dead_publications(&mut self, clients: &[AttachClientId]) {
        for (workspace, grant) in &self.workspace_locks {
            if clients.contains(&grant.holder) {
                self.workspace_publications.remove(workspace);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn readiness_revokes_old_grant_without_reviving_queued_publication() {
        let mut registry = OccupancyRegistry::default();
        let grant = registry.acquire_workspace(7, &[], &[1, 2], 41).unwrap();
        let (original, _) = registry
            .workspace_publication(1, 41, grant.granted_seq)
            .unwrap();
        registry.set_attachment_ready(41, grant.granted_seq, false);
        assert!(original.upgrade().is_none());
        assert!(
            registry
                .workspace_publication(1, 41, grant.granted_seq)
                .is_none()
        );
        registry.set_attachment_ready(41, grant.granted_seq, true);
        assert!(
            registry
                .workspace_publication(1, 41, grant.granted_seq)
                .is_some()
        );
        assert!(original.upgrade().is_none());
    }
    #[test]
    fn member_move_and_close_revoke_only_that_surface() {
        let mut registry = OccupancyRegistry::default();
        let grant = registry.acquire_workspace(7, &[], &[1, 2], 41).unwrap();
        let (_, first) = registry
            .workspace_publication(1, 41, grant.granted_seq)
            .unwrap();
        let (_, sibling) = registry
            .workspace_publication(2, 41, grant.granted_seq)
            .unwrap();
        registry.add_workspace_member(7, 1, false);
        assert!(first.upgrade().is_some());
        let other = registry.acquire_workspace(8, &[], &[], 42).unwrap();
        registry.add_workspace_member(8, 1, false);
        assert!(first.upgrade().is_none());
        assert!(sibling.upgrade().is_some());
        assert!(
            registry
                .workspace_publication(1, 41, grant.granted_seq)
                .is_none()
        );
        let (_, moved) = registry
            .workspace_publication(1, 42, other.granted_seq)
            .unwrap();
        registry.forget_closed_surface(1);
        assert!(moved.upgrade().is_none());
        assert!(sibling.upgrade().is_some());
    }
    #[test]
    fn disconnect_marker_and_force_detach_invalidate_before_later_cleanup() {
        let mut registry = OccupancyRegistry::default();
        let grant = registry.acquire_workspace(7, &[], &[1], 41).unwrap();
        let (old, _) = registry
            .workspace_publication(1, 41, grant.granted_seq)
            .unwrap();
        registry.mark_clients_disconnected(&[41]);
        assert!(old.upgrade().is_none());
        registry.release_all_for_client(41);
        let fresh = registry.acquire_workspace(7, &[], &[1], 41).unwrap();
        let (live, _) = registry
            .workspace_publication(1, 41, fresh.granted_seq)
            .unwrap();
        registry.force_detach_workspace(7);
        assert!(live.upgrade().is_none());
        assert!(old.upgrade().is_none());
    }
}
