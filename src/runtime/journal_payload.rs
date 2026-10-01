use tasty_core::canonical::{ResolveData,CanonData,fnv_hex};
use tasty_core::DataRef;
use tasty_event_store::{EventStore,PayloadRef};
/// 저장소의 payload 바이트를 그대로 해시한다.
pub(crate) struct StoreBytes<'a>(pub(crate) &'a EventStore);

impl ResolveData for StoreBytes<'_> {
    fn resolve(&self, data: DataRef) -> CanonData {
        match self.0.read_payload(PayloadRef(data.0)) {
            Ok(bytes) => CanonData::Bytes {
                len: bytes.len(),
                hash: fnv_hex(&bytes),
            },
            Err(error) => CanonData::Unreadable(error.to_string()),
        }
    }
}

