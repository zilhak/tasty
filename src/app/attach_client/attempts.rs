//! App continuations retain targets; Remote owns sockets, tunnels and cancellation resources.
use super::pending::PendingMirrorInstall;
use std::collections::HashMap;
use tasty_remote::{
    connection::ConnectionEpoch,
    outbound::{AttemptToken, Remote},
    pending_connection::ConnectionTicket,
};

#[derive(Default)]
pub(crate) struct MirrorAttempts {
    endpoints: HashMap<AttemptToken, PendingMirrorInstall>,
    connections: HashMap<ConnectionTicket, PendingMirrorInstall>,
}
impl MirrorAttempts {
    pub(crate) fn register_endpoint(&mut self, token: AttemptToken, target: PendingMirrorInstall) {
        self.discard_inactive_endpoints();
        self.endpoints.insert(token, target);
    }
    pub(crate) fn take_endpoint(&mut self, token: &AttemptToken) -> Option<PendingMirrorInstall> {
        self.endpoints.remove(token).filter(|_| token.is_active())
    }
    pub(crate) fn cancel_endpoint(&mut self, token: &AttemptToken, remote: &mut Remote) {
        self.endpoints.remove(token);
        remote.cancel_attempt(token);
    }
    pub(crate) fn discard_inactive_endpoints(&mut self) {
        self.endpoints.retain(|token, _| token.is_active());
    }
    pub(crate) fn endpoint_snapshot(&self) -> Vec<(AttemptToken, PendingMirrorInstall)> {
        self.endpoints
            .iter()
            .map(|(token, target)| (token.clone(), target.clone()))
            .collect()
    }
    pub(crate) fn supersede_connections(
        &mut self,
        target: &PendingMirrorInstall,
        remote: &mut Remote,
    ) {
        self.connections.retain(|ticket, pending| {
            let replaced = (target.anchor.is_some() || target.reconnect.is_some())
                && pending.engine == target.engine
                && pending.reconnect.as_ref().map(|(id, _)| *id)
                    == target.reconnect.as_ref().map(|(id, _)| *id)
                && pending.anchor == target.anchor;
            if replaced {
                remote.cancel_connection(*ticket);
            }
            !replaced
        });
    }
    pub(crate) fn register_connection(
        &mut self,
        ticket: ConnectionTicket,
        target: PendingMirrorInstall,
    ) {
        self.connections.insert(ticket, target);
    }
    pub(crate) fn finish_connection(&mut self, ticket: ConnectionTicket) {
        self.connections.remove(&ticket);
    }
    pub(crate) fn cancel_connection(&mut self, ticket: ConnectionTicket, remote: &mut Remote) {
        self.finish_connection(ticket);
        remote.cancel_connection(ticket);
    }
    pub(crate) fn connection_snapshot(&self) -> Vec<(ConnectionTicket, PendingMirrorInstall)> {
        self.connections
            .iter()
            .map(|(ticket, target)| (*ticket, target.clone()))
            .collect()
    }
    pub(crate) fn installing_epoch(&self, workspace: u32, epoch: &ConnectionEpoch) -> bool {
        self.connections.values().any(|pending| {
            pending
                .reconnect
                .as_ref()
                .is_some_and(|(id, pending_epoch)| *id == workspace && epoch.same(pending_epoch))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(
        engine: crate::runtime::engine_session::EngineId,
        anchor: Option<u32>,
    ) -> PendingMirrorInstall {
        PendingMirrorInstall {
            engine,
            anchor,
            window: None,
            view: None,
            selection: None,
            activate: false,
            mapping: None,
            mapping_token: None,
            reconnect: None,
            resync: false,
        }
    }

    #[test]
    fn cancelled_or_superseded_endpoint_cannot_claim_a_late_result() {
        let (_, engine) = crate::state::tests::test_state();
        let mut remote = Remote::new();
        let mut pending = MirrorAttempts::default();
        let old = remote.begin_attempt(Some(10), None).unwrap();
        pending.register_endpoint(old.clone(), target(engine.id, Some(10)));
        pending.cancel_endpoint(&old, &mut remote);
        assert!(!old.is_active());
        assert!(pending.take_endpoint(&old).is_none());

        let replaced = remote.begin_attempt(Some(10), None).unwrap();
        pending.register_endpoint(replaced.clone(), target(engine.id, Some(10)));
        let current = remote.begin_attempt(Some(10), None).unwrap();
        pending.register_endpoint(current.clone(), target(engine.id, Some(10)));
        assert!(pending.take_endpoint(&replaced).is_none());
        assert_eq!(pending.take_endpoint(&current).unwrap().engine, engine.id);
        assert!(pending.take_endpoint(&current).is_none());
        remote.finish_attempt(&current);
    }

    #[test]
    fn connection_replacement_and_failure_keep_other_targets_and_epochs() {
        let (_, first) = crate::state::tests::test_state();
        let (_, other) = crate::state::tests::test_state();
        let mut remote = Remote::new();
        let mut pending = MirrorAttempts::default();
        let (old_sender, _old_receiver) = tasty_remote::connection::channel();
        let (new_sender, _new_receiver) = tasty_remote::connection::channel();
        let mut old = target(first.id, Some(10));
        old.reconnect = Some((20, old_sender.epoch()));
        pending.register_connection(ConnectionTicket(1), old.clone());
        pending.register_connection(ConnectionTicket(2), target(other.id, Some(10)));
        let mut replacement = old.clone();
        replacement.reconnect = Some((20, new_sender.epoch()));
        pending.supersede_connections(&replacement, &mut remote);
        pending.register_connection(ConnectionTicket(3), replacement);
        assert!(!pending.installing_epoch(20, &old_sender.epoch()));
        assert!(pending.installing_epoch(20, &new_sender.epoch()));
        // A late failure for the replaced ticket cannot remove the replacement.
        pending.cancel_connection(ConnectionTicket(1), &mut remote);
        assert!(pending.installing_epoch(20, &new_sender.epoch()));
        assert_eq!(pending.connection_snapshot().len(), 2);
        pending.finish_connection(ConnectionTicket(3));
        assert!(!pending.installing_epoch(20, &new_sender.epoch()));
        let remaining = pending.connection_snapshot();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].1.engine, other.id);
    }
}
