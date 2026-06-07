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
//!
//! attach 제어 API(acquire/release/force_detach/통지)의 호출자는 `attach.*` IPC
//! 핸들러로, `ipc/handler.rs:5` 와 동일하게 gui 라우팅 경유다. headless 빌드엔
//! 호출자가 없어(같은 정책) dead_code 를 *headless 한정* 침묵한다. `is_attached`
//! (서버 입력 차단)만 headless 에서도 `apply_send_to_surface` 가 호출한다.
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

use std::collections::HashMap;

use crate::adapters::production::stream_hub::StreamHub;
use crate::ipc::stream::{StreamFrame, StreamTag};
use crate::model::{SurfaceId, WorkspaceId};

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
    /// workspace 단위 점유(단계 6, D1). workspace 점유 시 그 안 모든 *터미널*
    /// surface 는 `surface_locks` 에도 동시에 들어가(holder 동일) 단계 4 의 서버
    /// 렌더 placeholder·입력차단이 무수정 재사용된다.
    workspace_locks: HashMap<WorkspaceId, AttachLock>,
    /// workspace 점유 시 그 안 *모든* surface(터미널+비-터미널) → workspace_id 역매핑
    /// (단계 6, D2). 비-터미널 placeholder 숨김(decision 3) 판정 + force-detach/EOF 시
    /// 멤버 일괄 정리에 쓴다. 비-터미널은 PTY 가 없어 lock(점유) 대상이 아니라 "숨김
    /// 표시" 만 필요하므로 surface_locks 가 아니라 이 맵으로 관리한다.
    surface_to_workspace: HashMap<SurfaceId, WorkspaceId>,
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

    /// surface 의 점유 client(없으면 None). 단계 4 placeholder 렌더("client N 점유
    /// 중")·force-detach UI 가 사용. 현재는 테스트만 호출하므로 dead_code 침묵.
    #[allow(dead_code)]
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
    /// 연결이라 통지하지 않는다. workspace 점유(단계 6)면 멤버 surface 까지 일괄
    /// 정리한다. 반환: 해제된 surface_id 목록.
    pub fn release_all_for_client(&mut self, client_id: AttachClientId) -> Vec<SurfaceId> {
        let mut released: Vec<SurfaceId> = Vec::new();
        // ① 이 client 가 hold 한 workspace → 멤버(터미널 lock + 역매핑) 일괄 해제.
        let ws_held: Vec<WorkspaceId> = self
            .workspace_locks
            .iter()
            .filter(|(_, l)| l.holder == client_id)
            .map(|(&w, _)| w)
            .collect();
        for ws in &ws_held {
            self.workspace_locks.remove(ws);
            let members: Vec<SurfaceId> = self
                .surface_to_workspace
                .iter()
                .filter(|(_, w)| **w == *ws)
                .map(|(&s, _)| s)
                .collect();
            for s in &members {
                self.surface_locks.remove(s);
                self.surface_to_workspace.remove(s);
                released.push(*s);
            }
        }
        // ② 남은 surface 단위 lock 해제.
        let surf_held: Vec<SurfaceId> = self
            .surface_locks
            .iter()
            .filter(|(_, l)| l.holder == client_id)
            .map(|(&sid, _)| sid)
            .collect();
        for sid in &surf_held {
            self.surface_locks.remove(sid);
            released.push(*sid);
        }
        released
    }

    /// 현재 점유 목록 스냅샷(`attach.list` 용).
    pub fn locks_snapshot(&self) -> Vec<(SurfaceId, AttachLock)> {
        self.surface_locks.iter().map(|(&s, &l)| (s, l)).collect()
    }

    /// client 가 점유 중인 surface(단계 4 입력 라우팅 역조회). surface 단위라 client
    /// 당 0/1 개 — 여러 개면 임의의 하나(workspace 단위는 단계 6 에서 명시 surface_id).
    pub fn surface_held_by(&self, client_id: AttachClientId) -> Option<SurfaceId> {
        self.surface_locks
            .iter()
            .find(|(_, l)| l.holder == client_id)
            .map(|(&s, _)| s)
    }

    // ─── workspace 단위 점유 (단계 6) ──────────────────────────────────────

    /// workspace 배타 점유(D1). free → 획득. 이미 점유면 `AlreadyAttached`.
    /// 같은 client 재-acquire 는 멱등. `terminals` 는 surface_locks 에 동시 등록되어
    /// 서버 렌더/입력차단이 자동 적용되고, `members`(터미널+비-터미널 전부)는
    /// `surface_to_workspace` 에 등록되어 비-터미널 숨김(D2)·일괄 정리(D6)에 쓰인다.
    ///
    /// **부분 점유 충돌 방지**: 멤버 터미널 중 하나라도 *다른* client 의 surface 단위
    /// lock 에 잡혀 있으면 거부한다(workspace 가 절반만 점유되는 상태 차단).
    pub fn acquire_workspace(
        &mut self,
        workspace_id: WorkspaceId,
        terminals: &[SurfaceId],
        members: &[SurfaceId],
        client_id: AttachClientId,
    ) -> Result<AttachLock, AttachError> {
        if let Some(existing) = self.workspace_locks.get(&workspace_id) {
            if existing.holder == client_id {
                return Ok(*existing); // 멱등 재-acquire
            }
            return Err(AttachError::AlreadyAttached {
                holder: existing.holder,
            });
        }
        for s in terminals {
            if let Some(l) = self.surface_locks.get(s)
                && l.holder != client_id
            {
                return Err(AttachError::AlreadyAttached { holder: l.holder });
            }
        }
        self.next_seq += 1;
        let lock = AttachLock {
            holder: client_id,
            granted_seq: self.next_seq,
        };
        self.workspace_locks.insert(workspace_id, lock);
        for s in terminals {
            self.surface_locks.entry(*s).or_insert(lock);
        }
        for s in members {
            self.surface_to_workspace.insert(*s, workspace_id);
        }
        Ok(lock)
    }

    /// surface 의 *내용을 숨겨야* 하는가(D2, decision 3). 터미널(surface_locks)이거나
    /// 점유된 workspace 의 멤버(비-터미널 포함)면 true. render 분기용.
    pub fn is_content_hidden(&self, surface_id: SurfaceId) -> bool {
        self.surface_locks.contains_key(&surface_id)
            || self.surface_to_workspace.contains_key(&surface_id)
    }

    /// workspace 의 점유 client(없으면 None).
    pub fn workspace_holder(&self, workspace_id: WorkspaceId) -> Option<AttachClientId> {
        self.workspace_locks.get(&workspace_id).map(|l| l.holder)
    }

    /// surface 가 속한 점유 workspace(없으면 None). 입력 라우팅 holder 검증·일괄 정리용.
    pub fn workspace_of_surface(&self, surface_id: SurfaceId) -> Option<WorkspaceId> {
        self.surface_to_workspace.get(&surface_id).copied()
    }

    /// surface 가 속한 점유 workspace 의 holder(placeholder 표시·force-detach UI 용).
    pub fn workspace_holder_of(&self, surface_id: SurfaceId) -> Option<AttachClientId> {
        self.workspace_of_surface(surface_id)
            .and_then(|ws| self.workspace_holder(ws))
    }

    /// client 가 workspace 를 하나라도 점유 중인가(입력 demux 모드 판정용).
    pub fn client_holds_workspace(&self, client_id: AttachClientId) -> bool {
        self.workspace_locks.values().any(|l| l.holder == client_id)
    }

    /// workspace 강제 해제(서버 권한, D6). workspace_lock + 멤버 surface_locks +
    /// surface_to_workspace 를 일괄 정리하고 holder 에게 종료 통지 push.
    /// 반환: 강제로 끊긴 holder(점유 중이 아니었으면 None).
    pub fn force_detach_workspace(&mut self, workspace_id: WorkspaceId) -> Option<AttachClientId> {
        let lock = self.workspace_locks.remove(&workspace_id)?;
        self.clear_workspace_members(workspace_id);
        self.notify_detached(lock.holder, "force_detach_workspace");
        Some(lock.holder)
    }

    /// 현재 workspace 점유 목록 스냅샷(`attach.list` 용).
    pub fn workspaces_snapshot(&self) -> Vec<(WorkspaceId, AttachLock)> {
        self.workspace_locks.iter().map(|(&w, &l)| (w, l)).collect()
    }

    /// workspace 의 멤버 surface(터미널 lock + 역매핑)를 전부 제거. 통지하지 않는다.
    fn clear_workspace_members(&mut self, workspace_id: WorkspaceId) {
        let members: Vec<SurfaceId> = self
            .surface_to_workspace
            .iter()
            .filter(|(_, w)| **w == workspace_id)
            .map(|(&s, _)| s)
            .collect();
        for s in &members {
            self.surface_locks.remove(s);
            self.surface_to_workspace.remove(s);
        }
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

    // ─── workspace 단위 (단계 6) ──────────────────────────────────────────

    #[test]
    fn acquire_workspace_locks_terminals_and_members() {
        let mut reg = AttachRegistry::new();
        // ws 100: 터미널 [10,11] + 비-터미널 [12].
        reg.acquire_workspace(100, &[10, 11], &[10, 11, 12], 1).unwrap();
        // 터미널은 surface_locks 에도 들어가 서버 렌더/입력차단이 자동 적용.
        assert!(reg.is_attached(10));
        assert!(reg.is_attached(11));
        assert!(!reg.is_attached(12)); // 비-터미널은 lock 아님
        // 내용 숨김은 멤버 전체(비-터미널 포함).
        assert!(reg.is_content_hidden(10));
        assert!(reg.is_content_hidden(12));
        assert_eq!(reg.workspace_holder(100), Some(1));
        assert_eq!(reg.workspace_of_surface(12), Some(100));
        assert_eq!(reg.workspace_holder_of(12), Some(1));
        assert!(reg.client_holds_workspace(1));
    }

    #[test]
    fn acquire_workspace_already_attached_rejected() {
        let mut reg = AttachRegistry::new();
        reg.acquire_workspace(100, &[10], &[10], 1).unwrap();
        let err = reg.acquire_workspace(100, &[10], &[10], 2).unwrap_err();
        assert_eq!(err, AttachError::AlreadyAttached { holder: 1 });
    }

    #[test]
    fn acquire_workspace_idempotent_same_client() {
        let mut reg = AttachRegistry::new();
        let a = reg.acquire_workspace(100, &[10], &[10], 1).unwrap();
        let b = reg.acquire_workspace(100, &[10], &[10], 1).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn acquire_workspace_partial_conflict_rejected() {
        let mut reg = AttachRegistry::new();
        // surface 11 을 client 2 가 surface 단위로 먼저 점유.
        reg.acquire(11, 2).unwrap();
        // ws 100 이 11 을 포함 → 부분 충돌로 거부.
        let err = reg.acquire_workspace(100, &[10, 11], &[10, 11], 1).unwrap_err();
        assert_eq!(err, AttachError::AlreadyAttached { holder: 2 });
        assert!(!reg.workspace_locks.contains_key(&100));
        assert!(!reg.is_attached(10)); // 부분 점유 안 됨
    }

    #[test]
    fn force_detach_workspace_clears_members() {
        let mut reg = AttachRegistry::new();
        reg.acquire_workspace(100, &[10, 11], &[10, 11, 12], 7).unwrap();
        assert_eq!(reg.force_detach_workspace(100), Some(7));
        assert!(!reg.is_attached(10));
        assert!(!reg.is_attached(11));
        assert!(!reg.is_content_hidden(12));
        assert_eq!(reg.workspace_holder(100), None);
        assert!(reg.workspaces_snapshot().is_empty());
    }

    #[test]
    fn force_detach_workspace_idempotent_when_free() {
        let mut reg = AttachRegistry::new();
        assert_eq!(reg.force_detach_workspace(100), None);
    }

    #[test]
    fn release_all_for_client_clears_workspace_and_surface() {
        let mut reg = AttachRegistry::new();
        reg.acquire_workspace(100, &[10, 11], &[10, 11, 12], 1).unwrap();
        reg.acquire(20, 1).unwrap(); // 별도 surface 단위
        reg.acquire_workspace(200, &[30], &[30], 2).unwrap(); // 다른 client
        let mut released = reg.release_all_for_client(1);
        released.sort_unstable();
        // workspace 멤버(터미널 10/11 + 비-터미널 12) + surface 단위 20 전부 해제.
        assert_eq!(released, vec![10, 11, 12, 20]);
        assert!(!reg.is_content_hidden(10));
        assert!(!reg.is_content_hidden(12));
        assert!(!reg.is_attached(20));
        // 다른 client 의 workspace 는 유지.
        assert_eq!(reg.workspace_holder(200), Some(2));
        assert!(reg.is_attached(30));
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
