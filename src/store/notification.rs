use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::{Duration, Instant};

use crate::model::{SurfaceId, WorkspaceId};

type NotificationId = u64;

/// User-facing notification severity, independent of logs and surface attention.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    Error,
    Warning,
    #[default]
    Info,
}

impl NotificationLevel {
    #[cfg(feature = "gui")]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct UnreadCounts {
    pub error: usize,
    pub warning: usize,
    pub info: usize,
}

impl UnreadCounts {
    pub fn total(self) -> usize {
        self.error + self.warning + self.info
    }

    #[cfg(any(feature = "gui", test))]
    pub fn add(&mut self, other: Self) {
        self.error += other.error;
        self.warning += other.warning;
        self.info += other.info;
    }
}

pub struct Notification {
    pub id: NotificationId,
    pub level: NotificationLevel,
    pub source_workspace: WorkspaceId,
    pub source_surface: SurfaceId,
    pub title: String,
    pub body: String,
    pub timestamp: Instant,
    pub read: bool,
}

/// 알림을 합치고 개수 상한에 도달하면 오래된 항목부터 제거한다.
/// surface attention은 별도 저장소이며 호출자가 갱신한다.
pub struct NotificationStore {
    notifications: std::collections::VecDeque<Notification>,
    max_count: usize,
    next_id: Arc<AtomicU64>,
    coalesce_ms: u64,
}

impl NotificationStore {
    #[cfg(test)]
    pub fn with_coalesce_ms(coalesce_ms: u64) -> Self {
        Self::with_counter(coalesce_ms, Arc::new(AtomicU64::new(1)))
    }

    /// 창별 저장소가 같은 ID 카운터를 공유해 전역 IPC 조회에서 항목을 구분한다.
    pub fn with_counter(coalesce_ms: u64, next_id: Arc<AtomicU64>) -> Self {
        Self {
            notifications: std::collections::VecDeque::new(),
            max_count: 100,
            next_id,
            coalesce_ms,
        }
    }

    /// Tests without an explicit level use the backwards-compatible info default.
    #[cfg(test)]
    pub fn add(
        &mut self,
        source_workspace: WorkspaceId,
        source_surface: SurfaceId,
        title: String,
        body: String,
    ) -> Option<u64> {
        self.add_with_level(
            source_workspace,
            source_surface,
            title,
            body,
            NotificationLevel::Info,
        )
    }

    pub fn add_with_level(
        &mut self,
        source_workspace: WorkspaceId,
        source_surface: SurfaceId,
        title: String,
        body: String,
        level: NotificationLevel,
    ) -> Option<u64> {
        let now = Instant::now();
        let coalesce_window = Duration::from_millis(self.coalesce_ms);

        if let Some(existing) = self.notifications.iter_mut().rev().find(|n| {
            n.level == level
                && n.source_workspace == source_workspace
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
            existing.read = false;
            return None;
        }

        while self.notifications.len() >= self.max_count {
            self.notifications.pop_front();
        }

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);

        self.notifications.push_back(Notification {
            id,
            level,
            source_workspace,
            source_surface,
            title,
            body,
            timestamp: now,
            read: false,
        });
        Some(id)
    }

    #[cfg(test)]
    pub fn unread_count(&self) -> usize {
        self.unread_counts(None).total()
    }

    pub fn unread_counts(&self, workspace: Option<WorkspaceId>) -> UnreadCounts {
        let mut counts = UnreadCounts::default();
        for n in self
            .notifications
            .iter()
            .filter(|n| !n.read && workspace.is_none_or(|id| n.source_workspace == id))
        {
            match n.level {
                NotificationLevel::Error => counts.error += 1,
                NotificationLevel::Warning => counts.warning += 1,
                NotificationLevel::Info => counts.info += 1,
            }
        }
        counts
    }

    /// Updates channel state; the engine applies the existing attention clear policy.
    pub fn acknowledge(
        &mut self,
        id: Option<NotificationId>,
        workspace: Option<WorkspaceId>,
    ) -> usize {
        let mut changed = 0;
        for n in &mut self.notifications {
            if id.is_none_or(|id| n.id == id)
                && workspace.is_none_or(|id| n.source_workspace == id)
                && !n.read
            {
                n.read = true;
                changed += 1;
            }
        }
        changed
    }

    pub fn has_unread_for_surface(&self, surface_id: SurfaceId) -> bool {
        self.notifications
            .iter()
            .any(|n| !n.read && n.source_surface == surface_id)
    }

    /// 오래된 항목부터 반환한다.
    pub fn all(&self) -> impl DoubleEndedIterator<Item = &Notification> + ExactSizeIterator {
        self.notifications.iter()
    }

    pub fn mark_read(&mut self, id: NotificationId) {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.read = true;
        }
    }

    #[cfg(test)]
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
    fn shared_notification_ids_keep_creation_order_and_coalescing_identity() {
        let ids = crate::runtime::counters::RuntimeCounters::new();
        let mut a = NotificationStore::with_counter(60_000, ids.notification_counter());
        let mut b = NotificationStore::with_counter(60_000, ids.notification_counter());
        let first = a.add(1, 1, "A".into(), "one".into()).unwrap();
        let second = b.add(2, 2, "B".into(), "two".into()).unwrap();
        assert!(first < second);
        assert_eq!(a.add(1, 1, "updated".into(), "more".into()), None);
        assert_eq!(a.all().next().unwrap().id, first);
        let third = a.add(1, 3, "C".into(), "three".into()).unwrap();
        assert!(second < third);
        a.mark_read(first);
        assert!(!b.all().next().unwrap().read);
    }

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
        let mut store = NotificationStore::with_coalesce_ms(60000);
        store.add(1, 1, "Title".into(), "first".into());
        store.add(1, 1, "Title".into(), "second".into());
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
        for i in 0..110 {
            store.add(1, i as u32, format!("N{}", i), "".into());
        }
        assert_eq!(store.all().len(), 100);
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn levels_coalesce_independently_and_new_content_reopens_read_item() {
        let mut store = NotificationStore::with_coalesce_ms(60_000);
        let error = store
            .add_with_level(
                1,
                10,
                "Error".into(),
                "first".into(),
                NotificationLevel::Error,
            )
            .unwrap();
        store.add_with_level(
            1,
            10,
            "Warning".into(),
            String::new(),
            NotificationLevel::Warning,
        );
        store.add(1, 10, "Info".into(), String::new());
        assert_eq!(
            store.unread_counts(None),
            UnreadCounts {
                error: 1,
                warning: 1,
                info: 1
            }
        );
        assert_eq!(store.acknowledge(Some(error), None), 1);
        assert_eq!(
            store.add_with_level(
                1,
                10,
                "Error".into(),
                "second".into(),
                NotificationLevel::Error
            ),
            None
        );
        assert_eq!(store.all().len(), 3);
        assert_eq!(store.unread_counts(None).error, 1);
        let row = store.all().find(|n| n.id == error).unwrap();
        assert_eq!(row.body, "first\nsecond");
        assert!(!row.read);
    }

    #[test]
    fn counts_include_rows_outside_list_limit_and_follow_eviction_and_scope() {
        let mut store = NotificationStore::with_coalesce_ms(0);
        for i in 0..110 {
            store.add_with_level(
                if i < 60 { 1 } else { 2 },
                i,
                String::new(),
                String::new(),
                NotificationLevel::Warning,
            );
        }
        assert_eq!(store.unread_counts(None).total(), 100);
        assert_eq!(store.unread_counts(Some(1)).warning, 50);
        assert_eq!(store.acknowledge(None, Some(1)), 50);
        assert_eq!(store.unread_counts(None).warning, 50);
        assert_eq!(store.acknowledge(None, Some(1)), 0);
        let mut merged = store.unread_counts(None);
        merged.add(UnreadCounts {
            error: 2,
            warning: 0,
            info: 3,
        });
        assert_eq!(merged.total(), 55);
    }
}
