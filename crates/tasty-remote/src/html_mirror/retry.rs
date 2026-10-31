//! Fresh requests for transfers the server expired while the user was still deciding.
use super::*;

impl HtmlMirror {
    /// Answered expired transfers awaiting a fresh identity, in the order they were requested.
    pub fn take_retries(&self) -> Vec<TransferId> {
        if self.id.is_none_or(|id| !self.current(id)) {
            return vec![];
        }
        self.document
            .as_ref()
            .map_or_else(Vec::new, HtmlDocument::retries)
    }
    /// The caller allocates `new` in actual send order. A retry that cannot be rebound ends as
    /// skipped, so an abandoned question never keeps the receiving page alive.
    pub fn bind_retry(&mut self, old: TransferId, new: TransferId) -> Option<HtmlRequest> {
        if !self.current(old) {
            return None;
        }
        let entry = self.id?;
        let request = if old == entry {
            Some(
                self.resources
                    .entry_request(new)
                    .unwrap_or(HtmlRequest::HtmlEntryRequest { id: new }),
            )
        } else {
            self.resource_retry(old, new, entry)
        };
        let retried = request.is_some()
            && self
                .document
                .as_mut()
                .is_some_and(|document| document.retry(old, new));
        let Some(request) = request.filter(|_| retried) else {
            if old != entry
                && let Some(resources) = self.body_store().resources_mut()
            {
                resources.fail(new, crate::html_body::ResourceFailure::Failed);
            }
            if let Some(document) = self.document.as_mut() {
                document.decline(old);
            }
            return None;
        };
        if old == entry {
            self.id = Some(new);
            self.body_store().begin(new);
        }
        Some(request)
    }
    fn resource_retry(
        &mut self,
        old: TransferId,
        new: TransferId,
        entry: TransferId,
    ) -> Option<HtmlRequest> {
        let source_generation = self.metadata.as_ref()?.source_generation;
        let (relative, query) = self.body_store().resources_mut()?.rebind(old, new)?;
        Some(HtmlRequest::HtmlResourceRequest {
            id: new,
            entry,
            source_generation,
            relative,
            query,
        })
    }
}
