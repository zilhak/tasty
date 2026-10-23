//! Bounded document roots persist after entry EOF, retaining original directory authorization.
use super::*;
use std::path::Path;

type Slot = Arc<Mutex<Option<Arc<Entry>>>>;
#[derive(Clone)]
pub(super) struct Lease {
    root: Slot,
    relative: Option<String>,
    query: Option<String>,
    fragment: Option<String>,
    initialize: bool,
    pub(super) cancel: Arc<AtomicBool>,
}
impl Lease {
    pub(super) fn entry() -> Self {
        Self {
            root: Arc::default(),
            relative: None,
            query: None,
            fragment: None,
            initialize: true,
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
        let root = match self.root()? {
            Some(root) => root,
            None if self.initialize => {
                let entry = Arc::new(Entry::open(Path::new(path))?);
                *self.root.lock().map_err(|error| {
                    tracing::error!(%error,"HTML authorized root poisoned during publication");
                    self.cancel.store(true, Ordering::Release);
                    "HTML root unavailable"
                })? = Some(Arc::clone(&entry));
                entry
            }
            None => return Err("HTML entry root not ready".into()),
        };
        if let Some(relative) = &self.relative {
            let entry = Arc::new(root.resource(relative)?);
            // Preserve the original server path namespace even when its parent is a safe alias.
            let resource_path = Path::new(path)
                .parent()
                .ok_or("HTML source has no parent")?
                .join(relative);
            let mut source = tasty_remote::html_source::classify(resource_path.to_str(), None);
            if let HtmlSource::File { url, .. } = &mut source {
                if let Some(query) = &self.query {
                    url.push('?');
                    url.push_str(query);
                }
                if let Some(fragment) = &self.fragment {
                    url.push('#');
                    url.push_str(fragment);
                }
            }
            Ok((entry, source))
        } else {
            Ok((root, source))
        }
    }
    pub(super) fn nested(&self) -> bool {
        self.relative.is_some()
    }
    pub(super) fn file_metadata(
        &self,
        path: &str,
    ) -> Result<tasty_ipc::html_transfer::HtmlFileMetadata, String> {
        let relative = match &self.relative {
            Some(relative) => relative.clone(),
            None => Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or("unrepresentable HTML entry name")?
                .to_owned(),
        };
        let extension = Path::new(&relative)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime = match extension.as_str() {
            "html" | "htm" => "text/html",
            "svg" => "image/svg+xml",
            "css" => "text/css",
            "js" | "mjs" => "text/javascript",
            "json" | "map" => "application/json",
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "avif" => "image/avif",
            "ico" => "image/x-icon",
            "woff" => "font/woff",
            "woff2" => "font/woff2",
            "ttf" => "font/ttf",
            "otf" => "font/otf",
            "txt" => "text/plain",
            "wasm" => "application/wasm",
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            "mp3" => "audio/mpeg",
            "ogg" => "audio/ogg",
            _ => "application/octet-stream",
        }
        .to_owned();
        Ok(tasty_ipc::html_transfer::HtmlFileMetadata { relative, mime })
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
        match request {
            HtmlRequest::HtmlResourceRequest {
                entry,
                source_generation,
                relative,
                query,
                ..
            } => {
                validate_target(relative, query, &None)?;
                let root = self.0.get(&key).ok_or("HTML document root unavailable")?;
                root.validate(
                    id,
                    *entry,
                    *source_generation,
                    Authority {
                        owner: &owner,
                        source: &source,
                        generation,
                    },
                    false,
                )?;
                let mut lease = root.lease.clone();
                lease.initialize = false;
                lease.relative = Some(relative.clone());
                lease.query = query.clone();
                lease.fragment = None;
                Ok(lease)
            }
            HtmlRequest::HtmlNavigateRequest {
                entry,
                source_generation,
                relative,
                query,
                fragment,
                ..
            } => {
                validate_target(relative, query, fragment)?;
                let root = self.0.get(&key).ok_or("HTML document root unavailable")?;
                root.validate(
                    id,
                    *entry,
                    *source_generation,
                    Authority {
                        owner: &owner,
                        source: &source,
                        generation,
                    },
                    true,
                )?;
                let mut lease = root.lease.clone();
                lease.initialize = false;
                lease.relative = Some(relative.clone());
                lease.query = query.clone();
                lease.fragment = fragment.clone();
                lease.cancel = Arc::new(AtomicBool::new(false));
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
            HtmlRequest::HtmlFileEntryRequest {
                relative,
                query,
                fragment,
                ..
            } => {
                validate_target(relative, query, fragment)?;
                let mut lease = Lease::entry();
                lease.relative = Some(relative.clone());
                lease.query = query.clone();
                lease.fragment = fragment.clone();
                self.install(key, id, generation, owner, source, lease)
            }
            HtmlRequest::HtmlEntryRequest { .. } => {
                self.install(key, id, generation, owner, source, Lease::entry())
            }
            _ => Err("HTML request is not a document/resource start".into()),
        }
    }
    fn install(
        &mut self,
        key: (u32, u32),
        id: TransferId,
        generation: u64,
        owner: TransferOwner,
        source: Weak<Mutex<tasty_remote::html_source::HtmlSourceState>>,
        lease: Lease,
    ) -> Result<Lease, String> {
        if !self.0.contains_key(&key) && self.0.len() >= 256 {
            return Err("HTML document root capacity".into());
        }
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
struct Authority<'a> {
    owner: &'a TransferOwner,
    source: &'a Weak<Mutex<tasty_remote::html_source::HtmlSourceState>>,
    generation: u64,
}
impl RootOwner {
    fn validate(
        &self,
        id: TransferId,
        entry: TransferId,
        source_generation: u64,
        authority: Authority<'_>,
        navigation: bool,
    ) -> Result<(), String> {
        let wrong_document = if navigation {
            id.document <= entry.document
        } else {
            id.document != entry.document
        };
        if self.id != entry
            || self.generation != source_generation
            || authority.generation != self.generation
            || !self.owner.same(authority.owner)
            || !self.source.ptr_eq(authority.source)
            || (id.epoch, id.surface_id, id.binding)
                != (entry.epoch, entry.surface_id, entry.binding)
            || wrong_document
            || self.lease.cancel.load(Ordering::Acquire)
        {
            return Err("stale HTML document resource".into());
        }
        Ok(())
    }
}
fn validate_target(
    relative: &str,
    query: &Option<String>,
    fragment: &Option<String>,
) -> Result<(), String> {
    if relative.len() > 16 * 1024
        || query.as_ref().is_some_and(|q| q.len() > 16 * 1024)
        || fragment.as_ref().is_some_and(|f| f.len() > 16 * 1024)
    {
        return Err("HTML resource target too long".into());
    }
    if query
        .as_ref()
        .is_some_and(|q| q.contains('#') || q.chars().any(char::is_control))
        || fragment
            .as_ref()
            .is_some_and(|f| f.chars().any(char::is_control))
    {
        return Err("invalid HTML resource URL suffix".into());
    }
    Ok(())
}
