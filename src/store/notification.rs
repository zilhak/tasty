use std::time::{Duration, Instant};

use crate::model::{SurfaceId, WorkspaceId};

/// Unique notification identifier.
type NotificationId = u64;

/// A single notification entry.
pub struct Notification {
    pub id: NotificationId,
    pub source_workspace: WorkspaceId,
    pub source_surface: SurfaceId,
    pub title: String,
    pub body: String,
    pub timestamp: Instant,
    pub read: bool,
}

/// Stores and manages terminal notifications with FIFO eviction.
pub struct NotificationStore {
    notifications: std::collections::VecDeque<Notification>,
    max_count: usize,
    next_id: u64,
    /// Coalesce window in milliseconds.
    coalesce_ms: u64,
}

impl NotificationStore {
    /// Create a notification store with a custom coalesce window.
    pub fn with_coalesce_ms(coalesce_ms: u64) -> Self {
        Self {
            notifications: std::collections::VecDeque::new(),
            max_count: 100,
            next_id: 1,
            coalesce_ms,
        }
    }

    /// Add a notification, coalescing if the same source sent one within the coalesce window.
    /// Returns `Some(id)` for a newly-created notification, `None` when the call coalesced
    /// into an existing entry (Event Bus 발화 시 신규 알림만 broadcast하기 위한 분기).
    pub fn add(
        &mut self,
        source_workspace: WorkspaceId,
        source_surface: SurfaceId,
        title: String,
        body: String,
    ) -> Option<u64> {
        let now = Instant::now();
        let coalesce_window = Duration::from_millis(self.coalesce_ms);

        // Coalesce: if same source sent a notification recently, merge
        if let Some(existing) = self.notifications.iter_mut().rev().find(|n| {
            n.source_workspace == source_workspace
                && n.source_surface == source_surface
                && now.duration_since(n.timestamp) < coalesce_window
        }) {
            if !body.is_empty() {
                if existing.body.is_empty() {
                    existing.body = body;
                } else {
                    existing.body = format!("{}\n{}", existing.body, body);
                }
            }
            if !title.is_empty() {
                existing.title = title;
            }
            existing.timestamp = now;
            return None;
        }

        // FIFO eviction
        while self.notifications.len() >= self.max_count {
            self.notifications.pop_front();
        }

        let id = self.next_id;
        self.next_id += 1;

        // NOTE: surface attention 발동은 이제 producer(호출처)가 담당한다
        // (`CoreState::raise_attention`). NotificationStore 는 알림 엔트리
        // 저장/coalesce 만 책임진다 — attention 은 producer 중립 공유 상태이자
        // NotificationStore 와 별개 저장소다.

        self.notifications.push_back(Notification {
            id,
            source_workspace,
            source_surface,
            title,
            body,
            timestamp: now,
            read: false,
        });
        Some(id)
    }

    /// Total unread notification count.
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    /// Whether any unread notification originates from the given surface.
    /// 알림 읽음 처리 시 그 surface 의 highlight 를 지워도 되는지 판단하는 데 쓰인다.
    pub fn has_unread_for_surface(&self, surface_id: SurfaceId) -> bool {
        self.notifications
            .iter()
            .any(|n| !n.read && n.source_surface == surface_id)
    }

    /// Get all notifications (newest last).
    pub fn all(&self) -> impl DoubleEndedIterator<Item = &Notification> + ExactSizeIterator {
        self.notifications.iter()
    }

    /// Mark a specific notification as read.
    pub fn mark_read(&mut self, id: NotificationId) {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.read = true;
        }
    }

    /// Mark all notifications as read.
    pub fn mark_all_read(&mut self) {
        for n in &mut self.notifications {
            n.read = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_count() {
        let mut store = NotificationStore::with_coalesce_ms(500);
        assert_eq!(store.unread_count(), 0);
        store.add(1, 1, "Title".into(), "Body".into());
        assert_eq!(store.unread_count(), 1);
    }

    #[test]
    fn mark_read() {
        let mut store = NotificationStore::with_coalesce_ms(500);
        store.add(1, 1, "T".into(), "B".into());
        assert_eq!(store.unread_count(), 1);
        let id = store.all().next().unwrap().id;
        store.mark_read(id);
        assert_eq!(store.unread_count(), 0);
    }

    #[test]
    fn mark_all_read() {
        let mut store = NotificationStore::with_coalesce_ms(500);
        store.add(1, 1, "A".into(), "".into());
        store.add(1, 2, "B".into(), "".into());
        assert_eq!(store.unread_count(), 2);
        store.mark_all_read();
        assert_eq!(store.unread_count(), 0);
    }

    #[test]
    fn coalescing() {
        // With a large coalesce window, notifications from the same source should merge
        let mut store = NotificationStore::with_coalesce_ms(60000);
        store.add(1, 1, "Title".into(), "first".into());
        store.add(1, 1, "Title".into(), "second".into());
        // Should still be 1 notification (coalesced)
        assert_eq!(store.all().len(), 1);
        let n = store.all().next().unwrap();
        assert!(n.body.contains("first"));
        assert!(n.body.contains("second"));
    }

    #[test]
    fn no_coalescing_different_sources() {
        let mut store = NotificationStore::with_coalesce_ms(60000);
        store.add(1, 1, "A".into(), "".into());
        store.add(1, 2, "B".into(), "".into()); // different surface
        assert_eq!(store.all().len(), 2);
    }

    #[test]
    fn has_unread_for_surface_reflects_remaining_unread() {
        let mut store = NotificationStore::with_coalesce_ms(0);
        let id1 = store.add(1, 100, "t1".into(), "b1".into()).unwrap();
        let _id2 = store.add(1, 100, "t2".into(), "b2".into()).unwrap(); // 다른 알림, 같은 surface
        assert!(store.has_unread_for_surface(100));
        store.mark_read(id1);
        assert!(store.has_unread_for_surface(100)); // id2 아직 안읽음
    }

    #[test]
    fn has_unread_for_surface_false_when_none_or_all_read() {
        let mut store = NotificationStore::with_coalesce_ms(0);
        assert!(!store.has_unread_for_surface(100));
        let id = store.add(1, 100, "t".into(), "b".into()).unwrap();
        assert!(store.has_unread_for_surface(100));
        store.mark_read(id);
        assert!(!store.has_unread_for_surface(100));
    }

    #[test]
    fn fifo_eviction() {
        let mut store = NotificationStore::with_coalesce_ms(0);
        // Default max is 100
        for i in 0..110 {
            store.add(1, i as u32, format!("N{}", i), "".into());
        }
        assert_eq!(store.all().len(), 100);
    }
}
