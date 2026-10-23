//! File GET scheduling and completion; HTTP method/body identity belongs to its separate executor.
use super::*;
use crate::html_body::{
    FileNavigation, FileTarget, NativeBodyResponse, NativeResourceFetch, ResourceFailure,
};
use std::collections::HashSet;
use tasty_ipc::html_transfer::HtmlSource;

#[derive(Default)]
pub(super) struct ResourceDriver {
    target: Option<FileTarget>,
    navigation: Option<FileNavigation>,
    finished: HashSet<TransferId>,
    last_credit: u64,
}
impl ResourceDriver {
    pub(super) fn start(
        &mut self,
        id: TransferId,
        default: HtmlRequest,
        cancelled: &mut Vec<HtmlRequest>,
    ) -> HtmlRequest {
        self.finished.clear();
        self.last_credit = 0;
        if let Some(navigation) = self.navigation.take() {
            cancelled.retain(|request| request.id() != Some(navigation.entry));
            self.target = Some(navigation.target.clone());
            return HtmlRequest::HtmlNavigateRequest {
                id,
                entry: navigation.entry,
                source_generation: navigation.source_generation,
                relative: navigation.target.relative,
                query: navigation.target.query,
                fragment: navigation.target.fragment,
            };
        }
        match &self.target {
            Some(target) => HtmlRequest::HtmlFileEntryRequest {
                id,
                relative: target.relative.clone(),
                query: target.query.clone(),
                fragment: target.fragment.clone(),
            },
            None => default,
        }
    }
}
impl HtmlMirror {
    pub fn enable_resources(&mut self, enabled: bool) {
        self.body_store().enable_resources(enabled);
    }
    /// The native callback records main-document intent; this owner retires the document once.
    pub fn poll_navigation(&mut self) -> bool {
        let navigation = self.body_store().take_navigation();
        let Some(navigation) = navigation else {
            return false;
        };
        if !self.current(navigation.entry)
            || self
                .metadata
                .as_ref()
                .is_none_or(|m| m.source_generation != navigation.source_generation)
        {
            return false;
        }
        let Some(next) = self.generation.checked_add(1) else {
            return false;
        };
        self.cancel_scan();
        self.body_store().clear();
        self.generation = next;
        self.reload = true;
        self.resources.navigation = Some(navigation);
        true
    }
    pub fn take_resource_fetches(&mut self) -> Vec<NativeResourceFetch> {
        if self.id.is_none_or(|id| !self.current(id)) {
            return vec![];
        }
        let active = self.document.as_ref().map_or(0, |d| {
            d.resources
                .iter()
                .filter(|(id, r)| {
                    Some(**id) != self.id
                        && matches!(
                            r.state,
                            ReceiveState::Metadata
                                | ReceiveState::SizeNotice
                                | ReceiveState::Ready
                                | ReceiveState::Receiving
                        )
                })
                .count()
        });
        self.body_store()
            .resources_mut()
            .map_or_else(Vec::new, |r| r.take_pending(4usize.saturating_sub(active)))
    }
    pub fn bind_resource(
        &mut self,
        fetch: NativeResourceFetch,
        id: TransferId,
    ) -> Option<HtmlRequest> {
        if self.id != Some(fetch.entry) || !self.current(fetch.entry) {
            return None;
        }
        let request = HtmlRequest::HtmlResourceRequest {
            id,
            entry: fetch.entry,
            source_generation: fetch.source_generation,
            relative: fetch.relative,
            query: fetch.query,
        };
        {
            let mut store = self.body_store();
            let resources = store.resources_mut()?;
            if !resources.bind(fetch.ticket, id) {
                resources.reject(fetch.ticket, ResourceFailure::Denied);
                return None;
            }
        }
        if self.document.as_mut()?.request(id).is_none() {
            self.body_store()
                .resources_mut()?
                .fail(id, ResourceFailure::Denied);
            return None;
        }
        Some(request)
    }
    pub(super) fn next_credit(&mut self) -> Option<HtmlRequest> {
        let document = self.document.as_mut()?;
        let mut ready: Vec<_> = document
            .resources
            .iter()
            .filter(|(_, r)| r.state == ReceiveState::Ready)
            .map(|(id, _)| *id)
            .collect();
        ready.sort_by_key(|id| (id.transfer <= self.resources.last_credit, id.transfer));
        for id in ready {
            if let Some(credit) = document.credit(id) {
                self.resources.last_credit = id.transfer;
                return Some(credit);
            }
        }
        None
    }
    pub(super) fn validate_resource_metadata(&self, reply: HtmlReply) -> HtmlReply {
        let HtmlReply::HtmlMetadata { id, metadata } = &reply else {
            return reply;
        };
        if Some(*id) != self.id
            && self.metadata.as_ref().is_none_or(|entry| {
                entry.source_generation != metadata.source_generation
                    || !matches!(metadata.source, HtmlSource::File { .. })
                    || metadata.file.is_none()
            })
        {
            return HtmlReply::HtmlFailure {
                id: *id,
                reason: "stale or unsupported remote file metadata".into(),
            };
        }
        reply
    }
    /// Immutable completed allocations are handed directly to native waiters, without another read.
    pub fn poll_resources(&mut self) -> bool {
        if self.id.is_none_or(|id| !self.current(id)) {
            return false;
        }
        let abandoned = self
            .body_store()
            .resources_mut()
            .map_or_else(Vec::new, |r| r.abandoned());
        for id in abandoned {
            if let Some(cancel) = self.document.as_mut().and_then(|d| d.decline(id)) {
                self.pending.push(cancel);
            }
        }
        let terminal: Vec<_> = self
            .document
            .as_ref()
            .into_iter()
            .flat_map(|d| d.resources.iter())
            .filter(|(id, resource)| {
                Some(**id) != self.id
                    && !self.resources.finished.contains(id)
                    && matches!(
                        resource.state,
                        ReceiveState::Complete
                            | ReceiveState::Failed
                            | ReceiveState::Cancelled
                            | ReceiveState::Skipped
                    )
            })
            .map(|(id, resource)| (*id, resource.clone()))
            .collect();
        let mut changed = false;
        for (id, resource) in terminal {
            self.resources.finished.insert(id);
            let body = resource
                .metadata
                .as_ref()
                .zip(resource.shared_body())
                .and_then(|(metadata, body)| {
                    let HtmlSource::File { url, .. } = &metadata.source else {
                        return None;
                    };
                    match NativeBodyResponse::new(
                        id,
                        metadata.source_generation,
                        &metadata.file.as_ref()?.mime,
                        url,
                        body,
                    ) {
                        Ok(body) => Some(Arc::new(body)),
                        Err(error) => {
                            tracing::warn!(%error,"invalid remote resource response");
                            None
                        }
                    }
                });
            if let Some(resources) = self.body_store().resources_mut() {
                changed |= match body {
                    Some(body) => resources.publish(id, body),
                    None => resources.fail(id, ResourceFailure::Failed),
                };
            }
        }
        changed
    }
}
