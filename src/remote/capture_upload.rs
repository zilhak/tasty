//! 캡처 업로드를 (client_id, upload_id)별로 누적한다.
//! 프레임 해석은 stream_hub, 권한 확인·파일 저장·클립보드는 attach_runtime이 맡는다.
//! 시각을 인자로 받아 실제 대기 없이 만료를 검사할 수 있다.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use super::transfer_spool::{Spool,TransferOwner};

/// 마지막 청크 이후 이 시간 이상 지난 버퍼를 회수한다. 연결 종료를 판정하는 값은 아니다.
pub(crate) const DEFAULT_TTL: Duration = Duration::from_secs(300);

struct PartialUpload {
    spool:Spool,
    owner:TransferOwner,
    last_activity: Instant,
}

/// append가 만료 버퍼를 회수하고, 연결 종료 시 호출자가 clear_client를 호출한다.
/// GUI는 주기 타이머로도 정리한다. 헤드리스에는 주기 회수가 없어 새 청크나 연결 종료를 기다린다.
pub(crate) struct CaptureUploadRegistry {
    partials: HashMap<(u32, u64), PartialUpload>,
    ttl: Duration,
}

impl Default for CaptureUploadRegistry {
    fn default() -> Self {
        Self::with_ttl(DEFAULT_TTL)
    }
}

impl CaptureUploadRegistry {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn with_ttl(ttl: Duration) -> Self {
        Self {
            partials: HashMap::new(),
            ttl,
        }
    }

    /// 만료 버퍼를 먼저 지운 뒤 청크를 추가한다. 만료된 같은 ID가 다시 오면 새 버퍼가 된다.
    pub(crate) fn append(&mut self, client_id:u32,upload_id:u64,data:&[u8],now:Instant,owner:TransferOwner)->bool {
        self.sweep_expired(now);
        let key=(client_id,upload_id);
        if !self.partials.contains_key(&key) {
            if self.partials.len()>=256 {return false;}
            let spool=match Spool::new() {Ok(spool)=>spool,Err(error)=>{tracing::warn!(%error,"capture spool creation failed");return false;}};
            self.partials.insert(key,PartialUpload {spool,owner:owner.clone(),last_activity:now});
        }
        let entry=self.partials.get_mut(&key).expect("capture entry inserted");
        if !entry.owner.same(&owner) {self.partials.remove(&key);return false;}
        if let Err(error)=entry.spool.append(data) {tracing::warn!(%error,"capture spool write failed");self.partials.remove(&key);return false;}
        entry.last_activity=now;true
    }

    /// 마지막 활동에서 TTL 이상 지난 버퍼를 지운다.
    pub(crate) fn sweep_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        let before = self.partials.len();
        self.partials
            .retain(|_, e| now.duration_since(e.last_activity) < ttl);
        let removed = before - self.partials.len();
        if removed > 0 {
            tracing::debug!(
                "capture upload: removed {removed} incomplete upload(s) idle for at least the TTL"
            );
        }
    }

    /// 버퍼를 제거하고 바이트를 반환한다. 여기서는 만료 시간을 다시 검사하지 않는다.
    pub(crate) fn take(&mut self, client_id: u32, upload_id: u64) -> Option<(TransferOwner,Spool)> {
        self.partials
            .remove(&(client_id, upload_id))
            .map(|e| (e.owner,e.spool))
    }

    pub(crate) fn clear_client(&mut self, client_id: u32) {
        self.partials.retain(|(cid, _), _| *cid != client_id);
    }
}

