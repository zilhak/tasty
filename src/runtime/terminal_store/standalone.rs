//! Standalone policy on the existing resource collection; no second resource registry.
use super::TerminalStore;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};
use tasty_terminal::Pty;

pub(crate) const DEFAULT_MAX_CONCURRENT: usize = 8;
pub(crate) const DEFAULT_IDLE_TTL: Duration = Duration::from_secs(300);
pub(crate) const PTY_ID_BASE: u32 = 0x8000_0000;
pub(crate) const fn is_surface_id_space(id: u32) -> bool {
    id < PTY_ID_BASE
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum PtySpawnError {
    LimitReached { current: usize, max: usize },
    IdExhausted,
}
impl std::fmt::Display for PtySpawnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LimitReached { current, max } => write!(
                f,
                "headless PTY concurrency limit reached ({current}/{max})"
            ),
            Self::IdExhausted => f.write_str("headless PTY ID space exhausted"),
        }
    }
}
impl std::error::Error for PtySpawnError {}

impl TerminalStore {
    pub(crate) fn reserve_pty_id(&self) -> Result<u32, PtySpawnError> {
        let current = self.standalone_iter().count();
        if current >= self.max_standalone {
            return Err(PtySpawnError::LimitReached {
                current,
                max: self.max_standalone,
            });
        }
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
    pub(crate) fn set_standalone_limits(&mut self, max: usize, ttl: Duration) {
        self.max_standalone = max;
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
    fn exited_entries_keep_their_limit_slot_until_the_single_entry_is_removed() {
        let _home = crate::test_support::IsolatedHome::new();
        let mut store = TerminalStore::new(Arc::new(AtomicU32::new(PTY_ID_BASE)));
        store.set_standalone_limits(1, DEFAULT_IDLE_TTL);
        let id = add(&mut store, Instant::now(), "exit 7");
        let deadline = Instant::now() + Duration::from_secs(10);
        while store.standalone(id).unwrap().state().exit().is_none() {
            store.process_surface(id);
            assert!(Instant::now() < deadline, "actual child exit not observed");
            std::thread::sleep(Duration::from_millis(5));
        }
        let pty = store.standalone(id).unwrap();
        assert_eq!(pty.state().exit().unwrap().code, Some(7));
        assert_eq!(pty.state().standalone().unwrap().owner_agent_id, "owner");
        assert_eq!(
            store.reserve_pty_id(),
            Err(PtySpawnError::LimitReached { current: 1, max: 1 })
        );
        drop(store.remove(id));
        assert!(store.get(id).is_none() && store.pty(id).is_none());
        assert!(store.reserve_pty_id().unwrap() > id);
    }

    #[cfg(unix)]
    #[test]
    fn touch_and_adopt_use_the_same_metadata_and_physical_owner() {
        let _home = crate::test_support::IsolatedHome::new();
        let mut store = TerminalStore::new(Arc::new(AtomicU32::new(PTY_ID_BASE)));
        store.set_standalone_limits(2, Duration::from_secs(10));
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
        assert!(store.adopt(first, 5, Arc::new(|| {})));
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
