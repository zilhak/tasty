//! Standalone policy on the existing resource collection; no second resource registry.
use super::TerminalStore;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tasty_terminal::Pty;

pub(crate) const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);
pub(crate) const PTY_ID_BASE: u32 = 0x8000_0000;
pub(crate) const fn is_surface_id_space(id: u32) -> bool {
    id < PTY_ID_BASE
}

/// 동시 개수는 거절 사유가 아니다. ID 무결성을 지킬 수 없을 때만 생성을 거절한다.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PtySpawnError {
    IdExhausted,
}
impl std::fmt::Display for PtySpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IdExhausted => f.write_str("headless PTY ID space exhausted"),
        }
    }
}
impl std::error::Error for PtySpawnError {}

impl TerminalStore {
    pub(crate) fn reserve_pty_id(&self) -> Result<u32, PtySpawnError> {
        self.next_pty_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
            .map_err(|_| PtySpawnError::IdExhausted)
    }
    pub(crate) fn is_standalone(&self, id: u32) -> bool {
        self.standalone(id).is_some()
    }
    pub(crate) fn standalone(&self, id: u32) -> Option<&Pty> {
        self.pty(id)
            .filter(|pty| pty.state().standalone().is_some())
    }
    pub(crate) fn standalone_iter(&self) -> impl Iterator<Item = (u32, &Pty)> {
        self.terminals.iter().filter_map(|(&id, (_, pty))| {
            pty.as_ref()
                .filter(|pty| pty.state().standalone().is_some())
                .map(|pty| (id, pty))
        })
    }
    pub(crate) fn touch_standalone(&mut self, id: u32, now: Instant) {
        if let Some(pty) = self.pty_mut(id) {
            pty.touch(now);
        }
    }
    pub(crate) fn expired_standalone_ids(&self, now: Instant) -> Vec<u32> {
        self.standalone_iter()
            .filter(|(_, pty)| {
                now.saturating_duration_since(
                    pty.state().standalone().expect("standalone").last_activity,
                ) >= self.idle_ttl
            })
            .map(|(id, _)| id)
            .collect()
    }
    #[cfg(test)]
    pub(crate) fn set_idle_ttl(&mut self, ttl: Duration) {
        self.idle_ttl = ttl;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, atomic::AtomicU32};

    #[test]
    fn shared_ids_keep_the_standalone_range_and_refuse_wraparound() {
        let counter = Arc::new(AtomicU32::new(PTY_ID_BASE));
        let first = TerminalStore::new(Arc::clone(&counter));
        let second = TerminalStore::new(counter);
        assert_eq!(first.reserve_pty_id().unwrap(), PTY_ID_BASE);
        assert_eq!(second.reserve_pty_id().unwrap(), PTY_ID_BASE + 1);
        assert!(is_surface_id_space(PTY_ID_BASE - 1));
        assert!(!is_surface_id_space(PTY_ID_BASE));
        let exhausted = TerminalStore::new(Arc::new(AtomicU32::new(u32::MAX)));
        assert_eq!(exhausted.reserve_pty_id(), Err(PtySpawnError::IdExhausted));
        assert_eq!(exhausted.reserve_pty_id(), Err(PtySpawnError::IdExhausted));
    }

    #[cfg(unix)]
    fn add(store: &mut TerminalStore, now: Instant, command: &str) -> u32 {
        let id = store.reserve_pty_id().unwrap();
        let (terminal, mut pty) = tasty_terminal::spawn_terminal(
            tasty_terminal::TerminalConfig {
                cols: 80,
                rows: 24,
                shell: Some("/bin/sh"),
                args: &["-c", command],
                surface_id: id,
                working_dir: None,
                initial_input: None,
                extra_env: &[],
            },
            Arc::new(|| {}),
        )
        .unwrap();
        pty.set_standalone(tasty_terminal::StandalonePty {
            owner_agent_id: "owner".into(),
            cwd: None,
            command: vec![command.into()],
            created_at: now,
            last_activity: now,
        });
        store.insert(id, terminal, Some(pty));
        id
    }

    #[cfg(unix)]
    #[test]
    fn exited_entries_do_not_block_new_entries_beyond_the_former_limit() {
        let _home = crate::test_support::IsolatedHome::new();
        let mut store = TerminalStore::new(Arc::new(AtomicU32::new(PTY_ID_BASE)));
        let now = Instant::now();
        let exited: Vec<u32> = (0..3).map(|_| add(&mut store, now, "exit 7")).collect();
        let deadline = Instant::now() + Duration::from_secs(10);
        for &id in &exited {
            while store.standalone(id).unwrap().state().exit().is_none() {
                store.process_surface(id);
                assert!(Instant::now() < deadline, "actual child exit not observed");
                std::thread::sleep(Duration::from_millis(5));
            }
            let pty = store.standalone(id).unwrap();
            assert_eq!(pty.state().exit().unwrap().code, Some(7));
            assert_eq!(pty.state().standalone().unwrap().owner_agent_id, "owner");
        }
        let running: Vec<u32> = (0..9)
            .map(|_| add(&mut store, now, "exec sleep 60"))
            .collect();
        assert_eq!(store.standalone_iter().count(), 12);
        assert!(running.iter().all(|&id| store.is_standalone(id)));
        let next = store.reserve_pty_id().expect("no concurrency refusal");
        assert!(next > *running.last().unwrap());
        drop(store.remove(exited[0]));
        assert!(store.get(exited[0]).is_none() && store.pty(exited[0]).is_none());
    }

    #[cfg(unix)]
    #[test]
    fn adoption_transfer_and_rollback_keep_the_physical_owner() {
        let _home = crate::test_support::IsolatedHome::new();
        let mut store = TerminalStore::new(Arc::new(AtomicU32::new(PTY_ID_BASE)));
        store.set_idle_ttl(Duration::from_secs(10));
        let now = Instant::now();
        let first = add(&mut store, now, "exec sleep 60");
        let second = add(&mut store, now, "exec sleep 60");
        store.touch_standalone(second, now + Duration::from_secs(9));
        assert_eq!(
            store.expired_standalone_ids(now + Duration::from_secs(11)),
            vec![first]
        );
        let generation = store.generation(first).unwrap();
        let pid = store.pty(first).unwrap().process_id();
        assert!(
            store
                .take_standalone_for_adoption(first, generation.value() + 1)
                .is_none()
        );
        let (terminal, pty, persisted) = store
            .take_standalone_for_adoption(first, generation.value())
            .expect("original owner");
        assert!(store.get(first).is_none());
        store
            .restore_standalone_adoption(first, terminal, pty, persisted)
            .unwrap_or_else(|_| panic!("rollback must preserve the original owner"));
        assert_eq!(store.generation(first), Some(generation));
        assert_eq!(store.pty(first).unwrap().process_id(), pid);
        assert!(store.is_standalone(first));
        let (mut terminal, mut pty, _) = store
            .take_standalone_for_adoption(first, generation.value())
            .expect("retry adoption");
        pty.adopt();
        terminal.rewire_waker(Arc::new(|| {}));
        store.insert(5, terminal, Some(pty));
        assert!(!store.is_standalone(first) && !store.is_standalone(5));
        assert!(store.get(first).is_none() && store.pty(first).is_none());
        assert_eq!(store.generation(5), Some(generation));
        assert_eq!(store.pty(5).unwrap().process_id(), pid);
        assert_eq!(
            store.expired_standalone_ids(now + Duration::from_secs(30)),
            vec![second]
        );
    }
}
