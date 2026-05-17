//! Per-surface TTL cache for port scans.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::{CachedScan, ListeningPort};

/// LRU-ish cache keyed by surface_id (or any caller-chosen key). Entries
/// older than `ttl` are considered stale; callers should kick a fresh scan
/// when the entry is missing or stale.
#[derive(Debug)]
pub struct PortScanCache {
    ttl: Duration,
    entries: HashMap<u32, CachedScan>,
}

impl PortScanCache {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: HashMap::new(),
        }
    }

    /// Returns the cached ports if fresh, else None.
    pub fn get_fresh(&self, key: u32, now: Instant) -> Option<&[ListeningPort]> {
        let entry = self.entries.get(&key)?;
        if now.duration_since(entry.at) <= self.ttl {
            Some(&entry.ports)
        } else {
            None
        }
    }

    /// Returns the last-known ports regardless of staleness (UI fallback).
    pub fn get_any(&self, key: u32) -> Option<&[ListeningPort]> {
        self.entries.get(&key).map(|e| e.ports.as_slice())
    }

    /// Insert a fresh scan result.
    pub fn insert(&mut self, key: u32, ports: Vec<ListeningPort>, at: Instant) {
        self.entries.insert(key, CachedScan { ports, at });
    }

    /// Whether the entry is missing or stale.
    pub fn needs_refresh(&self, key: u32, now: Instant) -> bool {
        match self.entries.get(&key) {
            None => true,
            Some(entry) => now.duration_since(entry.at) > self.ttl,
        }
    }

    /// Drop an entry (e.g. surface closed).
    pub fn forget(&mut self, key: u32) {
        self.entries.remove(&key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn lp(pid: u32, port: u16) -> ListeningPort {
        ListeningPort {
            pid,
            port,
            addr: Ipv4Addr::new(127, 0, 0, 1).into(),
        }
    }

    #[test]
    fn fresh_then_stale() {
        let mut cache = PortScanCache::new(Duration::from_secs(5));
        let t0 = Instant::now();
        cache.insert(1, vec![lp(100, 8080)], t0);

        assert_eq!(cache.get_fresh(1, t0).map(|s| s.len()), Some(1));
        assert!(!cache.needs_refresh(1, t0));

        let t1 = t0 + Duration::from_secs(6);
        assert!(cache.get_fresh(1, t1).is_none());
        assert!(cache.needs_refresh(1, t1));
        // get_any still returns stale data.
        assert_eq!(cache.get_any(1).map(|s| s.len()), Some(1));
    }

    #[test]
    fn missing_needs_refresh() {
        let cache = PortScanCache::new(Duration::from_secs(5));
        assert!(cache.needs_refresh(42, Instant::now()));
    }

    #[test]
    fn forget_removes() {
        let mut cache = PortScanCache::new(Duration::from_secs(5));
        cache.insert(1, vec![lp(100, 8080)], Instant::now());
        cache.forget(1);
        assert!(cache.get_any(1).is_none());
    }
}
