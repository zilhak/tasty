//! Immutable native response data and entry scan publication; no filesystem or network access.
mod navigation;
mod resources;
mod response_wait;
pub use navigation::{FileNavigation, FileTarget};
pub use resources::{Fetch as NativeResourceFetch, NativeResources};
pub use response_wait::{ResourceFailure, ResponseWait};
use std::{fmt, io, sync::Arc};
use tasty_ipc::html_transfer::{HtmlSource, TransferId};
use tasty_model::html_script::{
    ScriptScan,
    scan::{SCAN_LIMIT_BYTES, scan_reader},
};
use url::Url;

use crate::html_document::Resource;

pub const ENTRY_SCHEME: &str = "tasty-html-entry";

/// The native adapter consumes these exact bytes. MIME/provenance/identity also apply to future resources.
pub struct NativeBodyResponse {
    generation: TransferId,
    source_generation: u64,
    mime: String,
    source_url: Url,
    bytes: Arc<Vec<u8>>,
}
impl NativeBodyResponse {
    pub fn new(
        generation: TransferId,
        source_generation: u64,
        mime: &str,
        source_url: &str,
        bytes: Arc<Vec<u8>>,
    ) -> io::Result<Self> {
        if mime.is_empty() || !mime.is_ascii() || mime.bytes().any(|b| b < 0x20 || b == 0x7f) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid response MIME",
            ));
        }
        let source_url =
            Url::parse(source_url).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        Ok(Self {
            generation,
            source_generation,
            mime: mime.to_owned(),
            source_url,
            bytes,
        })
    }
    pub fn generation(&self) -> TransferId {
        self.generation
    }
    pub fn source_generation(&self) -> u64 {
        self.source_generation
    }
    pub fn mime(&self) -> &str {
        &self.mime
    }
    /// Provenance for chrome and the script gate; never a client navigation or file-open instruction.
    pub fn source_url(&self) -> &str {
        self.source_url.as_str()
    }
    pub fn bytes(&self) -> &[u8] {
        self.bytes.as_slice()
    }
    pub fn shared_bytes(&self) -> Arc<Vec<u8>> {
        Arc::clone(&self.bytes)
    }
}
impl fmt::Debug for NativeBodyResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativeBodyResponse")
            .field("generation", &self.generation)
            .field("source_generation", &self.source_generation)
            .field("mime", &self.mime)
            .field("byte_len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub struct ReceivedEntry {
    response: Arc<NativeBodyResponse>,
    scan: ScriptScan,
    uri: String,
}
impl ReceivedEntry {
    /// Run on a scan worker after complete, before exposing any native response.
    pub fn from_complete(id: TransferId, resource: &Resource) -> io::Result<Option<Self>> {
        Self::from_complete_cancellable(id, resource, &std::sync::atomic::AtomicBool::new(false))
    }
    pub fn from_complete_cancellable(
        id: TransferId,
        resource: &Resource,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> io::Result<Option<Self>> {
        if id != resource.id() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "entry identity mismatch",
            ));
        }
        let Some(bytes) = resource.shared_body() else {
            return Ok(None);
        };
        let Some(metadata) = &resource.metadata else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "completed entry lacks metadata",
            ));
        };
        let HtmlSource::File { url, .. } = &metadata.source else {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "entry mirror requires a remote file",
            ));
        };
        let source = Url::parse(url).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if source.scheme() != "file" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "entry provenance is not a file URL",
            ));
        }
        let mime = if let Some(file) = &metadata.file {
            file.mime.as_str()
        } else if source.path().to_ascii_lowercase().ends_with(".svg") {
            "image/svg+xml"
        } else {
            "text/html"
        };
        let mut scan = scan_reader(
            CancellableBytes {
                bytes: bytes.as_slice(),
                cancel,
            },
            SCAN_LIMIT_BYTES,
        )?;
        if !matches!(
            mime.split(';').next().unwrap_or("").trim(),
            "text/html" | "image/svg+xml"
        ) {
            scan.detection = tasty_model::html_script::ScriptDetection::None;
        }
        let response = Arc::new(NativeBodyResponse::new(
            id,
            metadata.source_generation,
            mime,
            url,
            bytes,
        )?);
        // Separate virtual origins across connection, binding and document generations. The remaining
        // request/transfer identity is also part of the exact entry route. No remote path is navigated.
        let mut uri = format!(
            "{ENTRY_SCHEME}://e{}-s{}-b{}-d{}/r{}/t{}/index.html",
            id.epoch, id.surface_id, id.binding, id.document, id.request, id.transfer
        );
        if let Some(file) = &metadata.file {
            if file.relative.is_empty()
                || file.relative.len() > 16 * 1024
                || file.relative.contains('\\')
                || file.relative.chars().any(char::is_control)
                || file
                    .relative
                    .split('/')
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unsafe remote entry route",
                ));
            }
            let mut route = Url::parse(&uri).map_err(io::Error::other)?;
            route
                .path_segments_mut()
                .map_err(|_| io::Error::other("entry route is not hierarchical"))?
                .pop()
                .extend(file.relative.split('/'));
            route.set_query(source.query());
            route.set_fragment(source.fragment());
            uri = route.into();
        } else if let Some(fragment) = source.fragment() {
            uri.push('#');
            uri.push_str(fragment);
        }
        Ok(Some(Self {
            response,
            scan,
            uri,
        }))
    }
    pub fn response(&self) -> &Arc<NativeBodyResponse> {
        &self.response
    }
    pub fn scan(&self) -> ScriptScan {
        self.scan
    }
    pub fn native_uri(&self) -> &str {
        &self.uri
    }
}

/// A caller owns/synchronizes one store per SurfaceBinding. Scan workers publish only to their
/// captured six-part identity; native callbacks hold a weak owner and resolve only this exact entry.
#[derive(Default, Debug)]
pub struct EntryBodyStore {
    expected: Option<TransferId>,
    entry: Option<Arc<ReceivedEntry>>,
    resources: Option<NativeResources>,
    resources_enabled: bool,
    navigation: Option<FileNavigation>,
}
impl EntryBodyStore {
    pub fn begin(&mut self, id: TransferId) -> bool {
        if self.expected == Some(id) {
            return false;
        }
        self.expected = Some(id);
        self.entry = None;
        self.resources = None;
        self.navigation = None;
        true
    }
    pub fn expects(&self, id: TransferId) -> bool {
        self.expected == Some(id)
    }
    pub fn publish(&mut self, entry: Arc<ReceivedEntry>) -> bool {
        if self.expected != Some(entry.response.generation()) || self.entry.is_some() {
            return false;
        }
        self.resources = Some(NativeResources::new(
            Arc::clone(&entry),
            self.resources_enabled,
        ));
        self.entry = Some(entry);
        true
    }
    pub fn current(&self) -> Option<&Arc<ReceivedEntry>> {
        self.entry.as_ref().filter(|_| self.navigation.is_none())
    }
    pub fn entry_for_uri(&self, uri: &str) -> Option<Arc<ReceivedEntry>> {
        self.current()
            .filter(|e| without_fragment(e.native_uri()) == without_fragment(uri))
            .cloned()
    }
    pub fn response_for_uri(&self, uri: &str) -> Option<Arc<NativeBodyResponse>> {
        if self.navigation.is_some() {
            return None;
        }
        self.entry_for_uri(uri)
            .map(|entry| Arc::clone(entry.response()))
            .or_else(|| self.resources.as_ref()?.cached(uri))
    }
    /// Capability is chosen by the owning session before native context creation, not by a page.
    pub fn enable_resources(&mut self, enabled: bool) {
        self.resources_enabled = enabled;
        if let Some(resources) = &mut self.resources {
            resources.set_enabled(enabled);
        }
    }
    pub fn resources_enabled(&self) -> bool {
        self.resources_enabled
    }
    pub fn resources_mut(&mut self) -> Option<&mut NativeResources> {
        self.resources.as_mut()
    }
    pub fn request_resource(&mut self, uri: &str) -> ResponseWait {
        if self.navigation.is_some() {
            let (wait, target) = ResponseWait::pair();
            target.finish(Err(ResourceFailure::Cancelled));
            return wait;
        }
        if let Some(resources) = &mut self.resources {
            return resources.request(uri);
        }
        let (wait, target) = ResponseWait::pair();
        target.finish(Err(ResourceFailure::Denied));
        wait
    }
    pub fn cancel(&mut self, id: TransferId) -> bool {
        if !self.expects(id) {
            return false;
        }
        self.clear();
        true
    }
    /// A completed disconnected page remains available for a local script-allow reload. A pending
    /// scan loses publication rights; a reconnect must call begin with its new connection epoch.
    pub fn disconnect(&mut self) {
        if self.navigation.is_some() {
            self.clear();
            return;
        }
        if let Some(resources) = &mut self.resources {
            resources.disconnect();
        }
        self.expected = self.entry.as_ref().map(|entry| entry.response.generation());
    }
    pub fn clear(&mut self) {
        self.navigation = None;
        self.resources = None;
        self.expected = None;
        self.entry = None;
    }
}
struct CancellableBytes<'a> {
    bytes: &'a [u8],
    cancel: &'a std::sync::atomic::AtomicBool,
}
impl io::Read for CancellableBytes<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(std::sync::atomic::Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "entry scan cancelled",
            ));
        }
        io::Read::read(&mut self.bytes, out)
    }
}
fn without_fragment(uri: &str) -> &str {
    uri.split('#').next().unwrap_or(uri)
}

#[cfg(test)]
mod tests;
