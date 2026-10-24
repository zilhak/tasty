//! Actual engine-owned HTTP admission/publication. Network setup never runs on the engine loop.
mod authority;
mod setup;
#[cfg(test)]
mod tests;

use super::{html_http as core, html_http_server as bridge};
use crate::runtime::engine_access::{EngineMut, EngineRef};
use authority::Authority;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak, atomic::AtomicBool, mpsc},
    time::Duration,
};
use tasty_ipc::{
    html_http::{self as wire, HttpRequest},
    html_transfer::TransferId,
    stream_hub::{PublicationFence, PushResult, StreamHub},
};

type Key = (u32, u64, u32, u64, u64);
fn key(client: u32, id: TransferId) -> Key {
    (client, id.epoch, id.surface_id, id.binding, id.document)
}
struct Document {
    authority: Authority,
    token: Option<bridge::OwnerToken>,
    initial: Option<HttpRequest>,
    requests: HashMap<TransferId, [u8; 32]>,
    fence: PublicationFence,
    transferred: u64,
    stopped: bool,
}
#[derive(Default)]
pub(crate) struct HttpHost {
    documents: HashMap<Key, Document>,
    setup: Option<setup::Setup>,
    backend: Option<core::Backend>,
    server: Option<bridge::Server>,
}
impl HttpHost {
    fn reject(
        hub: &StreamHub,
        client: u32,
        binding: &Weak<()>,
        request: &HttpRequest,
        error: &core::Error,
    ) {
        let Some(reservation) =
            hub.reserve_frame(client, binding, tasty_ipc::admission::QUEUED_BYTES_LIMIT)
        else {
            hub.unregister_bound(client, binding);
            return;
        };
        Self::publish_rejection(hub, client, binding, reservation, request, error);
    }
    fn reject_current(
        hub: &StreamHub,
        client: u32,
        document: &Document,
        request: &HttpRequest,
        error: &core::Error,
    ) {
        let binding = &document.authority.binding;
        let Some(reservation) =
            hub.reserve_frame(client, binding, tasty_ipc::admission::QUEUED_BYTES_LIMIT)
        else {
            hub.unregister_bound(client, binding);
            return;
        };
        Self::publish_rejection(
            hub,
            client,
            binding,
            reservation
                .for_owner(&document.fence)
                .with_guard(document.authority.publication.clone()),
            request,
            error,
        );
    }
    fn publish_rejection(
        hub: &StreamHub,
        client: u32,
        binding: &Weak<()>,
        reservation: tasty_ipc::stream_hub::FrameReservation,
        request: &HttpRequest,
        error: &core::Error,
    ) {
        match bridge::rejection_reply(request, error).frame() {
            Ok(frame) => {
                if hub.push_reserved(reservation, frame) != PushResult::Sent {
                    hub.unregister_bound(client, binding);
                }
            }
            Err(_) => {
                hub.unregister_bound(client, binding);
            }
        }
    }
    fn retire(&mut self, key: Key) {
        if let Some(document) = self.documents.remove(&key) {
            // Drop the publication fence before any old queued response can be written.
            drop(document.fence);
            if let (Some(server), Some(token)) = (&mut self.server, document.token) {
                server.retire(&token);
            }
        }
    }
    fn start(&mut self, wake: Arc<dyn Fn() + Send + Sync>) -> core::Result<()> {
        if self.setup.is_none() {
            self.setup = Some(setup::Setup::start(wake)?);
        }
        Ok(())
    }
    fn ready(&mut self, engine: &EngineRef<'_>, hub: &StreamHub) {
        if self.server.is_some() {
            return;
        }
        let Some(result) = self.setup.as_ref().and_then(setup::Setup::poll) else {
            return;
        };
        let result = result.and_then(|backend| {
            let server = bridge::Server::new(backend.clone(), 8, &setup::limits())?;
            self.backend = Some(backend);
            Ok(server)
        });
        match result {
            Ok(server) => self.server = Some(server),
            Err(error) => {
                for (key, mut document) in std::mem::take(&mut self.documents) {
                    if document.authority.current(engine, hub, key.0)
                        && let Some(request) = document.initial.take()
                    {
                        Self::reject(hub, key.0, &document.authority.binding, &request, &error);
                    }
                }
                drop(self.setup.take());
            }
        }
    }
    fn handle(
        &mut self,
        engine: &EngineRef<'_>,
        hub: &StreamHub,
        client: u32,
        request: HttpRequest,
    ) {
        self.collect(engine, hub);
        let Some(binding) = hub.client_binding(client) else {
            return;
        };
        let id = request.id();
        let key = key(client, id);
        if let Some(document) = self.documents.get(&key)
            && !document.authority.current(engine, hub, client)
        {
            self.retire(key);
        }
        if request.validate().is_err() {
            Self::reject(
                hub,
                client,
                &binding,
                &request,
                &core::Error::Invalid("HTTP command"),
            );
            return;
        }
        let new_document = !self.documents.contains_key(&key);
        if matches!(request, HttpRequest::Begin { .. }) {
            if let Err(error) = self.begin(engine, hub, client, &binding, &request) {
                if let Some(document) = self.documents.get(&key) {
                    Self::reject_current(hub, client, document, &request, &error);
                } else {
                    Self::reject(hub, client, &binding, &request, &error);
                }
                return;
            }
            if new_document {
                self.collect(engine, hub);
                return;
            }
        }
        let Some(document) = self.documents.get_mut(&key) else {
            Self::reject(hub, client, &binding, &request, &core::Error::Stale);
            return;
        };
        if !document.authority.current(engine, hub, client) {
            self.retire(key);
            return;
        }
        if document.authority.generation != request.source_generation() {
            Self::reject_current(hub, client, document, &request, &core::Error::Stale);
            return;
        }
        let (Some(server), Some(token)) = (&mut self.server, &document.token) else {
            // Only the original Begin can wait for setup. No upload window has been issued yet.
            if document.initial.as_ref() != Some(&request) {
                Self::reject_current(hub, client, document, &request, &core::Error::InProgress);
            }
            return;
        };
        match server.handle(
            token,
            |_| document.authority.current(engine, hub, client),
            request.clone(),
        ) {
            Err(error) => Self::reject_current(hub, client, document, &request, &error),
            Ok(()) => {
                if matches!(request, HttpRequest::Stop { .. }) {
                    document.stopped = true;
                }
                if matches!(request, HttpRequest::Close { .. }) {
                    self.retire(key);
                }
            }
        }
    }
    fn begin(
        &mut self,
        engine: &EngineRef<'_>,
        hub: &StreamHub,
        client: u32,
        binding: &Weak<()>,
        request: &HttpRequest,
    ) -> core::Result<()> {
        let id = request.id();
        let key = key(client, id);
        let fingerprint = bridge::begin_fingerprint(request, wire::HEADER_BYTES)?;
        if let Some(document) = self.documents.get(&key) {
            document.authority.permits(request, hub, client)?;
            if let Some(prior) = document.requests.get(&id) {
                if self.server.is_none() {
                    return Err(if prior == &fingerprint {
                        core::Error::InProgress
                    } else {
                        core::Error::DuplicateMismatch
                    });
                }
                // Existing request classification belongs to Server; do not claim either cursor again.
                return Ok(());
            }
            if document.stopped {
                return Err(core::Error::Closed);
            }
            if document.token.is_none() {
                return Err(core::Error::Capacity);
            }
            if document.requests.len() >= setup::limits().requests_per_document {
                return Err(core::Error::Capacity);
            }
        } else {
            let authority = Authority::capture(engine, hub, client, request)?;
            authority.permits(request, hub, client)?;
            let old: Vec<_> = self
                .documents
                .keys()
                .copied()
                .filter(|k| k.0 == client && k.2 == id.surface_id && k.3 == id.binding)
                .collect();
            if old.iter().any(|k| k.4 >= id.document) {
                return Err(core::Error::Stale);
            }
            if self.documents.len() - old.len() >= 8 {
                return Err(core::Error::Capacity);
            }
            self.start(engine.runtime.waker.clone())?;
            if !hub.admit_html_request(client, binding, id) {
                return Err(core::Error::Stale);
            }
            // A rejected replacement must leave the accepted document and POST alive.
            for old in old {
                self.retire(old);
            }
            self.documents.insert(
                key,
                Document {
                    authority,
                    token: None,
                    initial: Some(request.clone()),
                    requests: HashMap::from([(id, fingerprint)]),
                    fence: PublicationFence::default(),
                    transferred: 0,
                    stopped: false,
                },
            );
            return Ok(());
        }
        if !hub.admit_html_request(client, binding, id) {
            return Err(core::Error::Stale);
        }
        self.documents
            .get_mut(&key)
            .ok_or(core::Error::Stale)?
            .requests
            .insert(id, fingerprint);
        Ok(())
    }
    fn collect(&mut self, engine: &EngineRef<'_>, hub: &StreamHub) {
        self.ready(engine, hub);
        for key in self.documents.keys().copied().collect::<Vec<_>>() {
            let current = self
                .documents
                .get(&key)
                .is_some_and(|d| d.authority.current(engine, hub, key.0));
            if !current {
                self.retire(key);
                continue;
            }
            let Some(server) = &mut self.server else {
                continue;
            };
            let Some(document) = self.documents.get_mut(&key) else {
                continue;
            };
            if document.token.is_none() {
                let Some(request) = document.initial.take() else {
                    self.retire(key);
                    continue;
                };
                let result = server
                    .admit(document.authority.bridge_capture())
                    .and_then(|token| {
                        if let Err(error) = server.handle(
                            &token,
                            |_| document.authority.current(engine, hub, key.0),
                            request.clone(),
                        ) {
                            server.retire(&token);
                            return Err(error);
                        }
                        Ok(token)
                    });
                match result {
                    Ok(token) => document.token = Some(token),
                    Err(error) => {
                        Self::reject(hub, key.0, &document.authority.binding, &request, &error);
                        self.retire(key);
                        continue;
                    }
                }
            }
            let Some(token) = document.token.as_ref() else {
                self.retire(key);
                continue;
            };
            let Some(reservation) = hub.reserve_frame(
                key.0,
                &document.authority.binding,
                tasty_ipc::admission::QUEUED_BYTES_LIMIT,
            ) else {
                continue;
            };
            let capacity = reservation.capacity();
            let reservation = reservation
                .for_owner(&document.fence)
                .with_guard(document.authority.publication.clone());
            match server.poll(
                token,
                |_| document.authority.current(engine, hub, key.0),
                capacity,
            ) {
                Ok(Some(frame)) if document.authority.current(engine, hub, key.0) => {
                    if hub.push_reserved(reservation, frame) != PushResult::Sent {
                        self.retire(key);
                    } else {
                        (engine.runtime.waker)();
                    }
                }
                Ok(_) => {}
                Err(_) => {
                    self.retire(key);
                }
            }
            if let Some(document) = self.documents.get_mut(&key)
                && let Some(server) = &self.server
                && let Some(token) = &document.token
            {
                match server.transferred_bytes(token) {
                    Ok(total) if total >= document.transferred => document.transferred = total,
                    _ => self.retire(key),
                }
            }
        }
    }
}
impl Drop for HttpHost {
    fn drop(&mut self) {
        for key in self.documents.keys().copied().collect::<Vec<_>>() {
            self.retire(key);
        }
        drop(self.server.take());
        if let Some(backend) = self.backend.take() {
            tracing::debug!(
                workers = backend.available_workers(),
                retained = backend.retained_payload_bytes(),
                "HTTP host runtime retiring"
            );
        }
        drop(self.setup.take());
    }
}
impl EngineMut<'_> {
    pub(crate) fn handle_http_request(
        &mut self,
        hub: &StreamHub,
        client: u32,
        request: HttpRequest,
    ) {
        let mut host = std::mem::take(&mut self.remote.html_http);
        host.handle(&self.as_ref(), hub, client, request);
        self.remote.html_http = host;
    }
    pub(crate) fn poll_http_transfers(&mut self, hub: &StreamHub) {
        let mut host = std::mem::take(&mut self.remote.html_http);
        host.collect(&self.as_ref(), hub);
        self.remote.html_http = host;
    }
}
