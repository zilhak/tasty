//! Bounded document roots persist after entry EOF, retaining original directory authorization.
use super::*;
use std::path::Path;

type Slot = Arc<Mutex<Option<Arc<Entry>>>>;
#[derive(Clone)]
pub(super) struct Lease {
    root: Slot,
    relative: Option<String>,
    pub(super) cancel: Arc<AtomicBool>,
}
impl Lease {
    pub(super) fn entry() -> Self {
        Self {
            root: Arc::default(),
            relative: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }
    #[cfg(test)]
    pub(super) fn set_test_relative(&mut self, relative: &str) {
        self.relative = Some(relative.into());
    }
    fn root(&self) -> Result<Option<Arc<Entry>>, String> {
        self.root.lock().map(|root| root.clone()).map_err(|error| {
            tracing::error!(%error, "HTML authorized root poisoned; refusing resources");
            self.cancel.store(true, Ordering::Release);
            "HTML root unavailable".into()
        })
    }
    pub(super) fn open(
        &self,
        path: &str,
        source: HtmlSource,
    ) -> Result<(Arc<Entry>, HtmlSource), String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("HTML document cancelled".into());
        }
        if let Some(relative) = &self.relative {
            let root = self.root()?.ok_or("HTML entry root not ready")?;
            let entry = Arc::new(root.resource(relative)?);
            let resource_path = entry.root.join(relative);
            let source = tasty_remote::html_source::classify(resource_path.to_str(), None);
            Ok((entry, source))
        } else {
            let entry = Arc::new(Entry::open(Path::new(path))?);
            *self.root.lock().map_err(|error| {
                tracing::error!(%error, "HTML authorized root poisoned during publication");
                "HTML root unavailable"
            })? = Some(Arc::clone(&entry));
            Ok((entry, source))
        }
    }
    pub(super) fn unchanged(&self) -> bool {
        !self.cancel.load(Ordering::Acquire)
            && self
                .root()
                .ok()
                .flatten()
                .is_some_and(|entry| entry.unchanged())
    }
}
struct RootOwner {
    id: TransferId,
    generation: u64,
    owner: TransferOwner,
    source: Weak<Mutex<tasty_remote::html_source::HtmlSourceState>>,
    lease: Lease,
}
impl Drop for RootOwner {
    fn drop(&mut self) {
        self.lease.cancel.store(true, Ordering::Release);
    }
}
#[derive(Default)]
pub(super) struct Roots(HashMap<(u32, u32), RootOwner>);
impl Roots {
    pub(super) fn cancel_entry(&mut self, client: u32, id: TransferId) {
        let key = (client, id.surface_id);
        if self.0.get(&key).is_some_and(|root| root.id == id) {
            self.0.remove(&key);
        }
    }
    pub(super) fn collect(&mut self, engine: &EngineRef<'_>, hub: &StreamHub) {
        self.0.retain(|(client, _), root| {
            root.owner.current(engine, hub, *client)
                && source_current(&root.source, root.generation)
        });
    }
    pub(super) fn lease(
        &mut self,
        client: u32,
        id: TransferId,
        request: &HtmlRequest,
        owner: TransferOwner,
        source: Weak<Mutex<tasty_remote::html_source::HtmlSourceState>>,
        generation: u64,
    ) -> Result<Lease, String> {
        let key = (client, id.surface_id);
        if let HtmlRequest::HtmlResourceRequest {
            entry,
            source_generation,
            relative,
            ..
        } = request
        {
            let root = self.0.get(&key).ok_or("HTML document root unavailable")?;
            if root.id != *entry
                || root.generation != *source_generation
                || generation != root.generation
                || !root.owner.same(&owner)
                || !root.source.ptr_eq(&source)
                || (id.epoch, id.surface_id, id.binding, id.document)
                    != (entry.epoch, entry.surface_id, entry.binding, entry.document)
                || root.lease.cancel.load(Ordering::Acquire)
            {
                return Err("stale HTML document resource".into());
            }
            if relative.len() > 16 * 1024 {
                return Err("HTML resource path too long".into());
            }
            let mut lease = root.lease.clone();
            lease.relative = Some(relative.clone());
            return Ok(lease);
        }
        if !self.0.contains_key(&key) && self.0.len() >= 256 {
            return Err("HTML document root capacity".into());
        }
        let lease = Lease::entry();
        self.0.insert(
            key,
            RootOwner {
                id,
                generation,
                owner,
                source,
                lease: lease.clone(),
            },
        );
        Ok(lease)
    }
}
