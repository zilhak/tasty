//! Completed-response cache and bounded native request scheduling for one immutable entry owner.
use super::response_wait::{ResourceFailure, ResponseTarget, ResponseWait};
use crate::html_body::{NativeBodyResponse, ReceivedEntry};
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};
use tasty_ipc::html_transfer::TransferId;
use url::Url;

pub struct Fetch {
    pub ticket: u64,
    pub entry: TransferId,
    pub source_generation: u64,
    pub relative: String,
    pub query: Option<String>,
}
enum State {
    Queued,
    Active(TransferId),
    Ready(Arc<NativeBodyResponse>),
    Failed(ResourceFailure),
}
struct Slot {
    ticket: u64,
    relative: String,
    query: Option<String>,
    state: State,
    id: Option<TransferId>,
    waiters: Vec<ResponseTarget>,
}
pub struct NativeResources {
    entry: Arc<ReceivedEntry>,
    enabled: bool,
    connected: bool,
    sequence: u64,
    slots: HashMap<String, Slot>,
    pending: VecDeque<String>,
}
impl NativeResources {
    pub fn new(entry: Arc<ReceivedEntry>, enabled: bool) -> Self {
        Self {
            entry,
            enabled,
            connected: true,
            sequence: 0,
            slots: HashMap::new(),
            pending: VecDeque::new(),
        }
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }
    pub fn cached(&self, uri: &str) -> Option<Arc<NativeBodyResponse>> {
        let (key, _, _) = self.route(uri)?;
        match &self.slots.get(&key)?.state {
            State::Ready(body) => Some(Arc::clone(body)),
            _ => None,
        }
    }
    pub fn reject(&mut self, ticket: u64, reason: ResourceFailure) {
        if let Some(slot) = self.slots.values_mut().find(|slot| slot.ticket == ticket) {
            for target in slot.waiters.drain(..) {
                target.finish(Err(reason));
            }
            slot.state = State::Failed(reason);
        }
    }
    pub fn request(&mut self, uri: &str) -> ResponseWait {
        let (wait, target) = ResponseWait::pair();
        let Some((key, relative, query)) = self.route(uri) else {
            target.finish(Err(ResourceFailure::Denied));
            return wait;
        };
        if let Some(slot) = self.slots.get_mut(&key) {
            slot.waiters.retain(ResponseTarget::live);
            match &slot.state {
                State::Ready(body) => target.finish(Ok(Arc::clone(body))),
                State::Failed(reason) => target.finish(Err(*reason)),
                _ if slot.waiters.len() >= 32 => target.finish(Err(ResourceFailure::Denied)),
                _ => slot.waiters.push(target),
            }
            return wait;
        }
        if !self.enabled || !self.connected || self.slots.len() >= 256 {
            target.finish(Err(ResourceFailure::Denied));
            return wait;
        }
        let Some(ticket) = self.sequence.checked_add(1) else {
            target.finish(Err(ResourceFailure::Denied));
            return wait;
        };
        self.sequence = ticket;
        self.pending.push_back(key.clone());
        self.slots.insert(
            key,
            Slot {
                ticket,
                relative,
                query,
                state: State::Queued,
                id: None,
                waiters: vec![target],
            },
        );
        wait
    }
    fn route(&self, uri: &str) -> Option<(String, String, Option<String>)> {
        if uri.len() > 16 * 1024 {
            return None;
        }
        let mut url = Url::parse(uri).ok()?;
        let entry = Url::parse(self.entry.native_uri()).ok()?;
        if url.scheme() != entry.scheme()
            || url.host_str() != entry.host_str()
            || url.port() != entry.port()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return None;
        }
        let id = self.entry.response().generation();
        let prefix = format!("/r{}/t{}/", id.request, id.transfer);
        let relative = decode_path(url.path().strip_prefix(&prefix)?)?;
        let query = url.query().map(str::to_owned);
        url.set_fragment(None);
        Some((url.to_string(), relative, query))
    }
    pub fn take_pending(&mut self, max: usize) -> Vec<Fetch> {
        let mut result = Vec::new();
        if !self.connected {
            return result;
        }
        while result.len() < max {
            let Some(key) = self.pending.pop_front() else {
                break;
            };
            let Some(slot) = self.slots.get_mut(&key) else {
                continue;
            };
            if !matches!(slot.state, State::Queued) {
                continue;
            }
            slot.waiters.retain(ResponseTarget::live);
            if slot.waiters.is_empty() {
                slot.state = State::Failed(ResourceFailure::Cancelled);
                continue;
            }
            result.push(Fetch {
                ticket: slot.ticket,
                entry: self.entry.response().generation(),
                source_generation: self.entry.response().source_generation(),
                relative: slot.relative.clone(),
                query: slot.query.clone(),
            });
        }
        result
    }
    pub fn bind(&mut self, ticket: u64, id: TransferId) -> bool {
        let entry = self.entry.response().generation();
        if !self.connected
            || id.transfer <= entry.transfer
            || self.slots.values().any(|slot| slot.id == Some(id))
            || (id.epoch, id.surface_id, id.binding, id.document)
                != (entry.epoch, entry.surface_id, entry.binding, entry.document)
        {
            return false;
        }
        let Some(slot) = self.slots.values_mut().find(|slot| slot.ticket == ticket) else {
            return false;
        };
        if !matches!(slot.state, State::Queued) {
            return false;
        }
        slot.id = Some(id);
        slot.state = State::Active(id);
        true
    }
    pub fn publish(&mut self, id: TransferId, body: Arc<NativeBodyResponse>) -> bool {
        if !self.connected {
            return false;
        }
        if body.generation() != id
            || body.source_generation() != self.entry.response().source_generation()
        {
            self.fail(id, ResourceFailure::Denied);
            return false;
        }
        let Some(slot) = self
            .slots
            .values_mut()
            .find(|slot| matches!(slot.state,State::Active(current) if current==id))
        else {
            return false;
        };
        for target in slot.waiters.drain(..) {
            target.finish(Ok(Arc::clone(&body)));
        }
        slot.state = State::Ready(body);
        true
    }
    pub fn fail(&mut self, id: TransferId, reason: ResourceFailure) -> bool {
        let Some(slot) = self
            .slots
            .values_mut()
            .find(|slot| matches!(slot.state,State::Active(current) if current==id))
        else {
            return false;
        };
        for target in slot.waiters.drain(..) {
            target.finish(Err(reason));
        }
        slot.state = State::Failed(reason);
        true
    }
    pub fn abandoned(&mut self) -> Vec<TransferId> {
        let mut cancel = Vec::new();
        for slot in self.slots.values_mut() {
            slot.waiters.retain(ResponseTarget::live);
            if slot.waiters.is_empty()
                && let State::Active(id) = slot.state
            {
                cancel.push(id);
                slot.state = State::Failed(ResourceFailure::Cancelled);
            }
        }
        cancel
    }
    pub fn disconnect(&mut self) {
        self.connected = false;
        self.pending.clear();
        for slot in self.slots.values_mut() {
            if matches!(slot.state, State::Ready(_)) {
                continue;
            }
            for target in slot.waiters.drain(..) {
                target.finish(Err(ResourceFailure::Disconnected));
            }
            slot.state = State::Failed(ResourceFailure::Disconnected);
        }
    }
}
fn decode_path(raw: &str) -> Option<String> {
    let mut bytes = Vec::new();
    let mut chars = raw.as_bytes().iter().copied();
    while let Some(byte) = chars.next() {
        let value = if byte == b'%' {
            let high = char::from(chars.next()?).to_digit(16)?;
            let low = char::from(chars.next()?).to_digit(16)?;
            let decoded = (high * 16 + low) as u8;
            if matches!(decoded, b'/' | b'\\') {
                return None;
            }
            decoded
        } else {
            byte
        };
        if value < 0x20 || value == 0x7f || value == b'\\' {
            return None;
        }
        bytes.push(value);
    }
    let path = String::from_utf8(bytes).ok()?;
    if path
        .split('/')
        .any(|part| part.is_empty() || matches!(part, "." | ".."))
    {
        return None;
    }
    Some(path)
}

impl std::fmt::Debug for NativeResources {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativeResources")
            .field("enabled", &self.enabled)
            .field("connected", &self.connected)
            .field("responses", &self.slots.len())
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}
#[cfg(test)]
mod tests;
