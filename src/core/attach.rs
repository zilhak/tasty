//! 배타적 attach 점유 lock 레지스트리 (attach/detach 단계 3).
//!
//! surface 단위 점유만 다룬다(workspace 단위는 단계 6). 한 surface 는 동시에 한
//! client 만 점유한다 — 동시 attach 는 거부된다. 점유 상태는 **휘발성**(직렬화/
//! 복원 안 함, decision 2): 재시작 시 빈 registry → 모든 surface free 환원.
//!
//! `client_id` 는 단계 1 `StreamClientId`(=u32)를 그대로 재사용한다(별도 ID 공간
//! 없음). force-detach 통지는 주입된 [`StreamHub`] 로 push 한다 — 자체 push 채널을
//! 보유하지 않는다(StreamHub 가 client 연결 권위; design §2.1 의 ClientConn 을 대체).
//!
//! decision 5: 자체 인증/토큰 레이어 없음 — SSH + 127.0.0.1 loopback 위임.

use std::collections::HashMap;

use crate::adapters::production::stream_hub::StreamHub;
use crate::ipc::stream::{StreamFrame, StreamTag};
use crate::model::SurfaceId;

/// attach 의 client 식별자. 단계 1 `StreamClientId` 와 값·의미가 동일(둘 다 u32).
pub type AttachClientId = u32;

/// 한 surface 의 점유 정보.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AttachLock {
    pub holder: AttachClientId,
    /// 단조 증가 부여 순번. `Instant` 비의존(직렬화/복원/workflow 제약 무관).
    pub granted_seq: u64,
}

/// attach lock 연산 실패 사유.
#[derive(Debug, PartialEq, Eq)]
pub enum AttachError {
    /// 이미 다른 client 가 점유 중(동시 attach 거부).
    AlreadyAttached { holder: AttachClientId },
    /// release 요청 client 가 점유자가 아님.
    NotHolder { holder: AttachClientId },
    /// 해당 surface 에 점유 lock 없음.
    NotAttached,
}

/// 배타 attach 점유 lock 테이블. `CoreState` 가 보유(엔진 권위, model-view 분리).
#[derive(Default)]
pub struct AttachRegistry {
    surface_locks: HashMap<SurfaceId, AttachLock>,
    next_seq: u64,
    /// force-detach 통지용 push 핸들(단계 1 StreamHub). App 부팅 시 주입.
    /// `None`(테스트/미주입)이면 통지는 no-op + lock 만 free 환원.
    notifier: Option<StreamHub>,
}

impl AttachRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// App 부팅 시 StreamHub clone 주입(waker_factory 패턴). gui/headless 공통.
    /// 엔진과 IPC 서버가 **같은 StreamHub 인스턴스**를 공유해야 force-detach 가
    /// 올바른 소켓에 닿는다(client_id 발급처 = push 처).
    pub fn set_notifier(&mut self, hub: StreamHub) {
        self.notifier = Some(hub);
    }

    /// surface 가 현재 점유 중인지(입력 차단·list 용).
    pub fn is_attached(&self, surface_id: SurfaceId) -> bool {
        self.surface_locks.contains_key(&surface_id)
    }

    /// surface 의 점유 client(없으면 None).
    pub fn holder(&self, surface_id: SurfaceId) -> Option<AttachClientId> {
        self.surface_locks.get(&surface_id).map(|l| l.holder)
    }

    /// lock 획득. free → 획득. 이미 다른 client 점유 → `AlreadyAttached`(동시 attach
    /// 거부). 같은 client 의 재-acquire 는 멱등(현 lock 을 그대로 Ok 반환).
    pub fn acquire(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
    ) -> Result<AttachLock, AttachError> {
        if let Some(existing) = self.surface_locks.get(&surface_id) {
            if existing.holder == client_id {
                return Ok(*existing); // 멱등 재-acquire
            }
            return Err(AttachError::AlreadyAttached {
                holder: existing.holder,
            });
        }
        self.next_seq += 1;
        let lock = AttachLock {
            holder: client_id,
            granted_seq: self.next_seq,
        };
        self.surface_locks.insert(surface_id, lock);
        Ok(lock)
    }

    /// 정상 해제(holder 본인). holder 불일치 → `NotHolder`, lock 없음 → `NotAttached`.
    pub fn release(
        &mut self,
        surface_id: SurfaceId,
        client_id: AttachClientId,
    ) -> Result<(), AttachError> {
        match self.surface_locks.get(&surface_id) {
            None => Err(AttachError::NotAttached),
            Some(l) if l.holder != client_id => Err(AttachError::NotHolder { holder: l.holder }),
            Some(_) => {
                self.surface_locks.remove(&surface_id);
                Ok(())
            }
        }
    }

    /// 강제 해제(서버 권한). holder 검사 없이 free 환원 + holder 에게 종료 통지 push.
    /// 반환: 강제로 끊긴 holder client_id(점유 중이 아니었으면 None).
    pub fn force_detach(&mut self, surface_id: SurfaceId) -> Option<AttachClientId> {
        let lock = self.surface_locks.remove(&surface_id)?;
        self.notify_detached(lock.holder, "force_detach");
        Some(lock.holder)
    }

    /// stream client 연결 종료(EOF) 시 그 client 의 모든 lock 해제. 이미 끊긴
    /// 연결이라 통지하지 않는다. 반환: 해제된 surface_id 목록.
    pub fn release_all_for_client(&mut self, client_id: AttachClientId) -> Vec<SurfaceId> {
        let released: Vec<SurfaceId> = self
            .surface_locks
            .iter()
            .filter(|(_, l)| l.holder == client_id)
            .map(|(&sid, _)| sid)
            .collect();
        for sid in &released {
            self.surface_locks.remove(sid);
        }
        released
    }

    /// 현재 점유 목록 스냅샷(`attach.list` 용).
    pub fn locks_snapshot(&self) -> Vec<(SurfaceId, AttachLock)> {
        self.surface_locks.iter().map(|(&s, &l)| (s, l)).collect()
    }

    /// holder 에게 강제 분리 통지를 push(사유 Control + Detach 종료 신호). best-effort:
    /// notifier 미주입이거나 holder 가 이미 끊겼으면 무해하게 무시된다.
    fn notify_detached(&self, holder: AttachClientId, reason: &str) {
        let Some(hub) = &self.notifier else {
            return;
        };
        let msg = serde_json::json!({ "event": "force_detached", "reason": reason });
        let payload = serde_json::to_vec(&msg).unwrap_or_default();
        let _ = hub.push(holder, StreamFrame::new(StreamTag::Control, payload));
        let _ = hub.push(holder, StreamFrame::new(StreamTag::Detach, Vec::new()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_free_succeeds() {
        let mut reg = AttachRegistry::new();
        let lock = reg.acquire(10, 1).unwrap();
        assert_eq!(lock.holder, 1);
        assert_eq!(lock.granted_seq, 1);
        assert!(reg.is_attached(10));
        assert_eq!(reg.holder(10), Some(1));
    }

    #[test]
    fn acquire_already_attached_rejected() {
        // 동시 attach 거부 — 핵심.
        let mut reg = AttachRegistry::new();
        reg.acquire(10, 1).unwrap();
        let err = reg.acquire(10, 2).unwrap_err();
        assert_eq!(err, AttachError::AlreadyAttached { holder: 1 });
        // 점유는 client 1 에 유지.
        assert_eq!(reg.holder(10), Some(1));
    }

    #[test]
    fn acquire_same_client_idempotent() {
        let mut reg = AttachRegistry::new();
        let a = reg.acquire(10, 1).unwrap();
        let b = reg.acquire(10, 1).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn release_by_holder() {
        let mut reg = AttachRegistry::new();
        reg.acquire(10, 1).unwrap();
        reg.release(10, 1).unwrap();
        assert!(!reg.is_attached(10));
    }

    #[test]
    fn release_non_holder_rejected() {
        let mut reg = AttachRegistry::new();
        reg.acquire(10, 1).unwrap();
        let err = reg.release(10, 2).unwrap_err();
        assert_eq!(err, AttachError::NotHolder { holder: 1 });
        assert!(reg.is_attached(10)); // 여전히 점유.
    }

    #[test]
    fn release_not_attached() {
        let mut reg = AttachRegistry::new();
        assert_eq!(reg.release(10, 1).unwrap_err(), AttachError::NotAttached);
    }

    #[test]
    fn force_detach_frees_and_returns_holder() {
        let mut reg = AttachRegistry::new();
        reg.acquire(10, 7).unwrap();
        assert_eq!(reg.force_detach(10), Some(7));
        assert!(!reg.is_attached(10));
    }

    #[test]
    fn force_detach_idempotent_when_free() {
        let mut reg = AttachRegistry::new();
        assert_eq!(reg.force_detach(10), None);
    }

    #[test]
    fn release_all_for_client() {
        let mut reg = AttachRegistry::new();
        reg.acquire(10, 1).unwrap();
        reg.acquire(11, 1).unwrap();
        reg.acquire(12, 2).unwrap();
        let mut released = reg.release_all_for_client(1);
        released.sort_unstable();
        assert_eq!(released, vec![10, 11]);
        assert!(!reg.is_attached(10));
        assert!(!reg.is_attached(11));
        assert!(reg.is_attached(12)); // 다른 client 유지.
    }

    #[test]
    fn granted_seq_monotonic() {
        let mut reg = AttachRegistry::new();
        let a = reg.acquire(10, 1).unwrap();
        let b = reg.acquire(11, 2).unwrap();
        assert!(b.granted_seq > a.granted_seq);
    }

    #[test]
    fn force_detach_pushes_to_notifier() {
        use crate::ipc::stream::StreamTag;
        let hub = StreamHub::new();
        let holder = hub.alloc_id();
        let rx = hub.register(holder);
        let mut reg = AttachRegistry::new();
        reg.set_notifier(hub);
        reg.acquire(10, holder).unwrap();
        assert_eq!(reg.force_detach(10), Some(holder));
        // Control(force_detached) 다음 Detach 프레임이 holder 의 sink 로 push 됨.
        let f1 = rx.recv().unwrap();
        assert_eq!(f1.tag, StreamTag::Control);
        assert!(String::from_utf8_lossy(&f1.payload).contains("force_detached"));
        let f2 = rx.recv().unwrap();
        assert_eq!(f2.tag, StreamTag::Detach);
    }
}
