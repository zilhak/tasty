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
//! 호출자가 없어(같은 정책) dead_code 를 *headless 한정* 침묵한다. `is_hard_occupied`
//! (서버 입력 차단)만 headless 에서도 `apply_send_to_surface` 가 호출한다.
//!
//! ADR-0040: 이 레지스트리는 hard(원격 attach)와 soft(표시만) 점유를 통합한다. hard 는
//! 위 기존 메커니즘 그대로이고, soft 는 `soft` 테이블에 additive 로 얹혀 hard 술어를
//! 오염하지 않는다. 통합 조회는 `occupancy_of`.
// 이유: 이 레지스트리를 읽고 쓰는 것이 렌더·attach 폴링(gui)이라 headless 엔 호출자가 거의 없다
// (위 모듈 주석). 모듈을 `#[cfg]` 로 가리지 않는 것은 headless 에서도 타입체크를 받게 하려는 것이다.
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

// ─── 통합 점유 모델 (ADR-0040) ────────────────────────────────────────────
//
// hard 점유(원격 attach)와 soft 점유(표시만)를 하나의 레지스트리가 관리한다.
// hard 는 기존 `AttachLock`(holder=StreamClient) 저장을 **그대로 보존** 하고, soft 는
// 별도 테이블(`soft`)에 additive 로 얹는다. 두 계층은 `occupancy_of` 로 단일 조회되며
// (작업 02 테두리 렌더 소비), soft 는 절대 hard 술어(`is_hard_occupied`)를 true 로
// 만들지 않는다(입력차단/mirror 회귀 0). soft 경로는 StreamHub/gui 비의존이라
// headless 컴파일·동작한다.

/// 점유 계층. `Soft`=advisory 마커(write 허용), `Hard`=readonly 강제(기존 attach 메커니즘).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OccupancyTier {
    Soft,
    Hard,
}

/// 점유 주체. hard 는 stream client(u32), soft 는 parent surface 로 식별되는 주체.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Holder {
    /// hard 점유자 = stream client(기존 attach). 값·의미 보존.
    StreamClient(AttachClientId),
    /// soft 점유자 = 주체에 대응하는 parent surface(=`Occupancy.parent`) + 선택 라벨.
    Subject { label: Option<String> },
}

/// 한 surface 의 통합 점유 뷰(tier 무관 단일 조회, 작업 02 소비). `occupancy_of` 가
/// tier 별 내부 저장(`surface_locks`/`soft`)을 이 값으로 투영해 반환한다.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occupancy {
    pub tier: OccupancyTier,
    pub holder: Holder,
    /// soft: 점유 주체에 대응하는 parent surface(focus 시 부재 청소용, ADR-0040 수명).
    /// hard: 항상 None(연결 EOF/force-detach 수명이라 parent 기록 불필요).
    pub parent: Option<SurfaceId>,
    pub granted_seq: u64,
}

/// soft 점유 연산 실패 사유(hard 의 `AttachError` 와 분리 — holder 표현이 다름).
#[derive(Debug, PartialEq, Eq)]
pub enum OccupancyError {
    /// 이미 다른 주체(parent)가 soft 점유 중(1:1 배타, ADR-0040).
    AlreadyOccupied { parent: SurfaceId },
    /// release 요청 주체(parent)가 점유자가 아님.
    NotHolder { parent: SurfaceId },
    /// 해당 surface 에 soft 점유 없음.
    NotOccupied,
}

/// soft 점유 내부 엔트리. hard(`AttachLock`)와 **분리 저장** — hard 기계(workspace_locks/
/// notifier/force-detach/release_all_for_client)는 이 맵에 진입하지 않는다.
#[derive(Clone, Debug, PartialEq, Eq)]
struct SoftEntry {
    parent: SurfaceId,
    label: Option<String>,
    granted_seq: u64,
}

/// 통합 점유 레지스트리(ADR-0040). hard(원격 attach)와 soft(표시만) 두 계층을 함께
/// 관리한다. `CoreState` 가 보유(엔진 권위, model-view 분리). 구 이름은 `AttachRegistry`.
#[derive(Default)]
pub struct OccupancyRegistry {
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
    /// **이미 끊긴 것으로 확인된 client** — 아직 그 배치의 정리 단계가 오지 않았을 뿐이다.
    /// [`Self::mark_clients_disconnected`] 가 채우고 [`Self::release_all_for_client`] 가
    /// 비운다. 그래서 한 배치 안에서만 값을 갖는다(누적되지 않는다).
    dead_clients: std::collections::HashSet<AttachClientId>,
    /// force-detach 통지용 push 핸들(단계 1 StreamHub). App 부팅 시 주입.
    /// `None`(테스트/미주입)이면 통지는 no-op + lock 만 free 환원. **hard 전용** —
    /// soft 는 이 경로에 진입하지 않는다.
    notifier: Option<StreamHub>,
    /// soft 점유 테이블(ADR-0040). hard(`surface_locks`)와 **분리** — soft 는 절대
    /// `is_hard_occupied` 를 true 로 만들지 않는다(입력차단/mirror/`"attached"`/content-hidden
    /// 회귀 0). gui/StreamHub 비의존이라 headless 컴파일·동작. holder=주체(parent surface).
    soft: HashMap<SurfaceId, SoftEntry>,
    /// `true` 인 동안 [`CoreState::tap_new_workspace_member`](crate::core::CoreState::tap_new_workspace_member)
    /// 는 멤버 편입(`add_workspace_member`)만 하고 실제 stream tap(`tap_surface_for_stream`)
    /// 은 스킵한다. forward-op 실행(`execute_forwarded_structural_op`)이 재사용 IPC
    /// 핸들러(`handle_split`/`handle_tab_create`)를 호출하는 동안만 켠다 — 그 경로는
    /// 호출측(`apply_forwarded_structural_op`/`boot.rs`)이 `StructuralDelta` 전송 **후**
    /// 정확한 순서로 직접 tap 하므로, 핸들러 내부의 즉시-tap 이 겹치면 이중 tap(문자
    /// 중복 echo)이 된다. 로컬 생성 경로(예: `tasty claude spawn`)는 이 플래그가 항상
    /// `false`라 기존대로 즉시 tap 된다.
    suppress_auto_tap: bool,
}

impl OccupancyRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// App 부팅 시 StreamHub clone 주입(waker_factory 패턴). gui/headless 공통.
    /// 엔진과 IPC 서버가 **같은 StreamHub 인스턴스**를 공유해야 force-detach 가
    /// 올바른 소켓에 닿는다(client_id 발급처 = push 처).
    pub fn set_notifier(&mut self, hub: StreamHub) {
        self.notifier = Some(hub);
    }

    /// 주입된 notifier(StreamHub)를 clone 해 반환(`StreamHub` 는 Arc 기반이라 clone
    /// 저렴, 위 `set_notifier` doc 참조). hard 점유 중인 workspace 에 로컬에서 새로
    /// 편입된 surface 를 즉시 tap 하는 경로(`CoreState::tap_new_workspace_member`)가
    /// 쓴다 — `hub`/`client_id` 를 `core/mod.rs` 의 순수 mutate 계층까지 파라미터로
    /// 꿰지 않고 boot 시 주입된 것을 재사용한다(`notify_detached` 와 동일 패턴).
    pub(crate) fn notifier(&self) -> Option<StreamHub> {
        self.notifier.clone()
    }

    /// [`Self::suppress_auto_tap`] 현재 값(`tap_new_workspace_member` 가 즉시-tap 을
    /// 스킵해야 하는지).
    pub(crate) fn is_auto_tap_suppressed(&self) -> bool {
        self.suppress_auto_tap
    }

    /// forward-op 실행 구간 동안 즉시-tap 을 켜고/끈다. `execute_forwarded_structural_op`
    /// 가 재사용 핸들러 호출 직전/직후에만 짧게 감싼다(early-return 경로 없음 —
    /// 핸들러 호출 자체는 `Result` 를 반환하지 않아 `?` 로 건너뛸 수 없다).
    pub(crate) fn set_auto_tap_suppressed(&mut self, suppressed: bool) {
        self.suppress_auto_tap = suppressed;
    }

    /// surface 가 **hard 점유** 중인지(서버 입력 차단·list `attached` 필드·readonly
    /// mirror·content-hidden 판정용). ADR-0040 이후 이 술어는 **hard 전용** 이다 —
    /// soft 점유는 절대 true 로 만들지 않는다(입력차단/mirror 회귀 방지). 소비처 5곳
    /// 모두 hard 의미이므로 개명(구 `is_attached`)만으로 의미 보존.
    pub fn is_hard_occupied(&self, surface_id: SurfaceId) -> bool {
        self.surface_locks.contains_key(&surface_id)
    }

    /// 통합 점유 조회(작업 02 테두리 렌더 소비). tier 를 한 번에 판별한다. hard 가
    /// soft 를 가린다(ADR-0040 테두리 우선순위: 점유 surface 는 hard 표시가 soft 를 덮음).
    /// 점유 없으면 None. 작업 02 테두리 렌더(egui_panels.rs::draw_occupied_overlays)가 소비.
    pub fn occupancy_of(&self, surface_id: SurfaceId) -> Option<Occupancy> {
        if let Some(lock) = self.surface_locks.get(&surface_id) {
            return Some(Occupancy {
                tier: OccupancyTier::Hard,
                holder: Holder::StreamClient(lock.holder),
                parent: None,
                granted_seq: lock.granted_seq,
            });
        }
        self.soft.get(&surface_id).map(|e| Occupancy {
            tier: OccupancyTier::Soft,
            holder: Holder::Subject {
                label: e.label.clone(),
            },
            parent: Some(e.parent),
            granted_seq: e.granted_seq,
        })
    }

    /// soft 점유 획득(표시만, write 제한 없음 — ADR-0040). 같은 parent 재-acquire 는
    /// 멱등(라벨만 갱신). 이미 다른 주체의 soft 점유면 `AlreadyOccupied`. hard 기계와
    /// 무관 — StreamHub/gui 없이 동작(headless 안전). `CoreState::occupy_soft` 가 호출한다.
    pub fn acquire_soft(
        &mut self,
        surface_id: SurfaceId,
        parent: SurfaceId,
        label: Option<String>,
    ) -> Result<(), OccupancyError> {
        if let Some(existing) = self.soft.get(&surface_id)
            && existing.parent != parent
        {
            return Err(OccupancyError::AlreadyOccupied {
                parent: existing.parent,
            });
        }
        self.next_seq += 1;
        self.soft.insert(
            surface_id,
            SoftEntry {
                parent,
                label,
                granted_seq: self.next_seq,
            },
        );
        Ok(())
    }

    /// soft 점유 self-release(ADR-0040: 주체 본인 해제). parent(주체 식별자) 불일치 →
    /// `NotHolder`, 엔트리 없음 → `NotOccupied`. `terminal.release` IPC 가
    /// in-process 호출한다.
    pub fn release_soft(
        &mut self,
        surface_id: SurfaceId,
        parent: SurfaceId,
    ) -> Result<(), OccupancyError> {
        match self.soft.get(&surface_id) {
            None => Err(OccupancyError::NotOccupied),
            Some(e) if e.parent != parent => Err(OccupancyError::NotHolder { parent: e.parent }),
            Some(_) => {
                self.soft.remove(&surface_id);
                Ok(())
            }
        }
    }

    /// soft 점유 무조건 해제(주체 검증 없음). 로컬 사용자 force-detach 와 focus 지연
    /// 청소(ADR-0040 §수명)가 tier 공용으로 호출한다 — soft holder 는 stream client 가
    /// 아니라 StreamHub 통지 없이 엔트리만 제거된다. 반환: 실제로 제거됐는지.
    pub fn clear_soft(&mut self, surface_id: SurfaceId) -> bool {
        self.soft.remove(&surface_id).is_some()
    }

    /// surface 의 점유 client(없으면 None). 단계 4 placeholder 렌더("client N 점유
    /// 중")·force-detach UI + mesh context/full-resend 요청의 holder 검증
    /// (`docs/dev-guide/attach-behavior.md` "mesh mirror 채널" 절,
    /// `CoreState::apply_attached_mesh_context`)이 사용.
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
        if let Some(existing) = self.surface_locks.get(&surface_id).copied() {
            if existing.holder == client_id {
                return Ok(existing); // 멱등 재-acquire
            }
            // 같은 배치에서 이미 끊긴 holder 는 막지 못한다
            // ([`Self::mark_clients_disconnected`]).
            if !self.evict_if_dead(existing.holder) {
                return Err(AttachError::AlreadyAttached {
                    holder: existing.holder,
                });
            }
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

    /// **이 client 들은 이미 끊겼다** — 아직 그 배치의 정리 단계가 오지 않았을 뿐이다.
    ///
    /// pump 한 배치의 적용 순서는 **attach 결선이 먼저, 끊김 정리가 마지막**이다
    /// (`src/boot/headless_stream.rs` 모듈 주석이 그 순서를 계약으로 선언하고, gui 의
    /// `App::apply_stream_outcome` 도 같은 순서다). 그 계약에는 이유가 있다 — 끊긴
    /// client 가 죽기 직전에 보낸 입력 프레임은 그 client 의 점유가 살아 있는 동안
    /// 적용돼야 하고, 그러려면 lock 해제가 입력 적용보다 뒤여야 한다.
    ///
    /// 그런데 그 순서만 두면 **같은 배치에 실린 제3자의 재attach 가 이미 죽은 holder
    /// 에게 막힌다**: 서버는 같은 배치의 끝에서 그 점유를 놓을 것을 이미 알면서도
    /// `already_attached` 를 돌려준다. 그 거절은 client 쪽에서 *영구* 충돌로 분류돼
    /// 재연결 backoff 를 최대 간격으로 고정한다
    /// (`crate::app::auto_attach` 의 `on_reconnect_attempt_failed`) — 끊기자마자
    /// 다시 붙는 정상 재연결이 가장 오래 기다리게 된다.
    ///
    /// 그래서 순서를 바꾸는 대신 **상태 전이를 거부**한다: 죽었다고 표시된 holder 의
    /// lock 은 다음 acquire 가 걸려 넘어질 대상이 아니라, 그 자리에서 회수할 대상이다
    /// (`acquire` · `acquire_workspace`). 경쟁하는 acquire 가 없으면 lock 은 그대로
    /// 남아 그 client 의 잔여 입력이 정상 처리되고, 배치 끝의
    /// [`Self::release_all_for_client`] 가 lock 과 표시를 함께 지운다.
    ///
    /// **부하와 무관하다** — 창을 좁힌 것이 아니라, 같은 배치에 실렸는지만 본다.
    pub fn mark_clients_disconnected(&mut self, clients: &[AttachClientId]) {
        self.dead_clients.extend(clients.iter().copied());
    }

    /// 죽은 holder 인가. [`Self::mark_clients_disconnected`] 참조.
    fn is_dead(&self, client_id: AttachClientId) -> bool {
        self.dead_clients.contains(&client_id)
    }

    /// acquire 를 막고 선 holder 가 **이미 끊긴 것**이면 그 자리에서 점유를 회수한다.
    /// 반환: 회수했는가(= 이 holder 는 더 이상 막지 못한다).
    ///
    /// 회수는 경쟁이 실제로 있을 때만 한다 — 경쟁이 없으면 lock 은 배치 끝까지 남아
    /// 그 client 의 잔여 입력 프레임이 정상 처리된다(끊김 정리를 마지막에 두는
    /// 계약의 목적). 경쟁이 있으면 그 잔여 입력은 어차피 주인이 바뀐 자원으로 가므로
    /// 버리는 것이 맞다.
    fn evict_if_dead(&mut self, holder: AttachClientId) -> bool {
        if !self.is_dead(holder) {
            return false;
        }
        self.release_all_for_client(holder);
        true
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
        // 이 배치의 "죽었다" 표시는 여기서 수명을 다한다 — lock 이 실제로 사라졌으니
        // 더 이상 회수 대상이 아니다. 표시를 남기면 재사용된 client_id 가 잘못 죽는다.
        self.dead_clients.remove(&client_id);
        released
    }

    /// **surface 가 사라졌다** — 그 surface 에 딸린 점유 흔적만 지운다.
    ///
    /// 닫기 정리(`AppState::cleanup_surface`)가 부른다. 이것이 없으면 레지스트리가
    /// **존재하지 않는 surface 를 점유 중이라고 계속 말한다**(실측으로 그랬다).
    ///
    /// # 왜 `release_occupancy` 를 쓰지 않는가
    ///
    /// 그쪽은 **로컬 사용자의 강제 끊기**용이라 workspace 티어 멤버를 만나면
    /// `force_detach_workspace` 로 **워크스페이스 락을 통째로** 무너뜨린다. 닫힌 것이
    /// 멤버 하나뿐인데 그것을 부르면 **닫지도 않은 형제 surface 의 점유까지 풀린다**
    /// (실측: 멤버 셋 중 하나를 그렇게 풀면 셋 다 풀렸다) — holder 가 워크스페이스에서
    /// 통째로 쫓겨난다. 여기서 필요한 것은 "이 한 자리를 잊어라" 뿐이다.
    ///
    /// # 통지하지 않는다
    ///
    /// surface 자체가 사라졌으므로 holder 는 구조 delta 로 그 사실을 받는다. 여기서
    /// `notify_detached` 를 쏘면 holder 는 "강제로 끊겼다" 로 읽는데, workspace 티어에서는
    /// **여전히 워크스페이스를 들고 있으므로 그것이 거짓**이다.
    ///
    /// 반환: 실제로 지운 것이 있었는지.
    pub fn forget_closed_surface(&mut self, surface_id: SurfaceId) -> bool {
        let had_lock = self.surface_locks.remove(&surface_id).is_some();
        let had_member = self.surface_to_workspace.remove(&surface_id).is_some();
        let had_soft = self.clear_soft(surface_id);
        had_lock || had_member || had_soft
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
        if let Some(existing) = self.workspace_locks.get(&workspace_id).copied() {
            if existing.holder == client_id {
                return Ok(existing); // 멱등 재-acquire
            }
            // 같은 배치에서 이미 끊긴 holder 는 막지 못한다
            // ([`Self::mark_clients_disconnected`]).
            if !self.evict_if_dead(existing.holder) {
                return Err(AttachError::AlreadyAttached {
                    holder: existing.holder,
                });
            }
        }
        for s in terminals {
            if let Some(l) = self.surface_locks.get(s).copied()
                && l.holder != client_id
                && !self.evict_if_dead(l.holder)
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

    /// 이미 점유된 workspace 에 **나중에 생긴 멤버 surface 를 추가 등록**한다. 구조 변경
    /// forward(split·새 탭 등)로 원격에 새 surface 가 생겼을 때, "workspace 전체가
    /// remote"(ADR-0040 불변식)를 유지하려면 그 새 surface 도 같은 holder 점유에 편입돼야
    /// 한다. `acquire_workspace` 의 per-member 등록과 동형으로: 터미널이면 surface_locks 에
    /// (is_hard_occupied → 서버 입력차단·resize skip·readonly), 모든 멤버를
    /// surface_to_workspace 에(입력 라우팅 holder 검증·EOF/force-detach 일괄 정리) 넣는다.
    /// 기존 workspace 점유의 lock(holder/granted_seq)을 그대로 승계한다. workspace 가
    /// 점유돼 있지 않으면 no-op(false). 멱등(이미 등록된 surface 는 유지).
    pub fn add_workspace_member(
        &mut self,
        workspace_id: WorkspaceId,
        surface_id: SurfaceId,
        is_terminal: bool,
    ) -> bool {
        let Some(&lock) = self.workspace_locks.get(&workspace_id) else {
            return false;
        };
        if is_terminal {
            self.surface_locks.entry(surface_id).or_insert(lock);
        }
        self.surface_to_workspace.insert(surface_id, workspace_id);
        true
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

    /// surface 가 속한 점유 workspace 의 holder(placeholder 표시·force-detach UI,
    /// mesh apply_attached_mesh_* 의 holder 검증 fallback 용 — workspace 단위 attach 로
    /// 편입된 비-터미널 mesh surface 는 `surface_locks`(=`holder()`) 에는 없고 이 경로로만
    /// 조회된다).
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
        tracing::debug!(
            "attach: force_detach_workspace workspace={workspace_id:?} holder={:?}",
            lock.holder
        );
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
        let _ = hub.push(holder, StreamFrame::new(StreamTag::Control, payload)); // best-effort 강제분리 통지 — PushResult 무시(위 doc 참조).
        let _ = hub.push(holder, StreamFrame::new(StreamTag::Detach, Vec::new())); // best-effort detach 신호 — holder 끊겼으면 무해.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_free_succeeds() {
        let mut reg = OccupancyRegistry::new();
        let lock = reg.acquire(10, 1).unwrap();
        assert_eq!(lock.holder, 1);
        assert_eq!(lock.granted_seq, 1);
        assert!(reg.is_hard_occupied(10));
        assert_eq!(reg.holder(10), Some(1));
    }

    #[test]
    fn acquire_already_attached_rejected() {
        // 동시 attach 거부 — 핵심.
        let mut reg = OccupancyRegistry::new();
        reg.acquire(10, 1).unwrap();
        let err = reg.acquire(10, 2).unwrap_err();
        assert_eq!(err, AttachError::AlreadyAttached { holder: 1 });
        // 점유는 client 1 에 유지.
        assert_eq!(reg.holder(10), Some(1));
    }

    #[test]
    fn acquire_same_client_idempotent() {
        let mut reg = OccupancyRegistry::new();
        let a = reg.acquire(10, 1).unwrap();
        let b = reg.acquire(10, 1).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn release_by_holder() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire(10, 1).unwrap();
        reg.release(10, 1).unwrap();
        assert!(!reg.is_hard_occupied(10));
    }

    #[test]
    fn release_non_holder_rejected() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire(10, 1).unwrap();
        let err = reg.release(10, 2).unwrap_err();
        assert_eq!(err, AttachError::NotHolder { holder: 1 });
        assert!(reg.is_hard_occupied(10)); // 여전히 점유.
    }

    #[test]
    fn release_not_attached() {
        let mut reg = OccupancyRegistry::new();
        assert_eq!(reg.release(10, 1).unwrap_err(), AttachError::NotAttached);
    }

    #[test]
    fn force_detach_frees_and_returns_holder() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire(10, 7).unwrap();
        assert_eq!(reg.force_detach(10), Some(7));
        assert!(!reg.is_hard_occupied(10));
    }

    #[test]
    fn force_detach_idempotent_when_free() {
        let mut reg = OccupancyRegistry::new();
        assert_eq!(reg.force_detach(10), None);
    }

    #[test]
    fn release_all_for_client() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire(10, 1).unwrap();
        reg.acquire(11, 1).unwrap();
        reg.acquire(12, 2).unwrap();
        let mut released = reg.release_all_for_client(1);
        released.sort_unstable();
        assert_eq!(released, vec![10, 11]);
        assert!(!reg.is_hard_occupied(10));
        assert!(!reg.is_hard_occupied(11));
        assert!(reg.is_hard_occupied(12)); // 다른 client 유지.
    }

    #[test]
    fn granted_seq_monotonic() {
        let mut reg = OccupancyRegistry::new();
        let a = reg.acquire(10, 1).unwrap();
        let b = reg.acquire(11, 2).unwrap();
        assert!(b.granted_seq > a.granted_seq);
    }

    // ─── workspace 단위 (단계 6) ──────────────────────────────────────────

    #[test]
    fn acquire_workspace_locks_terminals_and_members() {
        let mut reg = OccupancyRegistry::new();
        // ws 100: 터미널 [10,11] + 비-터미널 [12].
        reg.acquire_workspace(100, &[10, 11], &[10, 11, 12], 1)
            .unwrap();
        // 터미널은 surface_locks 에도 들어가 서버 렌더/입력차단이 자동 적용.
        assert!(reg.is_hard_occupied(10));
        assert!(reg.is_hard_occupied(11));
        assert!(!reg.is_hard_occupied(12)); // 비-터미널은 lock 아님
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
        let mut reg = OccupancyRegistry::new();
        reg.acquire_workspace(100, &[10], &[10], 1).unwrap();
        let err = reg.acquire_workspace(100, &[10], &[10], 2).unwrap_err();
        assert_eq!(err, AttachError::AlreadyAttached { holder: 1 });
    }

    #[test]
    fn acquire_workspace_idempotent_same_client() {
        let mut reg = OccupancyRegistry::new();
        let a = reg.acquire_workspace(100, &[10], &[10], 1).unwrap();
        let b = reg.acquire_workspace(100, &[10], &[10], 1).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn acquire_workspace_partial_conflict_rejected() {
        let mut reg = OccupancyRegistry::new();
        // surface 11 을 client 2 가 surface 단위로 먼저 점유.
        reg.acquire(11, 2).unwrap();
        // ws 100 이 11 을 포함 → 부분 충돌로 거부.
        let err = reg
            .acquire_workspace(100, &[10, 11], &[10, 11], 1)
            .unwrap_err();
        assert_eq!(err, AttachError::AlreadyAttached { holder: 2 });
        assert!(!reg.workspace_locks.contains_key(&100));
        assert!(!reg.is_hard_occupied(10)); // 부분 점유 안 됨
    }

    #[test]
    fn force_detach_workspace_clears_members() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_workspace(100, &[10, 11], &[10, 11, 12], 7)
            .unwrap();
        assert_eq!(reg.force_detach_workspace(100), Some(7));
        assert!(!reg.is_hard_occupied(10));
        assert!(!reg.is_hard_occupied(11));
        assert!(!reg.is_content_hidden(12));
        assert_eq!(reg.workspace_holder(100), None);
        assert!(reg.workspaces_snapshot().is_empty());
    }

    #[test]
    fn force_detach_workspace_idempotent_when_free() {
        let mut reg = OccupancyRegistry::new();
        assert_eq!(reg.force_detach_workspace(100), None);
    }

    #[test]
    fn release_all_for_client_clears_workspace_and_surface() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_workspace(100, &[10, 11], &[10, 11, 12], 1)
            .unwrap();
        reg.acquire(20, 1).unwrap(); // 별도 surface 단위
        reg.acquire_workspace(200, &[30], &[30], 2).unwrap(); // 다른 client
        let mut released = reg.release_all_for_client(1);
        released.sort_unstable();
        // workspace 멤버(터미널 10/11 + 비-터미널 12) + surface 단위 20 전부 해제.
        assert_eq!(released, vec![10, 11, 12, 20]);
        assert!(!reg.is_content_hidden(10));
        assert!(!reg.is_content_hidden(12));
        assert!(!reg.is_hard_occupied(20));
        // 다른 client 의 workspace 는 유지.
        assert_eq!(reg.workspace_holder(200), Some(2));
        assert!(reg.is_hard_occupied(30));
    }

    // ─── soft 점유 (ADR-0040) ─────────────────────────────────────────────

    #[test]
    fn soft_occupancy_does_not_set_hard_predicate() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_soft(10, /*parent*/ 99, None).unwrap();
        assert!(!reg.is_hard_occupied(10)); // 입력차단/mirror 회귀 0
        assert_eq!(
            reg.occupancy_of(10).map(|o| o.tier),
            Some(OccupancyTier::Soft)
        );
        // hard 는 여전히 독립 동작.
        reg.acquire(11, 1).unwrap();
        assert!(reg.is_hard_occupied(11));
    }

    #[test]
    fn soft_occupancy_records_parent_and_label() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_soft(10, 99, Some("agent".into())).unwrap();
        let occ = reg.occupancy_of(10).unwrap();
        assert_eq!(occ.tier, OccupancyTier::Soft);
        assert_eq!(occ.parent, Some(99));
        assert_eq!(
            occ.holder,
            Holder::Subject {
                label: Some("agent".into())
            }
        );
    }

    #[test]
    fn soft_acquire_same_parent_idempotent_updates_label() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_soft(10, 99, None).unwrap();
        reg.acquire_soft(10, 99, Some("x".into())).unwrap(); // 같은 주체 재-acquire
        assert_eq!(
            reg.occupancy_of(10).unwrap().holder,
            Holder::Subject {
                label: Some("x".into())
            }
        );
    }

    #[test]
    fn soft_acquire_other_subject_rejected() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_soft(10, 99, None).unwrap();
        let err = reg.acquire_soft(10, 77, None).unwrap_err();
        assert_eq!(err, OccupancyError::AlreadyOccupied { parent: 99 });
    }

    #[test]
    fn soft_release_by_subject() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_soft(10, 99, None).unwrap();
        reg.release_soft(10, 99).unwrap();
        assert!(reg.occupancy_of(10).is_none());
    }

    #[test]
    fn soft_release_non_holder_rejected() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_soft(10, 99, None).unwrap();
        assert_eq!(
            reg.release_soft(10, 77).unwrap_err(),
            OccupancyError::NotHolder { parent: 99 }
        );
        assert!(reg.occupancy_of(10).is_some()); // 여전히 점유.
    }

    #[test]
    fn soft_release_not_occupied() {
        let mut reg = OccupancyRegistry::new();
        assert_eq!(
            reg.release_soft(10, 99).unwrap_err(),
            OccupancyError::NotOccupied
        );
    }

    #[test]
    fn clear_soft_removes_unconditionally() {
        // force-detach·focus 지연청소 tier 공용 primitive: 주체 검증 없이 제거.
        let mut reg = OccupancyRegistry::new();
        reg.acquire_soft(10, 99, None).unwrap();
        assert!(reg.clear_soft(10));
        assert!(reg.occupancy_of(10).is_none());
        assert!(!reg.clear_soft(10)); // 이미 없으면 false
    }

    #[test]
    fn hard_dominates_soft_in_occupancy_of() {
        // 같은 surface 에 soft 후 hard — occupancy_of 는 hard 를 보고(테두리 우선순위),
        // is_hard_occupied 도 true.
        let mut reg = OccupancyRegistry::new();
        reg.acquire_soft(10, 99, None).unwrap();
        reg.acquire(10, 1).unwrap();
        assert!(reg.is_hard_occupied(10));
        assert_eq!(
            reg.occupancy_of(10).map(|o| o.tier),
            Some(OccupancyTier::Hard)
        );
    }

    #[test]
    fn force_detach_pushes_to_notifier() {
        use crate::ipc::stream::StreamTag;
        let hub = StreamHub::new();
        let holder = hub.alloc_id();
        let rx = hub.register(holder);
        let mut reg = OccupancyRegistry::new();
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
    // ─── 같은 배치의 끊김과 재attach (순서 계약) ──────────────────────────
    //
    // 합성 픽스처만 쓴다(실제 서버·소켓 없음). 재현하려는 것이 *타이밍*이 아니라
    // pump 가 **정적으로 선언한 적용 순서**이기 때문이다 — `apply` 는 attach 결선을
    // 먼저, 끊김 정리를 마지막에 부른다. 그래서 한 배치에 "C1 끊김" 과 "C2 attach"
    // 가 함께 실리면, C2 의 acquire 는 항상 C1 의 lock 을 본다. 부하는 그 두 개가
    // 한 배치에 실릴 확률만 바꾼다 — 결함 자체는 확률이 아니다.

    /// 같은 배치에 실린 재attach 는 **이미 끊긴** holder 에게 막히지 않는다.
    #[test]
    fn a_dead_holder_does_not_block_a_workspace_reattach_in_the_same_batch() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_workspace(100, &[10, 11], &[10, 11, 12], 1)
            .unwrap();

        // pump 가 배치 머리에서 알려준다: client 1 은 이미 끊겼다.
        reg.mark_clients_disconnected(&[1]);

        let lock = reg
            .acquire_workspace(100, &[10, 11], &[10, 11, 12], 2)
            .expect("끊긴 holder 는 재attach 를 막지 못한다");
        assert_eq!(lock.holder, 2);
        // 인수인계가 절반만 되면 안 된다 — 터미널 lock 도 새 holder 여야 한다.
        assert_eq!(
            reg.holder(10),
            Some(2),
            "surface 10 의 holder 도 넘어와야 한다"
        );
        assert_eq!(
            reg.holder(11),
            Some(2),
            "surface 11 의 holder 도 넘어와야 한다"
        );
        // 비-터미널 멤버도 새 holder 아래로 넘어온다(반환값은 workspace id 가 아니라
        // 그 workspace 의 holder client id 다).
        assert_eq!(reg.workspace_holder_of(12), Some(2));
    }

    /// surface 단위 점유도 같다.
    #[test]
    fn a_dead_holder_does_not_block_a_surface_reattach_in_the_same_batch() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire(10, 1).unwrap();
        reg.mark_clients_disconnected(&[1]);
        assert_eq!(
            reg.acquire(10, 2)
                .expect("끊긴 holder 는 막지 못한다")
                .holder,
            2
        );
    }

    /// **대조군** — 살아 있는 holder 는 여전히 막는다. 이쪽이 무너지면 위 두 개는
    /// "점유를 없앴다" 는 뜻이 된다.
    #[test]
    fn a_live_holder_still_blocks_a_reattach() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_workspace(100, &[10], &[10], 1).unwrap();
        reg.acquire(20, 3).unwrap();
        // 끊긴 것은 *다른* client 다 — 표시가 있다고 아무나 뚫리면 안 된다.
        reg.mark_clients_disconnected(&[9]);
        assert_eq!(
            reg.acquire_workspace(100, &[10], &[10], 2).unwrap_err(),
            AttachError::AlreadyAttached { holder: 1 }
        );
        assert_eq!(
            reg.acquire(20, 4).unwrap_err(),
            AttachError::AlreadyAttached { holder: 3 }
        );
    }

    /// 배치 **끝**의 정리는 그 사이에 들어온 새 holder 를 쫓아내지 않는다.
    /// (`release_all_for_client` 는 holder 로 거르므로 성립한다 — 그 성질을 값으로 박는다.)
    #[test]
    fn the_batch_end_cleanup_does_not_evict_the_new_holder() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire_workspace(100, &[10], &[10, 12], 1).unwrap();
        reg.mark_clients_disconnected(&[1]);
        reg.acquire_workspace(100, &[10], &[10, 12], 2).unwrap();

        // 배치 마지막 단계: 끊긴 client 정리.
        reg.release_all_for_client(1);

        assert_eq!(reg.holder(10), Some(2), "새 holder 가 살아 있어야 한다");
        assert_eq!(reg.workspace_holder_of(12), Some(2));
    }

    /// "죽었다" 표시는 그 배치를 넘어 살지 않는다 — client_id 는 재사용된다.
    #[test]
    fn the_dead_mark_does_not_outlive_its_batch() {
        let mut reg = OccupancyRegistry::new();
        reg.acquire(10, 1).unwrap();
        reg.mark_clients_disconnected(&[1]);
        reg.release_all_for_client(1); // 배치 끝 정리 = 표시 소멸

        // 같은 번호를 다시 쓰는 새 연결이 정상 점유하고, 그 점유는 유효해야 한다.
        reg.acquire(10, 1).unwrap();
        assert_eq!(
            reg.acquire(10, 2).unwrap_err(),
            AttachError::AlreadyAttached { holder: 1 },
            "표시가 남아 있으면 살아 있는 holder 가 죽은 것으로 취급된다"
        );
    }
    /// **배선 가드** — 두 pump 가 실제로 배치 머리에서 표시하는가.
    ///
    /// 위 테스트들은 *규칙*(죽은 holder 는 못 막는다)을 고정한다. 그런데 규칙이 맞아도
    /// 두 이벤트 루프가 `mark_clients_disconnected` 를 안 부르면 아무것도 안 빨개진다 —
    /// 그 배선을 지우는 뮤테이션이 살아남는다는 뜻이다. 배선은 `App`/`AppState` 를
    /// 세워야 실행으로 볼 수 있어 단위 테스트로 잡히지 않으므로, **호출 순서를 소스에서**
    /// 읽어 고정한다(레포에 선례가 있다 — `crate::dpi_conversion_guard`).
    ///
    /// 이 가드가 잡는 것과 못 잡는 것을 갈라 둔다: **호출이 사라지거나 attach 결선
    /// 뒤로 밀리면** 잡는다. **다른 이름의 함수로 우회하면** 못 잡는다 — 그 경우는
    /// 위 규칙 테스트가 여전히 초록이므로 이 가드가 유일한 채널이었다는 뜻이고,
    /// 그래서 판정 대상 두 자리를 아래에 **이름으로** 못 박는다(파일이 사라지면 실패).
    #[test]
    fn both_pumps_mark_disconnects_before_applying_attach_requests() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        // (파일, 배치 적용 함수, 표시 호출, attach 결선 호출)
        let sites = [
            (
                "src/boot/headless_stream.rs",
                "fn apply(",
                "mark_clients_disconnected(",
                "apply_attach_requests(",
            ),
            (
                "src/app/event_handler.rs",
                "fn apply_stream_outcome(",
                "mark_disconnected_clients(",
                "apply_attach_requests_batch(",
            ),
        ];
        let mut checked = 0usize;
        for (rel, func, mark, attach) in sites {
            let src = std::fs::read_to_string(root.join(rel))
                .unwrap_or_else(|e| panic!("{rel} 를 못 읽었다 — 파일이 옮겨졌나: {e}"));
            let start = src
                .find(func)
                .unwrap_or_else(|| panic!("{rel} 에 `{func}` 가 없다 — 적용 함수가 바뀌었다"));
            let body = &src[start..];
            let m = body
                .find(mark)
                .unwrap_or_else(|| panic!("{rel}::{func} 가 `{mark}` 를 부르지 않는다 — 같은 배치의 재attach 가 죽은 holder 에게 막힌다"));
            let a = body
                .find(attach)
                .unwrap_or_else(|| panic!("{rel}::{func} 에서 `{attach}` 를 못 찾았다"));
            assert!(
                m < a,
                "{rel}::{func}: 끊김 표시가 attach 결선보다 뒤에 있다(표시 {m} > 결선 {a}) — \
                 순서가 뒤집히면 이 규칙은 아무것도 막지 못한다"
            );
            checked += 1;
        }
        assert_eq!(checked, 2, "두 조합의 pump 를 모두 봐야 한다");
    }
}
