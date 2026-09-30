//! Headless command resolution and execution context.

use super::*;

/// Headless command defaults and application work queues. This is not a View;
/// its target choices exist solely for compatible omitted-target resolution.
pub struct CommandContext {
    pub(crate) navigation: navigation::NavigationState,
    #[cfg(any(debug_assertions, test))]
    pub(crate) category_last_active: std::collections::HashMap<u32, u32>,
    #[cfg(test)]
    pub(crate) tab_bar_height: PhysicalPx,
    pub(crate) recent_files: crate::recent_files::RecentFiles,
    pub(crate) memory: std::sync::Arc<std::sync::Mutex<dyn tasty_memory::MemoryStorage>>,
    pub(crate) pending_lifecycle_events: Vec<PendingSurfaceClosed>,
    pub(crate) pending_host_events: Vec<PendingHostEvent>,
    pub(crate) pending_intents: Vec<crate::intent::DispatchedIntent>,
}
