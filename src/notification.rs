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
    notifications: Vec<Notification>,
    max_count: usize,
    next_id: u64,
    /// Rate limiter: last time a system notification was sent.
    last_system_notification: Option<Instant>,
}

impl NotificationStore {
    pub fn new() -> Self {
        Self {
            notifications: Vec::new(),
            max_count: 100,
            next_id: 1,
            last_system_notification: None,
        }
    }

    /// Add a notification, coalescing if the same source sent one within 500ms.
    pub fn add(
        &mut self,
        source_workspace: WorkspaceId,
        source_surface: SurfaceId,
        title: String,
        body: String,
    ) {
        let now = Instant::now();
        let coalesce_window = Duration::from_millis(500);

        // Coalesce: if same source sent a notification recently, merge
        if let Some(existing) = self
            .notifications
            .iter_mut()
            .rev()
            .find(|n| {
                n.source_workspace == source_workspace
                    && n.source_surface == source_surface
                    && now.duration_since(n.timestamp) < coalesce_window
            })
        {
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
            return;
        }

        // FIFO eviction
        while self.notifications.len() >= self.max_count {
            self.notifications.remove(0);
        }

        let id = self.next_id;
        self.next_id += 1;

        self.notifications.push(Notification {
            id,
            source_workspace,
            source_surface,
            title,
            body,
            timestamp: now,
            read: false,
        });
    }

    /// Total unread notification count.
    pub fn unread_count(&self) -> usize {
        self.notifications.iter().filter(|n| !n.read).count()
    }

    /// Unread count for a specific workspace.
    pub fn unread_count_for_workspace(&self, workspace_id: WorkspaceId) -> usize {
        self.notifications
            .iter()
            .filter(|n| !n.read && n.source_workspace == workspace_id)
            .count()
    }

    /// Get all notifications (newest last).
    pub fn all(&self) -> &[Notification] {
        &self.notifications
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

    /// Check if we should send a system notification (rate limited to 1/sec).
    pub fn should_send_system_notification(&mut self) -> bool {
        let now = Instant::now();
        if let Some(last) = self.last_system_notification {
            if now.duration_since(last) < Duration::from_secs(1) {
                return false;
            }
        }
        self.last_system_notification = Some(now);
        true
    }

    /// Check if a surface has unread notifications.
    pub fn has_unread_for_surface(&self, surface_id: SurfaceId) -> bool {
        self.notifications
            .iter()
            .any(|n| !n.read && n.source_surface == surface_id)
    }
}

/// Send an OS-level desktop notification.
pub fn send_system_notification(title: &str, body: &str) {
    let _ = notify_rust::Notification::new()
        .summary(title)
        .body(body)
        .appname("Tasty")
        .show();
}
