//! Metadata-derived logical source and native-view fences require no mirror/model mutex at use.
use super::*;
use std::sync::atomic::AtomicU64;
use tasty_ipc::html_transfer::HtmlSource;
struct Source {
    metadata: TransferId,
    generation: u64,
    url: String,
    address: String,
    connection: ConnectionEpoch,
    live: AtomicBool,
    view: AtomicU64,
}
/// A source observed through this exact mirror's admitted metadata. No native/HTTP request ID
/// is allocated here; the first browser request allocates it only at the actual ordered send.
#[derive(Clone)]
pub struct HttpSource(Arc<Source>);
impl HttpSource {
    pub fn metadata_id(&self) -> TransferId {
        self.0.metadata
    }
    pub fn source_generation(&self) -> u64 {
        self.0.generation
    }
    /// Network identity without fragment. The native address retains its original fragment.
    pub fn entry_url(&self) -> &str {
        &self.0.url
    }
    pub fn address(&self) -> &str {
        &self.0.address
    }
    pub fn is_current(&self) -> bool {
        self.0.live.load(Ordering::Acquire) && self.0.connection.is_active()
    }
    fn retire(&self) {
        self.0.live.store(false, Ordering::Release);
    }
    /// Recreating a widget replaces only its view fence, preserving the logical document/body.
    /// Call only for an actual new native document/context, before generating its fresh TLS map.
    pub fn new_view(&self) -> Option<HttpView> {
        if !self.is_current() {
            return None;
        }
        let serial = self
            .0
            .view
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |n| n.checked_add(1))
            .ok()?
            .checked_add(1)?;
        if !self.is_current() {
            return None;
        }
        Some(HttpView(Arc::new(View {
            source: self.clone(),
            serial,
        })))
    }
}
struct View {
    source: HttpSource,
    serial: u64,
}
impl View {
    fn retire(&self) {
        let Some(next) = self.serial.checked_add(1) else {
            self.source.retire();
            return;
        };
        if self
            .source
            .0
            .view
            .compare_exchange(self.serial, next, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            tracing::debug!("replaced HTTP native view already retired");
        }
    }
}
impl Drop for View {
    fn drop(&mut self) {
        self.retire();
    }
}
#[derive(Clone)]
pub struct HttpView(Arc<View>);
impl HttpView {
    pub fn is_current(&self) -> bool {
        self.0.source.is_current() && self.0.source.0.view.load(Ordering::Acquire) == self.0.serial
    }
    pub fn source(&self) -> &HttpSource {
        &self.0.source
    }
    pub fn retire(&self) {
        self.0.retire();
    }
}
impl HtmlMirror {
    pub(super) fn retire_http_source(&mut self) {
        if let Some(source) = self.http_source.take() {
            source.retire();
        }
    }
    fn current_http_metadata(&self) -> Option<(TransferId, u64, String, String)> {
        let id = self.id?;
        if !self.current(id) {
            return None;
        }
        let metadata = self.metadata.as_ref()?;
        let HtmlSource::Http { url } = &metadata.source else {
            return None;
        };
        if metadata.source_generation == 0 || url.len() > tasty_ipc::html_http::URL_BYTES {
            return None;
        }
        let address = url.clone();
        let mut url = url::Url::parse(url).ok()?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return None;
        }
        url.set_fragment(None);
        Some((id, metadata.source_generation, url.into(), address))
    }
    pub(super) fn invalidate_http_source(&mut self) {
        if let Some(source) = &self.http_source {
            let matches =
                self.current_http_metadata()
                    .is_some_and(|(id, generation, url, address)| {
                        id == source.0.metadata
                            && generation == source.0.generation
                            && url == source.0.url
                            && address == source.0.address
                    });
            if !matches {
                self.retire_http_source();
            }
        }
    }
    /// The integrating native factory separately requires negotiated HTTP/HTTPS, HTML role and
    /// the real surface/connection binding. Metadata alone never activates a native viewer.
    pub fn http_source(&mut self) -> Option<HttpSource> {
        self.invalidate_http_source();
        if let Some(source) = &self.http_source {
            return source.is_current().then(|| source.clone());
        }
        let (metadata, generation, url, address) = self.current_http_metadata()?;
        let source = HttpSource(Arc::new(Source {
            metadata,
            generation,
            url,
            address,
            connection: self.connection.clone(),
            live: AtomicBool::new(true),
            view: AtomicU64::new(0),
        }));
        self.http_source = Some(source.clone());
        Some(source)
    }
}
#[cfg(test)]
mod tests;
