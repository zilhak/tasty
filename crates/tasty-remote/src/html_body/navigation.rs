//! Navigation intents are captured in the native owner and consumed by the transport owner.
use super::*;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTarget {
    pub relative: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
}
#[derive(Debug, Clone)]
pub struct FileNavigation {
    pub entry: TransferId,
    pub source_generation: u64,
    pub target: FileTarget,
}
impl EntryBodyStore {
    pub fn navigation_pending(&self) -> bool {
        self.navigation.is_some()
    }
    pub fn supports_resource_uri(&self, uri: &str) -> bool {
        self.navigation.is_none()
            && self.resources_enabled
            && self.resources.as_ref().is_some_and(|resources| {
                resources.allows_navigation() && resources.route(uri).is_some()
            })
    }
    /// Called only for an actual main-document load. URI equality cannot identify a main frame.
    /// The native adapter stops that load before offering a child response and wakes its host.
    pub fn navigate(&mut self, uri: &str) -> bool {
        if !self.supports_resource_uri(uri) || self.entry_for_uri(uri).is_some() {
            return false;
        }
        let Some(entry) = self.entry.as_ref() else {
            return false;
        };
        let Some((_, relative, query)) = self.resources.as_ref().and_then(|r| r.route(uri)) else {
            return false;
        };
        self.navigation = Some(FileNavigation {
            entry: entry.response().generation(),
            source_generation: entry.response().source_generation(),
            target: FileTarget {
                relative,
                query,
                fragment: Url::parse(uri)
                    .ok()
                    .and_then(|url| url.fragment().map(str::to_owned)),
            },
        });
        self.stop_resources(ResourceFailure::Cancelled);
        true
    }
    pub fn take_navigation(&mut self) -> Option<FileNavigation> {
        let navigation = self.navigation.take()?;
        self.clear();
        Some(navigation)
    }
    pub fn stop_resources(&mut self, reason: ResourceFailure) {
        if let Some(resources) = &mut self.resources {
            resources.stop(reason);
        }
    }
}
