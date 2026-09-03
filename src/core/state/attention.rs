//! Surface "Attention" 상태 조작/조회. `AttentionStore` 는 producer 중립 공유
//! primitive — toast 알림, completion IPC/CLI, OSC 133 명령 완료 등 여러 producer 가
//! `raise_attention` 으로 발동하고, surface 가 실제 렌더 시점 포커스를 얻으면
//! (`gpu.rs`) `clear_attention` 으로 해제된다. 세 소비처(테두리·탭 제목·워크스페이스
//! 개수 배지)가 이 상태를 읽는다. `state/busy.rs` 의 조회 헬퍼 형태를 1:1 미러한다.
//!
//! 단, **mirror(원격 attach) surface 는 로컬 producer 의 대상이 아니다** — 그 값은
//! 서버 push 만을 소스로 갖고 `set_mirror_surface_attention` 으로만 들어온다.
//! `raise_attention` 이 그 게이트를 집행한다.
//!
//! `AttentionStore` 는 `NotificationStore` 와 별개다 — attention 레코드가 곧 패널
//! 아이템은 아니다. 패널 노출 여부는 kind 별 정책(`effects_of` 의 `panel_item`)이
//! 결정하며, 실제 패널 아이템 생성은 지금처럼 producer 가 `notifications.add()` 를
//! 직접 호출해 만든다(이 TODO 는 순수 구조 이관이라 그 호출 여부 자체를 바꾸지
//! 않는다). OSC 133 명령 완료는 `notifications.add()` 를 호출하지 않으므로 패널에
//! 아이템이 쌓이지 않은 채로도 attention 레코드(및 그 파생 효과인 테두리·탭 제목)만
//! 발동하는 조합이 성립한다.

use std::collections::HashMap;
use std::time::Instant;

use super::CoreState;

/// Attention 을 유발한 사건의 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttentionKind {
    /// 작업 완료 신호 — toast 알림, `surface.completion` IPC/CLI, windows resume
    /// 알림, OSC 133 명령 완료 producer 가 이 kind 로 발동한다.
    Completion,
    /// 응답 대기 신호 — Claude 플러그인의 `notification`(비-`idle_prompt`)/
    /// `pre-tool-use`(AskUserQuestion) 훅이 이 kind 로 발동한다. `Completion` 보다
    /// 우선순위가 높다(디자인 rank 30 > 10) — 지금 답하지 않으면 진행이 멈추는
    /// 상태가, 이미 끝난 작업 확인보다 더 급하기 때문.
    NeedsInput,
}

impl AttentionKind {
    /// attach 스트림 wire 표현으로 변환(server→client push). `tasty-ipc` 는 host
    /// crate 를 의존하지 않고 이 enum 은 `pub(crate)` 라, 경계에서 서로 변환한다.
    pub(crate) fn to_wire(self) -> tasty_ipc::stream::AttentionKindWire {
        match self {
            AttentionKind::Completion => tasty_ipc::stream::AttentionKindWire::Completion,
            AttentionKind::NeedsInput => tasty_ipc::stream::AttentionKindWire::NeedsInput,
        }
    }

    /// wire 표현에서 복원(client 적용). `to_wire` 의 역.
    pub(crate) fn from_wire(wire: tasty_ipc::stream::AttentionKindWire) -> Self {
        match wire {
            tasty_ipc::stream::AttentionKindWire::Completion => AttentionKind::Completion,
            tasty_ipc::stream::AttentionKindWire::NeedsInput => AttentionKind::NeedsInput,
        }
    }
}

/// surface 하나가 가진 attention 레코드. `raised_at` 은 지금은 소비처가 없지만
/// 향후 kind 별 만료/정렬 정책을 위해 확장 여지로 둔다.
#[derive(Debug, Clone, Copy)]
struct AttentionRecord {
    kind: AttentionKind,
    #[allow(dead_code)] // 확장 여지 — 현재 소비처 없음(향후 kind 별 만료/정렬 정책이 소비할 필드).
    raised_at: Instant,
}

/// 색 우선순위 등급 — 디자인 rank 토큰(`--tasty-attention-rank-*`)을 그대로
/// 미러링한다(재도출 금지). 선언 순서가 곧 derived `Ord` 순서이므로 값이 낮은
/// 쪽을 먼저 선언한다. 탭 제목·collapsed rail dot 처럼 여러 surface 를 하나의
/// 색으로 압축해야 하는 소비처가 이 순서로 대표 kind 를 고른다
/// (`CoreState::attention_dominant_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AttentionLevel {
    /// `--tasty-attention-rank-completion` = 10.
    Completion,
    /// `--tasty-attention-rank-needs-input` = 30.
    NeedsInput,
}

/// kind → 효과. `effects_of` 가 이 값을 만들고, 호출부는 그 결과를 집행만 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AttentionEffects {
    /// 색 우선순위(현재는 소비처 없음 — `attention-needs-input-visuals` 가 테두리/배지
    /// 색 분기에 쓴다).
    pub(crate) level: AttentionLevel,
    /// 알림 패널 노출 여부 정책. attention 레코드 자체는 패널 아이템을 만들지
    /// 않는다 — 패널 노출이 필요한 producer(toast, windows resume)는 지금처럼
    /// `NotificationStore` 를 별도로 직접 호출한다.
    pub(crate) panel_item: bool,
    /// OS 네이티브 알림 발동 여부 정책. 이 레포에 아직 그 개념이 없어 항상 `false` —
    /// 향후 실제 소비처가 생기기 전까지는 값을 읽는 곳이 없다.
    pub(crate) os_notify: bool,
    /// 알림음 재생 여부 정책. 현재 toast producer 의 사운드 발동은 사용자 설정
    /// (`settings.notification.sound`) + bell-source 제외 게이트로 별도 결정되므로,
    /// 이 필드가 그 판단을 대체하지 않는다(대체 시 설정 게이트가 사라지는 회귀).
    pub(crate) sound: bool,
}

/// kind → 효과 정책. host/cascade 에 의존하지 않는 순수 함수 — 단위 테스트가
/// 분기 동작을 직접 검증한다(`crates/tasty-plugin-claude/src/hook.rs::apply_hook`
/// 과 동형 패턴). cascade 는 이 결과를 집행만 한다.
pub(crate) fn effects_of(kind: AttentionKind) -> AttentionEffects {
    match kind {
        AttentionKind::Completion => AttentionEffects {
            level: AttentionLevel::Completion,
            panel_item: false,
            os_notify: false,
            sound: false,
        },
        AttentionKind::NeedsInput => AttentionEffects {
            level: AttentionLevel::NeedsInput,
            // Completion 과 동일 정책 — 이 리포에 panel_item/os_notify/sound 의
            // 실제 소비처가 아직 없다(ADR-0062). 값이 생기면 그때 분기한다.
            panel_item: false,
            os_notify: false,
            sound: false,
        },
    }
}

/// Producer-neutral attention record store — surface 당 최대 1개 레코드.
/// `NotificationStore` 와 구조적으로 대응하되(둘 다 `CoreState` 가 보유) 서로
/// 독립이다.
#[derive(Debug, Default)]
pub(crate) struct AttentionStore {
    records: HashMap<u32, AttentionRecord>,
}

impl AttentionStore {
    fn raise(&mut self, surface_id: u32, kind: AttentionKind) {
        if surface_id != 0 {
            self.records.insert(
                surface_id,
                AttentionRecord {
                    kind,
                    raised_at: Instant::now(),
                },
            );
        }
    }

    /// 레코드를 제거하고 **실제로 제거했는지**를 돌려준다. 이 `true` 가 곧 해제
    /// edge 다 — 레코드가 없는 상태의 호출은 no-op(`false`)이라, 매 프레임 도는
    /// 호출부에서도 상태가 바뀐 순간에만 신호가 나간다.
    fn clear(&mut self, surface_id: u32) -> bool {
        self.records.remove(&surface_id).is_some()
    }

    fn kind_of(&self, surface_id: u32) -> Option<AttentionKind> {
        self.records.get(&surface_id).map(|r| r.kind)
    }

    fn count_of_kind(&self, kind: AttentionKind, surface_ids: &[u32]) -> usize {
        surface_ids
            .iter()
            .filter(|sid| self.records.get(sid).map(|r| r.kind) == Some(kind))
            .count()
    }

    /// 주어진 surface 목록 중 가장 높은 우선순위(`AttentionLevel`)를 가진 kind.
    /// 한 surface 는 kind 하나만 갖지만, 목록(탭의 여러 surface, 워크스페이스의
    /// 여러 surface)에는 서로 다른 kind 가 섞여 있을 수 있다 — 이 값이 그 목록을
    /// 대표하는 색 하나를 고른다.
    fn dominant_kind(&self, surface_ids: &[u32]) -> Option<AttentionKind> {
        surface_ids
            .iter()
            .filter_map(|sid| self.records.get(sid).map(|r| r.kind))
            .max_by_key(|k| effects_of(*k).level)
    }
}

impl CoreState {
    /// Mark a surface as needing attention. Called by any producer (toast,
    /// completion, OSC 133, …). `surface_id == 0`(=미지정) 은 무시한다.
    ///
    /// `effects_of(kind)` 정책을 여기 한 곳에서 조회한다 — `panel_item`/`os_notify`/
    /// `sound` 를 kind 별로 분기할 자리다.
    ///
    /// **mirror surface 는 대상에서 제외된다.** attention 의 진실 원천은 surface 를
    /// 소유한 인스턴스이고 미러는 서버 push 를 반영만 하므로
    /// ([`set_mirror_surface_attention`](Self::set_mirror_surface_attention)),
    /// 로컬 producer 가 미러에 자기 레코드를 만들면 같은 사건에 서버·미러가 각각
    /// 별개 레코드를 갖는 이중 상태가 된다. 미러 터미널도 서버가 흘려준 바이트를
    /// 그대로 파싱하므로 OSC 133 D·Bell·OSC 9/777 이 미러에서도 발화하는데, 그
    /// 사건은 서버에서도 똑같이 발화해 push 로 내려오므로 억제해도 정보가 사라지지
    /// 않는다. 게이트를 producer cascade 가 아니라 **이 단일 진입점**에 두어 네
    /// producer(OSC 133 자동 경로 · 알림 생성 cascade · `surface.completion` IPC/CLI ·
    /// Windows resume 헬스 패스)와 앞으로 추가될 producer 까지 한 번에 덮는다.
    /// 억제 대상은 attention 레코드 하나뿐이다 — 같은 cascade 의 알림 패널 아이템·
    /// 토스트·훅 발화는 이 게이트 밖이라 미러에서도 그대로 동작한다.
    /// 상세: `docs/features/surface-highlight/index.md` "Producer".
    pub(crate) fn raise_attention(&mut self, surface_id: u32, kind: AttentionKind) {
        if self.is_mirror_surface(surface_id) {
            tracing::trace!(
                surface_id,
                kind = ?kind,
                "attention raise suppressed — mirror surface (server push is the only source)"
            );
            return;
        }
        let effects = effects_of(kind);
        tracing::trace!(
            surface_id,
            kind = ?kind,
            level = ?effects.level,
            panel_item = effects.panel_item,
            os_notify = effects.os_notify,
            sound = effects.sound,
            "attention raised"
        );
        self.attention.raise(surface_id, kind);
    }

    /// Clear the attention record for a surface (e.g. when it gains focus).
    /// **실제로 레코드를 제거했으면 `true`** — 이 값이 해제 edge 다.
    ///
    /// mirror surface 에서 edge 가 발생하면 그 사실을
    /// [`pending_attention_clear_forward`](crate::core::CoreState::pending_attention_clear_forward)
    /// 에 넣어 서버(surface 소유 인스턴스)로 전달되게 한다. 큐 push 를 호출부가
    /// 아니라 **이 함수 안에서** 하는 이유는 해제 producer 가 여럿이기 때문이다 —
    /// 실-포커스 해제(`gpu.rs`)뿐 아니라 미러 로컬 알림 패널의 읽음 처리
    /// (`mark_notification_read`/`mark_all_notifications_read`)도 미러에서 일어날 수
    /// 있고, 포커스 경로에만 큐잉하면 알림으로 확인한 경우가 서버에 전달되지 않아
    /// 다음 push 에서 배지가 되살아난다. 여기 두면 세 경로가 균일하게 덮이고 해제
    /// producer 가 늘어도 누락이 생기지 않는다.
    ///
    /// 해제 규칙 자체는 바뀌지 않는다 — 판정은 여전히 인스턴스 로컬 사용자 행동
    /// (실 렌더 포커스 / 알림 읽음)이고, 이 큐는 그 결과만 소유 인스턴스로 옮긴다.
    /// 서버 push 를 적용하는 [`set_mirror_surface_attention`](Self::set_mirror_surface_attention)
    /// 과 teardown 용 [`forget_mirror_surface_attention`](Self::forget_mirror_surface_attention)
    /// 은 이 함수를 타지 않으므로 에코가 생기지 않는다.
    pub fn clear_attention(&mut self, surface_id: u32) -> bool {
        let removed = self.attention.clear(surface_id);
        if removed && self.is_mirror_surface(surface_id) {
            tracing::trace!(
                surface_id,
                "mirror attention cleared — queueing clear forward to the owning instance"
            );
            self.pending_attention_clear_forward.insert(surface_id);
        }
        removed
    }

    /// 로컬 사용자 사건이 이 surface 의 attention 을 해제할 수 있는가 — 하드 점유
    /// 게이트의 술어. **하드 점유(attach) 중이면 그 surface 의 주체는 홀더이고 로컬
    /// 사용자는 readonly 이므로**(ADR-0040), "확인했다" 는 판정도 홀더의 것이다.
    /// 홀더의 확인은 `ClientAttentionClear` 로 들어와
    /// [`apply_attached_attention_clear`](crate::core::CoreState::apply_attached_attention_clear)
    /// 가 적용한다(ADR-0104).
    ///
    /// **soft 점유는 대상이 아니다** — soft 는 로컬 사용자를 배제하지 않으므로
    /// (ADR-0040 "write 제한 없음") 술어가 `is_hard_occupied` 하나뿐이다.
    ///
    /// 렌더 경로(`gpu.rs`)는 GPU 없이 실행할 수 없어, 게이트 판정만 이렇게 떼어
    /// 단위 테스트가 직접 검증한다(`effects_of` 와 같은 형태).
    pub(crate) fn local_attention_clear_allowed(&self, surface_id: u32) -> bool {
        !self.attach.is_hard_occupied(surface_id)
    }

    /// 이 인스턴스의 **로컬 사용자 사건**(실 렌더 포커스 · 알림 읽음)에 의한 해제
    /// 진입점. 해제 호출부 셋 전부가 이 함수를 타고, 하드 점유 게이트
    /// ([`local_attention_clear_allowed`](Self::local_attention_clear_allowed))가
    /// 여기서 한 번만 걸린다.
    ///
    /// 게이트를 [`clear_attention`](Self::clear_attention) **안이 아니라 이 래퍼에**
    /// 두는 것이 핵심이다 — 홀더의 해제를 적용하는 서버측 경로
    /// (`apply_attached_attention_clear` → `clear_attention`)까지 막히면 점유 중
    /// 해제 주체가 다시 0 이 된다(그 요청자는 이미 holder 로 검증된 뒤다).
    /// 그래서 `clear_attention` 은 게이트 없는 primitive 로 남고, 로컬 축만 이
    /// 래퍼를 지난다. 근거: `docs/adr/0109-hard-occupancy-attention-clear-holder-only.md`.
    ///
    /// 미러 인스턴스에서는 이 게이트가 걸리지 않는다 — 미러 surface 는 그 인스턴스의
    /// `OccupancyRegistry` 에 lock 이 없다(점유는 surface 를 **소유한** 인스턴스가
    /// 기록한다). 미러 사용자의 확인은 그대로 `clear_attention` 의 제거 edge 를 만들어
    /// 서버로 forward 된다(ADR-0104).
    pub(crate) fn clear_attention_local(&mut self, surface_id: u32) -> bool {
        if !self.local_attention_clear_allowed(surface_id) {
            tracing::trace!(
                surface_id,
                "attention clear skipped — hard-occupied surface (only the holder may clear)"
            );
            return false;
        }
        self.clear_attention(surface_id)
    }

    /// **원격(서버) push 반영 전용 진입점** — attach mirror 가 받은
    /// `StreamControl::Attention` 을 자기 `AttentionStore` 에 그대로 쓴다.
    /// `kind == None` 은 해제.
    ///
    /// 로컬 producer 의 `raise_attention`/`clear_attention` 을 **일부러 타지 않는다.**
    /// 그 두 API 는 로컬 producer 축이다 — [`raise_attention`](Self::raise_attention)
    /// 에는 mirror surface 를 대상으로 한 **로컬 raise 억제 게이트가 이미 있고**,
    /// `clear_attention` 은 해제 forward(mirror→server)가 붙을 자리다. 서버가 내려준
    /// 값을 적용하면서 같은 함수를 타면 그 억제 게이트에 자기 push 가 막히고, 서버가
    /// 내려준 해제는 곧바로 서버로 되돌아가는 에코가 된다.
    ///
    /// 저장 위치는 busy 와 다르다 — busy 는 `refresh_busy_surfaces` 가 매 tick
    /// `busy_surfaces` 를 통째로 교체하므로 mirror 값을 `mirror_busy_surfaces` 별도
    /// 집합에 둬야 했지만, attention 에는 그런 wholesale 교체가 없다. 그래서 push 된
    /// 값을 **기존 `AttentionStore` 에 그대로** 넣는다 — 사이드바 배지·테두리·탭
    /// 제목 소비처가 코드 변경 없이 그대로 읽는다.
    pub(crate) fn set_mirror_surface_attention(
        &mut self,
        surface_id: u32,
        kind: Option<AttentionKind>,
    ) {
        match kind {
            Some(k) => self.attention.raise(surface_id, k),
            None => {
                // 반환값(제거 edge)은 여기서 의미가 없다 — 서버가 내려준 해제를
                // 그대로 적용하는 것이라 서버로 되돌려 보낼 edge 가 아니다.
                self.attention.clear(surface_id);
            }
        }
    }

    /// mirror surface 가 사라질 때 그 attention 레코드를 버린다(세션 정리 / 구조
    /// delta 에서 제거된 surface). `forget_mirror_surface_busy` 동형 — 로컬 id 가
    /// 재사용될 때 stale attention 이 새 surface 에 잘못 붙는 것을 막는다.
    ///
    /// `clear_attention` 이 아니라 별도 진입점인 이유는
    /// [`set_mirror_surface_attention`](Self::set_mirror_surface_attention) 과 같다 —
    /// teardown 은 로컬 사용자의 해제가 아니라 surface 소멸이므로 해제 forward 축을
    /// 타면 안 된다.
    pub(crate) fn forget_mirror_surface_attention(&mut self, surface_id: u32) {
        // 제거 edge 를 무시한다 — surface 소멸이지 사용자의 "확인" 이 아니라
        // 해제 forward 축(`clear_attention`)을 타면 안 된다.
        self.attention.clear(surface_id);
    }

    /// 점유(hard attach) 중인 surface 의 attention 변화분 — `(holder client,
    /// surface, kind)` 튜플. `kind == None` 은 해제. `busy_activity_forwards`
    /// (`state/busy.rs`) 와 동형이며 같은 1Hz tick 에 편승한다.
    ///
    /// `last_forwarded_attention` 캐시와 비교해 **값이 실제로 바뀐 것만** 내보낸다
    /// (스팸 억제). 캐시가 아니라 항상 live store 에서 재-diff 하므로, 프레임이
    /// 유실되거나 지연돼도 다음 tick 에 자동 수렴한다(client ack 에 의존하지 않는다).
    /// 이 수렴은 **wire 유실 축에만** 유효하다 — client 가 자기 store 를 로컬로 바꾸면
    /// 서버 값은 그대로라 재-push 가 없다. 그 축은 이 diff 가 아니라 전용 장치 둘이
    /// 맞춘다: 미러의 **발동**은 [`raise_attention`](Self::raise_attention) 게이트가 막고,
    /// **해제**는 [`clear_attention`](Self::clear_attention) 의 제거 edge 가 서버로
    /// forward 된다. 상세는 `docs/dev-guide/attach-behavior.md` "주의 환기(attention) 전파".
    /// 점유가 풀린 surface 의 엔트리는 매 호출 정리해, 나중에 재attach(다른 client 일
    /// 수 있음) 하면 값이 이전과 같아도 baseline push 를 다시 받는다.
    ///
    /// 첫 호출은 attention 이 없는 surface 에 대해서도 `None` baseline 을 1회
    /// 내보낸다 — busy 가 초기 `false` 를 내보내는 것과 같은 성질이고, mirror 쪽
    /// 초기 상태를 서버 기준으로 확정시킨다(client 에는 무해한 no-op 해제).
    pub(crate) fn attention_forwards(
        &mut self,
    ) -> Vec<(
        crate::core::attach::AttachClientId,
        u32,
        Option<AttentionKind>,
    )> {
        let locks = self.attach.locks_snapshot();
        let occupied: std::collections::HashSet<u32> = locks.iter().map(|&(sid, _)| sid).collect();
        self.last_forwarded_attention
            .retain(|sid, _| occupied.contains(sid));
        let mut out = Vec::new();
        for (sid, lock) in locks {
            let kind = self.attention.kind_of(sid);
            if self.last_forwarded_attention.get(&sid) != Some(&kind) {
                self.last_forwarded_attention.insert(sid, kind);
                out.push((lock.holder, sid, kind));
            }
        }
        out
    }

    /// The attention kind currently recorded for a surface, if any.
    pub(crate) fn attention_kind(&self, surface_id: u32) -> Option<AttentionKind> {
        self.attention.kind_of(surface_id)
    }

    /// Number of surfaces with an attention record of the given kind among the
    /// given list. 워크스페이스 행의 kind 별 배지 2종(NeedsInput/Completion)이
    /// 각각 이 API 를 호출한다(`sidebar/full.rs::entry_view`).
    pub(crate) fn attention_count_of_kind(
        &self,
        kind: AttentionKind,
        surface_ids: &[u32],
    ) -> usize {
        self.attention.count_of_kind(kind, surface_ids)
    }

    /// 목록(탭/워크스페이스에 속한 surface) 중 가장 높은 우선순위의 attention
    /// kind — `NeedsInput > Completion` 순서(디자인 rank 토큰 미러링). 탭 제목·
    /// collapsed rail dot 처럼 "여러 surface 를 하나의 색으로 압축" 해야 하는
    /// 소비처 전용(`tab_bar.rs`, `sidebar/view.rs` collapsed dot).
    pub fn attention_dominant_kind(&self, surface_ids: &[u32]) -> Option<AttentionKind> {
        self.attention.dominant_kind(surface_ids)
    }

    /// 알림 읽음 처리(ADR-0039 Reconsideration Triggers 참고) — 두 번째 clear producer.
    /// 특정 알림을 읽음 처리하고,
    /// 그 알림의 source surface 를 source 로 하는 다른 안읽음 알림이 남아있지 않은
    /// 경우에만 attention 을 지운다. 같은 surface 의 다른 알림이 아직 안읽음이면
    /// clear 하지 않는다(엣지 케이스 — 무조건 clear 시 오해제 발생).
    ///
    /// **알림의 읽음 플래그와 attention 해제는 분리된다.** 해제는 로컬 축이라
    /// [`clear_attention_local`](Self::clear_attention_local) 의 하드 점유 게이트를
    /// 지나므로, 점유 중 surface 의 알림을 읽어도 attention 은 유지된다 — 알림 자체는
    /// 점유와 무관하게 읽음 처리된다(읽음은 이 인스턴스 사용자의 알림 패널 상태이고,
    /// attention 은 홀더와 공유하는 상태다).
    pub(crate) fn mark_notification_read(&mut self, id: u64) {
        let source_surface = self
            .notifications
            .all()
            .find(|n| n.id == id)
            .map(|n| n.source_surface);
        self.notifications.mark_read(id);
        if let Some(surface_id) = source_surface {
            if !self.notifications.has_unread_for_surface(surface_id) {
                self.clear_attention_local(surface_id);
            }
        }
    }

    /// 모든 알림 읽음 처리(ADR-0039 Reconsideration Triggers 참고). 전부 읽음
    /// 처리되므로 엣지 케이스 없이, 읽음
    /// 처리 전 안읽음이었던 모든 알림의 source surface attention 을 지운다.
    ///
    /// 해제 대상에서 **하드 점유 중인 surface 는 빠진다** —
    /// [`clear_attention_local`](Self::clear_attention_local) 게이트가 걸러낸다.
    /// 게이트를 여기서 따로 필터링하지 않고 그 진입점에 맡기는 이유는 해제 producer
    /// 셋이 같은 규칙을 공유해야 하기 때문이다(`mark_notification_read` 와 실-포커스
    /// 해제도 같은 함수를 지난다). "모두 읽음" 한 번으로 점유 중 배지가 전부
    /// 사라지는 구멍이 이 게이트로 막힌다.
    pub(crate) fn mark_all_notifications_read(&mut self) {
        let unread_surfaces: std::collections::HashSet<u32> = self
            .notifications
            .all()
            .filter(|n| !n.read)
            .map(|n| n.source_surface)
            .collect();
        self.notifications.mark_all_read();
        for surface_id in unread_surfaces {
            self.clear_attention_local(surface_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AttentionKind, AttentionLevel, effects_of};
    use crate::core::CoreState;

    fn state() -> CoreState {
        let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
        CoreState::new(80, 24, waker).expect("engine")
    }

    /// 같은 source_surface 로 연달아 `add()` 해도 coalesce(기본 500ms 창)되지 않게
    /// coalesce window 를 0 으로 둔 state. 한 surface 에서 온 알림 2건 이상을
    /// 별개 엔트리로 만들어야 하는 엣지 케이스 테스트 전용.
    fn state_no_coalesce() -> CoreState {
        let mut s = state();
        s.notifications = crate::notification::NotificationStore::with_coalesce_ms(0);
        s
    }

    #[test]
    fn raise_and_query() {
        let mut s = state();
        assert!(!s.attention_dominant_kind(&[7]).is_some());
        s.raise_attention(7, AttentionKind::Completion);
        assert!(s.attention_dominant_kind(&[7]).is_some());
        assert_eq!(s.attention_kind(7), Some(AttentionKind::Completion));
        assert!(!s.attention_dominant_kind(&[8, 9]).is_some());
    }

    #[test]
    fn raise_ignores_zero() {
        let mut s = state();
        s.raise_attention(0, AttentionKind::Completion);
        assert!(!s.attention_dominant_kind(&[0]).is_some());
        assert_eq!(
            s.attention_count_of_kind(AttentionKind::Completion, &[0]),
            0
        );
    }

    #[test]
    fn clear_removes() {
        let mut s = state();
        s.raise_attention(3, AttentionKind::Completion);
        s.clear_attention(3);
        assert!(!s.attention_dominant_kind(&[3]).is_some());
        assert_eq!(s.attention_kind(3), None);
    }

    #[test]
    fn count_over_list() {
        let mut s = state();
        s.raise_attention(1, AttentionKind::Completion);
        s.raise_attention(2, AttentionKind::Completion);
        s.raise_attention(5, AttentionKind::Completion);
        assert_eq!(
            s.attention_count_of_kind(AttentionKind::Completion, &[1, 2, 3, 4, 5]),
            3
        );
        assert_eq!(
            s.attention_count_of_kind(AttentionKind::Completion, &[3, 4]),
            0
        );
    }

    /// 개별 읽음 처리 시 그 surface 에 다른 안읽음 알림이 남아있지 않으면
    /// attention 이 지워진다(ADR-0039 Reconsideration Triggers 참고).
    #[test]
    fn mark_notification_read_clears_attention_when_no_unread_left() {
        let mut s = state();
        let id = s.notifications.add(1, 100, "t".into(), "b".into()).unwrap();
        s.raise_attention(100, AttentionKind::Completion);
        assert!(s.attention_dominant_kind(&[100]).is_some());

        s.mark_notification_read(id);

        assert!(!s.attention_dominant_kind(&[100]).is_some());
    }

    /// 핵심 엣지 케이스(ADR-0039 Reconsideration Triggers 참고) — 같은 surface 에서
    /// 온 다른 알림이 아직 안읽음이면 하나만 읽음 처리해도 attention 이 지워지면
    /// 안 된다.
    #[test]
    fn mark_notification_read_keeps_attention_when_sibling_unread_remains() {
        let mut s = state_no_coalesce();
        let id1 = s
            .notifications
            .add(1, 100, "t1".into(), "b1".into())
            .unwrap();
        let _id2 = s
            .notifications
            .add(1, 100, "t2".into(), "b2".into())
            .unwrap();
        s.raise_attention(100, AttentionKind::Completion);

        s.mark_notification_read(id1);

        assert!(
            s.attention_dominant_kind(&[100]).is_some(),
            "다른 알림(id2)이 아직 안읽음이므로 attention 이 유지돼야 한다"
        );

        s.mark_notification_read(_id2);
        assert!(
            !s.attention_dominant_kind(&[100]).is_some(),
            "마지막 안읽음 알림까지 읽음 처리되면 attention 이 지워져야 한다"
        );
    }

    /// 존재하지 않는 알림 id 를 넘겨도 panic 없이 no-op.
    #[test]
    fn mark_notification_read_unknown_id_is_noop() {
        let mut s = state();
        s.raise_attention(100, AttentionKind::Completion);
        s.mark_notification_read(9999);
        assert!(s.attention_dominant_kind(&[100]).is_some());
    }

    /// "모두 읽음"은 엣지 케이스 없이 안읽음이었던 모든 surface 의 attention 을
    /// 지운다.
    #[test]
    fn mark_all_notifications_read_clears_all_unread_surfaces() {
        let mut s = state_no_coalesce();
        s.notifications.add(1, 100, "t1".into(), "b1".into());
        s.notifications.add(1, 100, "t2".into(), "b2".into());
        s.notifications.add(1, 200, "t3".into(), "b3".into());
        s.raise_attention(100, AttentionKind::Completion);
        s.raise_attention(200, AttentionKind::Completion);

        s.mark_all_notifications_read();

        assert!(!s.attention_dominant_kind(&[100]).is_some());
        assert!(!s.attention_dominant_kind(&[200]).is_some());
    }

    /// 회귀 방지 — 이미 읽은 알림만 있는 surface 의 attention 은
    /// `mark_all_notifications_read` 가 건드리지 않아도 원래 그 surface 는 안읽음
    /// 집합에서 제외되므로 clear 대상에 포함되지 않는다(다른 surface 의 attention 은
    /// 보존).
    #[test]
    fn mark_all_notifications_read_leaves_unrelated_surface_attention_untouched() {
        let mut s = state();
        let id = s.notifications.add(1, 100, "t".into(), "b".into()).unwrap();
        s.notifications.mark_read(id); // 이미 읽음 처리된 알림
        s.raise_attention(100, AttentionKind::Completion); // 알림과 무관한 producer(toast 등)가 건 attention
        s.raise_attention(200, AttentionKind::Completion);
        s.notifications.add(1, 200, "t2".into(), "b2".into());

        s.mark_all_notifications_read();

        assert!(
            s.attention_dominant_kind(&[100]).is_some(),
            "100 은 안읽음 알림이 없었으므로 clear 대상이 아니다 — 무관 producer 의 attention 보존"
        );
        assert!(!s.attention_dominant_kind(&[200]).is_some());
    }

    /// `attention_forwards` 는 값이 실제로 바뀔 때만 forward 한다(중복 억제) —
    /// `busy_activity_forwards_only_on_change` 미러. 첫 호출은 attention 이 없어도
    /// `None` baseline 을 1회 내보내 mirror 초기 상태를 서버 기준으로 확정한다.
    #[test]
    fn attention_forwards_only_on_change() {
        let mut e = state();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock 획득");

        // 최초 호출: attention 없음 → 최초 diff(캐시 없음 → None)라 baseline 1건.
        assert_eq!(e.attention_forwards(), vec![(7, sid, None)]);

        // 같은 상태 재호출 — 변화 없으니 forward 없음.
        assert!(e.attention_forwards().is_empty());

        // raise → 1건 forward.
        e.raise_attention(sid, AttentionKind::NeedsInput);
        assert_eq!(
            e.attention_forwards(),
            vec![(7, sid, Some(AttentionKind::NeedsInput))]
        );
        assert!(e.attention_forwards().is_empty());

        // 같은 surface 의 kind 만 바뀌어도 forward — 미러의 배지 색이 갈린다.
        e.raise_attention(sid, AttentionKind::Completion);
        assert_eq!(
            e.attention_forwards(),
            vec![(7, sid, Some(AttentionKind::Completion))]
        );
        assert!(e.attention_forwards().is_empty());

        // 해제 → "kind 없음"으로 1건 forward(별도 변형이 아니라 None).
        e.clear_attention(sid);
        assert_eq!(e.attention_forwards(), vec![(7, sid, None)]);
        assert!(e.attention_forwards().is_empty());
    }

    /// 점유되지 않은 surface 는 attention 이 있어도 forward 대상이 아니다 —
    /// attach client 가 없는 surface 의 상태를 흘리지 않는다.
    #[test]
    fn attention_forwards_ignores_unoccupied_surfaces() {
        let mut e = state();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.raise_attention(sid, AttentionKind::NeedsInput);
        assert!(e.attention_forwards().is_empty());
    }

    /// lock 해제 후 재획득(다른 client)하면, 값이 이전과 같아도 항상 fresh 하게 1건
    /// forward 해야 한다 — stale 캐시로 신규 holder 가 baseline 을 못 받는 회귀를
    /// 막는다(`busy_activity_forwards_resets_on_reacquire` 미러).
    #[test]
    fn attention_forwards_resets_on_reacquire() {
        let mut e = state();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock 획득");
        e.raise_attention(sid, AttentionKind::NeedsInput);
        assert_eq!(
            e.attention_forwards(),
            vec![(7, sid, Some(AttentionKind::NeedsInput))]
        );
        assert!(e.attention_forwards().is_empty());

        e.attach.release(sid, 7).expect("release");
        // 점유 해제 — 다음 diff 호출에서 캐시가 정리된다(occupied 집합에서 빠짐).
        assert!(e.attention_forwards().is_empty());

        e.attach.acquire(sid, 9).expect("다른 client 재획득");
        assert_eq!(
            e.attention_forwards(),
            vec![(9, sid, Some(AttentionKind::NeedsInput))],
            "재획득 후에는 값이 이전과 같아도 새 holder 에게 다시 push"
        );
    }

    /// 원격 push 반영 진입점은 로컬 producer API 와 분리되어 있지만, 결과는 같은
    /// `AttentionStore` 에 들어가야 한다 — 사이드바 배지·테두리·탭 제목 소비처가
    /// 코드 변경 없이 그대로 읽는 것이 이 설계의 요점이다.
    #[test]
    fn mirror_attention_lands_in_the_same_store_consumers_read() {
        let mut s = state();
        s.set_mirror_surface_attention(11, Some(AttentionKind::NeedsInput));
        assert_eq!(s.attention_kind(11), Some(AttentionKind::NeedsInput));
        assert_eq!(
            s.attention_count_of_kind(AttentionKind::NeedsInput, &[11]),
            1
        );
        assert_eq!(
            s.attention_dominant_kind(&[11]),
            Some(AttentionKind::NeedsInput)
        );

        // kind 없음 = 해제.
        s.set_mirror_surface_attention(11, None);
        assert_eq!(s.attention_kind(11), None);
    }

    /// mirror surface teardown 은 attention 레코드도 함께 버린다 — 로컬 id 가
    /// 재사용될 때 stale attention 이 새 surface 에 잘못 붙지 않아야 한다.
    #[test]
    fn forget_mirror_surface_attention_drops_the_record() {
        let mut s = state();
        s.set_mirror_surface_attention(12, Some(AttentionKind::Completion));
        s.forget_mirror_surface_attention(12);
        assert_eq!(s.attention_kind(12), None);
    }

    /// 미러가 서버 push 를 반영해도 **서버측 forward 캐시**는 건드리지 않는다 —
    /// 적용 경로가 로컬 producer 축과 분리돼 있음을 값으로 확인한다(같은 engine 이
    /// 서버이자 client 일 수는 없지만, 두 축이 공유 상태를 통해 얽히지 않는다는
    /// 불변식은 유지돼야 한다).
    #[test]
    fn mirror_apply_does_not_touch_forward_cache() {
        let mut s = state();
        s.set_mirror_surface_attention(13, Some(AttentionKind::NeedsInput));
        assert!(s.last_forwarded_attention.is_empty());
        s.forget_mirror_surface_attention(13);
        assert!(s.last_forwarded_attention.is_empty());
    }

    /// wire 변환은 왕복해도 값이 보존된다(server→client 경계).
    #[test]
    fn attention_kind_wire_roundtrips() {
        for k in [AttentionKind::Completion, AttentionKind::NeedsInput] {
            assert_eq!(AttentionKind::from_wire(k.to_wire()), k);
        }
        // 직렬화 문자열은 `surface.completion` IPC 의 `kind` 파라미터와 같은 어휘여야
        // 한다 — 한 vocabulary 로 producer IPC 와 attach 채널을 모두 덮는다.
        assert_eq!(
            serde_json::to_value(AttentionKind::NeedsInput.to_wire()).unwrap(),
            serde_json::Value::String("needs_input".into())
        );
        assert_eq!(
            serde_json::to_value(AttentionKind::Completion.to_wire()).unwrap(),
            serde_json::Value::String("completion".into())
        );
    }

    /// `effects_of` 는 host/cascade 없이 순수하게 kind → 효과를 매핑한다. OSC 133
    /// producer 는 `NotificationStore::add()` 를 호출하지 않으므로, 이 값
    /// (`panel_item == false`)이 실제로 패널 무관임을 보장하는 것이 이 리팩터의
    /// 핵심 회귀 포인트다 — 값이 뒤집히면 셸 명령마다 알림 패널이 오염된다.
    #[test]
    fn effects_of_completion_has_no_panel_item() {
        let effects = effects_of(AttentionKind::Completion);
        assert_eq!(effects.level, AttentionLevel::Completion);
        assert!(!effects.panel_item);
        assert!(!effects.os_notify);
        assert!(!effects.sound);
    }

    #[test]
    fn effects_of_needs_input_outranks_completion_and_has_no_panel_item() {
        let effects = effects_of(AttentionKind::NeedsInput);
        assert_eq!(effects.level, AttentionLevel::NeedsInput);
        assert!(effects.level > AttentionLevel::Completion);
        assert!(!effects.panel_item);
        assert!(!effects.os_notify);
        assert!(!effects.sound);
    }

    /// `dominant_kind` 는 목록에 섞인 kind 중 `NeedsInput` 을 고른다 — 탭 제목·
    /// collapsed rail dot 이 여러 surface 를 하나의 색으로 압축할 때 쓰는 규칙.
    #[test]
    fn dominant_kind_prefers_needs_input_over_completion() {
        let mut s = state();
        s.raise_attention(1, AttentionKind::Completion);
        s.raise_attention(2, AttentionKind::NeedsInput);
        assert_eq!(
            s.attention_dominant_kind(&[1, 2]),
            Some(AttentionKind::NeedsInput)
        );
        // 순서를 뒤집어도(NeedsInput 이 먼저 오지 않아도) 동일 — 값 기반 선택.
        assert_eq!(
            s.attention_dominant_kind(&[2, 1]),
            Some(AttentionKind::NeedsInput)
        );
    }

    #[test]
    fn dominant_kind_none_when_no_attention() {
        let s = state();
        assert_eq!(s.attention_dominant_kind(&[1, 2, 3]), None);
    }

    #[test]
    fn dominant_kind_single_completion() {
        let mut s = state();
        s.raise_attention(5, AttentionKind::Completion);
        assert_eq!(
            s.attention_dominant_kind(&[5]),
            Some(AttentionKind::Completion)
        );
    }

    /// 같은 surface 에 다시 raise 하면(예: needs_input 이후 completion 재발동)
    /// 최신 kind 로 완전히 대체된다 — 레코드는 surface 당 1개.
    #[test]
    fn raise_again_replaces_kind() {
        let mut s = state();
        s.raise_attention(1, AttentionKind::NeedsInput);
        assert_eq!(s.attention_kind(1), Some(AttentionKind::NeedsInput));
        s.raise_attention(1, AttentionKind::Completion);
        assert_eq!(s.attention_kind(1), Some(AttentionKind::Completion));
    }

    // ---- 해제 edge → mirror clear forward (client→server) ----

    /// 기본 워크스페이스에 mirror 플래그를 세우고 그 첫 surface id 를 돌려준다.
    /// `is_mirror_surface` 는 워크스페이스 플래그만 보므로 실제 attach 세션 없이도
    /// 판정에 충분하다.
    fn mirror_state() -> (CoreState, u32) {
        let mut s = state();
        s.workspaces[0].mirror = true;
        let sid = s.workspaces[0].all_surface_ids()[0];
        (s, sid)
    }

    /// `clear_attention` 은 **실제로 레코드를 제거했을 때만** true 다. 이 값이 곧
    /// 해제 edge 이므로, 레코드가 없는 상태의 반복 호출(매 프레임 도는 실-포커스
    /// 해제)은 전부 false 로 끝나 신호가 나가지 않는다.
    #[test]
    fn clear_attention_reports_the_removal_edge_only_once() {
        let mut s = state();
        assert!(!s.clear_attention(7), "레코드가 없으면 제거 edge 가 아니다");

        s.raise_attention(7, AttentionKind::Completion);
        assert!(s.clear_attention(7), "레코드를 실제로 지운 호출이 edge 다");
        assert!(
            !s.clear_attention(7),
            "연속 호출은 no-op — 포커스를 유지해도 edge 가 반복되지 않는다"
        );
    }

    /// mirror surface 의 제거 edge 는 forward 큐에 1건 쌓인다. 포커스를 유지한 채
    /// 다시 호출해도 추가로 쌓이지 않는다(프레임 스팸 없음).
    #[test]
    fn mirror_clear_queues_exactly_one_forward_edge() {
        let (mut s, sid) = mirror_state();
        // 미러의 레코드는 서버 push 로만 들어온다(로컬 raise 는 억제됨).
        s.set_mirror_surface_attention(sid, Some(AttentionKind::NeedsInput));
        assert!(s.pending_attention_clear_forward.is_empty());

        assert!(s.clear_attention(sid));
        assert_eq!(
            s.pending_attention_clear_forward
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![sid],
            "mirror 해제 edge 는 소유 인스턴스로 보낼 큐에 쌓여야 한다"
        );

        s.pending_attention_clear_forward.clear(); // App 이 drain 한 상태를 모사
        assert!(!s.clear_attention(sid));
        assert!(
            s.pending_attention_clear_forward.is_empty(),
            "레코드가 없으면 프레임이 다시 나가지 않는다"
        );
    }

    /// mirror 아닌 surface 의 해제는 로컬에서 끝난다 — forward 큐에 쌓이지 않는다.
    #[test]
    fn non_mirror_clear_does_not_queue_a_forward() {
        let mut s = state();
        let sid = s.workspaces[0].all_surface_ids()[0];
        s.raise_attention(sid, AttentionKind::Completion);

        assert!(s.clear_attention(sid));
        assert!(
            s.pending_attention_clear_forward.is_empty(),
            "소유 인스턴스의 해제는 전달할 곳이 없다"
        );
    }

    /// 회귀 방지 — 큐 push 를 포커스 호출부가 아니라 `clear_attention` 안에서 하는
    /// 이유. 미러 로컬 알림(미러 바이트의 Bell/OSC 9)을 알림 패널에서 읽음 처리해도
    /// 같은 큐에 쌓여야 한다. 포커스 경로만 커버하면 이 경우가 서버에 전달되지 않아
    /// 다음 push 에서 배지가 되살아난다.
    #[test]
    fn mirror_notification_read_queues_the_clear_forward() {
        let (mut s, sid) = mirror_state();
        let ws_id = s.workspaces[0].id;
        let nid = s
            .notifications
            .add(ws_id, sid, "t".into(), "b".into())
            .expect("알림 생성");
        s.set_mirror_surface_attention(sid, Some(AttentionKind::Completion));

        s.mark_notification_read(nid);

        assert_eq!(s.attention_kind(sid), None);
        assert_eq!(
            s.pending_attention_clear_forward
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![sid],
            "알림 읽음으로 확인한 경우도 서버로 전달돼야 한다"
        );
    }

    /// "모두 읽음" 경로도 같은 큐를 탄다.
    #[test]
    fn mirror_mark_all_read_queues_the_clear_forward() {
        let (mut s, sid) = mirror_state();
        let ws_id = s.workspaces[0].id;
        s.notifications.add(ws_id, sid, "t".into(), "b".into());
        s.set_mirror_surface_attention(sid, Some(AttentionKind::Completion));

        s.mark_all_notifications_read();

        assert_eq!(
            s.pending_attention_clear_forward
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![sid]
        );
    }

    /// 에코 루프 방지 — 서버가 내려준 해제(`kind: null` push)와 teardown 은
    /// `clear_attention` 을 타지 않으므로 forward 큐에 쌓이지 않는다.
    #[test]
    fn server_push_clear_and_teardown_do_not_queue_a_forward() {
        let (mut s, sid) = mirror_state();

        s.set_mirror_surface_attention(sid, Some(AttentionKind::NeedsInput));
        s.set_mirror_surface_attention(sid, None); // 서버가 내려준 해제
        assert_eq!(s.attention_kind(sid), None);
        assert!(
            s.pending_attention_clear_forward.is_empty(),
            "서버가 내려준 해제를 서버로 되돌리면 에코가 된다"
        );

        s.set_mirror_surface_attention(sid, Some(AttentionKind::NeedsInput));
        s.forget_mirror_surface_attention(sid); // surface 소멸 teardown
        assert!(
            s.pending_attention_clear_forward.is_empty(),
            "teardown 은 사용자의 확인이 아니다"
        );
    }

    // ───── 하드 점유 중 해제는 홀더만 (ADR-0109) ─────

    /// 게이트 술어 자체 — 렌더 경로(`gpu.rs`)는 GPU 없이 실행할 수 없으므로 그
    /// 호출부가 묻는 판정만 떼어 직접 검증한다. hard lock 이 걸린 동안 로컬 해제가
    /// 금지되고, 풀리면 즉시 허용으로 복귀한다(데드락 없음).
    #[test]
    fn local_clear_is_disallowed_exactly_while_hard_occupied() {
        let mut s = state();
        assert!(
            s.local_attention_clear_allowed(42),
            "점유 없는 surface 는 로컬 해제 대상이다"
        );

        s.attach.acquire(42, 1).expect("hard lock");
        assert!(
            !s.local_attention_clear_allowed(42),
            "하드 점유 중에는 로컬 사용자가 주체가 아니다"
        );
        assert!(
            s.local_attention_clear_allowed(43),
            "게이트는 점유된 surface 에만 걸린다"
        );

        s.attach.release(42, 1).expect("release");
        assert!(
            s.local_attention_clear_allowed(42),
            "점유가 풀리면 로컬 포커스가 해제 주체로 복귀한다"
        );
    }

    /// soft 점유(child-terminal)는 로컬 사용자를 배제하지 않으므로(ADR-0040
    /// "write 제한 없음") 이 게이트의 대상이 아니다.
    #[test]
    fn soft_occupancy_does_not_gate_the_local_clear() {
        let mut s = state();
        s.attach
            .acquire_soft(42, 7, Some("child".into()))
            .expect("soft lock");
        assert!(
            s.local_attention_clear_allowed(42),
            "soft 점유는 hard 술어를 세우지 않는다 — 로컬 해제 그대로"
        );

        s.raise_attention(42, AttentionKind::Completion);
        assert!(s.clear_attention_local(42), "soft 점유 중 로컬 해제는 성립");
        assert_eq!(s.attention_kind(42), None);
    }

    /// 실-포커스 경로의 대역 — `gpu.rs` 가 부르는 것과 같은 함수로, 점유 중에는
    /// 레코드가 살아남고 점유가 풀린 뒤의 같은 호출이 지운다.
    #[test]
    fn hard_occupied_surface_survives_the_local_focus_clear() {
        let mut s = state();
        s.raise_attention(42, AttentionKind::NeedsInput);
        s.attach.acquire(42, 1).expect("hard lock");

        assert!(
            !s.clear_attention_local(42),
            "게이트에 막히면 제거 edge 자체가 없다"
        );
        assert_eq!(
            s.attention_kind(42),
            Some(AttentionKind::NeedsInput),
            "점유 중 서버 로컬 포커스는 홀더의 신호를 지우지 못한다"
        );

        s.attach.release(42, 1).expect("release");
        assert!(s.clear_attention_local(42), "점유 해제 후에는 지워진다");
        assert_eq!(s.attention_kind(42), None);
    }

    /// 홀더의 해제(서버측 적용 경로)는 게이트를 타지 않는다 — ADR-0104 가 이 작업에
    /// 걸어둔 제약의 회귀 테스트다. 게이트가 `clear_attention` 안에 들어갔다면
    /// 점유된 surface 의 해제 주체가 0 이 되어 이 assert 가 깨진다.
    #[test]
    fn the_holders_clear_is_not_blocked_by_the_gate() {
        let mut s = state();
        let sid = s.workspaces[0].all_surface_ids()[0];
        let ws = s.workspaces[0].id;
        s.attach
            .acquire_workspace(ws, &[sid], &[sid], 1)
            .expect("workspace hard lock");
        s.raise_attention(sid, AttentionKind::NeedsInput);

        assert!(
            !s.clear_attention_local(sid),
            "로컬 축은 막혀 있어야 한다(전제 확인)"
        );
        assert!(
            s.apply_attached_attention_clear(1, sid),
            "holder 검증을 통과한 요청은 적용된다"
        );
        assert_eq!(
            s.attention_kind(sid),
            None,
            "홀더의 확인은 게이트와 무관하게 레코드를 지운다"
        );
    }

    /// 개별 읽음 처리 — 점유 중에는 attention 이 유지되고, 알림의 `read` 플래그는
    /// 점유 여부와 무관하게 세워진다(두 상태의 분리).
    #[test]
    fn marking_a_notification_read_keeps_attention_while_hard_occupied() {
        let mut s = state_no_coalesce();
        let occupied_read = s
            .notifications
            .add(1, 100, "t1".into(), "b1".into())
            .unwrap();
        s.raise_attention(100, AttentionKind::Completion);
        s.attach.acquire(100, 1).expect("hard lock");

        s.mark_notification_read(occupied_read);

        assert_eq!(
            s.attention_kind(100),
            Some(AttentionKind::Completion),
            "점유 중 알림 읽음은 홀더의 신호를 지우지 못한다"
        );
        assert!(
            s.notifications
                .all()
                .find(|n| n.id == occupied_read)
                .unwrap()
                .read,
            "알림 읽음 자체는 점유와 무관하게 처리된다(회귀 방지)"
        );

        // 점유 해제 후 **같은 호출**이 지운다 — 앞선 알림은 이미 읽음이라 새 안읽음
        // 알림 하나로 그 경로를 다시 태운다(해제 조건 = 남은 안읽음 0).
        s.attach.release(100, 1).expect("release");
        let after_release = s
            .notifications
            .add(1, 100, "t2".into(), "b2".into())
            .unwrap();
        s.mark_notification_read(after_release);
        assert_eq!(
            s.attention_kind(100),
            None,
            "점유가 풀리면 알림 읽음이 다시 해제 주체가 된다"
        );
    }

    /// "모두 읽음" — 점유 중 surface 는 clear 대상에서 빠지고, 점유되지 않은
    /// surface 는 그대로 지워진다. 이 조합이 "모두 읽음 한 번으로 점유 중 배지가
    /// 전부 사라지는" 구멍을 막는다.
    #[test]
    fn mark_all_read_skips_hard_occupied_surfaces_only() {
        let mut s = state_no_coalesce();
        let occupied = s
            .notifications
            .add(1, 100, "t1".into(), "b1".into())
            .unwrap();
        let free = s
            .notifications
            .add(1, 200, "t2".into(), "b2".into())
            .unwrap();
        s.raise_attention(100, AttentionKind::NeedsInput);
        s.raise_attention(200, AttentionKind::Completion);
        s.attach.acquire(100, 1).expect("hard lock");

        s.mark_all_notifications_read();

        assert_eq!(
            s.attention_kind(100),
            Some(AttentionKind::NeedsInput),
            "점유 중 surface 는 clear 대상에서 제외된다"
        );
        assert_eq!(
            s.attention_kind(200),
            None,
            "점유되지 않은 surface 는 기존 동작 그대로 지워진다"
        );
        for id in [occupied, free] {
            assert!(
                s.notifications.all().find(|n| n.id == id).unwrap().read,
                "모든 알림의 read 플래그는 점유 여부와 무관하게 세워진다(회귀 방지)"
            );
        }

        // 점유 해제 후 **같은 호출**이 지운다. 앞선 알림은 전부 읽음이라 안읽음 집합이
        // 비어 있으므로, 새 안읽음 알림 하나로 그 경로를 다시 태운다.
        s.attach.release(100, 1).expect("release");
        s.notifications.add(1, 100, "t3".into(), "b3".into());
        s.mark_all_notifications_read();
        assert_eq!(
            s.attention_kind(100),
            None,
            "점유가 풀리면 \"모두 읽음\" 이 다시 해제 주체가 된다(stale 레코드 회수)"
        );
    }

    /// 미러 인스턴스에는 이 게이트가 걸리지 않는다 — 점유는 surface 를 **소유한**
    /// 인스턴스가 기록하므로 미러의 `OccupancyRegistry` 는 비어 있다. 그래서 미러
    /// 사용자의 확인은 그대로 제거 edge 를 만들어 서버로 forward 된다(ADR-0104).
    #[test]
    fn the_gate_does_not_block_the_mirror_users_clear() {
        let (mut s, sid) = mirror_state();
        s.set_mirror_surface_attention(sid, Some(AttentionKind::NeedsInput));

        assert!(
            s.local_attention_clear_allowed(sid),
            "미러 surface 는 이 인스턴스에서 점유돼 있지 않다"
        );
        assert!(
            s.clear_attention_local(sid),
            "미러 사용자의 확인은 성립한다"
        );
        assert!(
            s.pending_attention_clear_forward.contains(&sid),
            "그 edge 는 소유 인스턴스로 forward 되어야 한다(ADR-0104)"
        );
    }
}
