//! Bounded chrome metadata from replies accepted by the actual connection-owned model.
use super::*;
use tasty_ipc::{html_http::HttpReply, html_transfer::TransferId};
pub(crate) enum Observation {
    Head(TransferId, String, Option<u64>),
    Chunk(TransferId, u64),
}
impl Observation {
    pub(crate) fn capture(reply: &HttpReply) -> Option<Self> {
        match reply {
            HttpReply::Head { head } => Some(Self::Head(
                head.id.request,
                head.logical_url.clone(),
                head.encoded_length,
            )),
            HttpReply::Chunk {
                response,
                offset,
                data_b64,
            } => {
                let padding = data_b64
                    .as_bytes()
                    .iter()
                    .rev()
                    .take_while(|b| **b == b'=')
                    .count();
                let bytes = (data_b64.len() / 4 * 3).saturating_sub(padding);
                Some(Self::Chunk(
                    response.request,
                    offset.saturating_add(bytes as u64),
                ))
            }
            _ => None,
        }
    }
}
impl HttpSession {
    pub(crate) fn observe(&self, observation: Observation) {
        let Ok(mut notices) = self.notices.lock() else {
            self.fail();
            return;
        };
        match observation {
            Observation::Head(id, url, size) => {
                if notices.len() < 256 || notices.contains_key(&id) {
                    notices.insert(id, (url, size, 0));
                }
            }
            Observation::Chunk(id, offset) => {
                if let Some((_, _, received)) = notices.get_mut(&id) {
                    *received = (*received).max(offset);
                }
            }
        }
    }
    pub(crate) fn notice(&self, id: TransferId) -> Option<(String, u64)> {
        // reason: poisoned notice metadata cannot authorize Allow or additional body credit.
        let notices = self.notices.lock().ok()?;
        let (url, size, received) = notices.get(&id)?;
        Some((url.clone(), size.unwrap_or(*received)))
    }
    pub(crate) fn responses(&self) -> usize {
        self.notices.lock().map_or(0, |notices| notices.len())
    }
    pub(crate) fn failed(&self) -> bool {
        self.failed.load(Ordering::Acquire)
    }
    pub(crate) fn fail(&self) {
        self.failed.store(true, Ordering::Release);
        self.close();
    }
}
