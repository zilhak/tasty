//! Surface "Attention" 상태 조작/조회. `AttentionStore` 는 producer 중립 공유
//! primitive — toast 알림, completion IPC/CLI, OSC 133 명령 완료 등 여러 producer 가
//! `raise_attention` 으로 발동하고, surface 가 실제 렌더 시점 포커스를 얻으면
//! (`gpu.rs`) `clear_attention` 으로 해제된다. 세 소비처(테두리·탭 제목·워크스페이스
//! 개수 배지)가 이 상태를 읽는다. `state/busy.rs` 의 조회 헬퍼 형태를 1:1 미러한다.
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

    fn clear(&mut self, surface_id: u32) {
        self.records.remove(&surface_id);
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
    /// `effects_of(kind)` 정책을 여기 한 곳에서 조회한다 — 이 TODO 범위(kind=Completion)
    /// 는 `panel_item`/`os_notify`/`sound` 가 전부 false 라 로그 외 추가 집행이 없지만,
    /// `attention-needs-input-visuals` 가 kind 를 늘리면 이 자리에서 분기가 생긴다.
    /// (`level` 은 아직 소비처가 없다 — 향후 테두리/배지 색 우선순위가 읽는다.)
    pub(crate) fn raise_attention(&mut self, surface_id: u32, kind: AttentionKind) {
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
    pub fn clear_attention(&mut self, surface_id: u32) {
        self.attention.clear(surface_id);
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
    pub(crate) fn mark_notification_read(&mut self, id: u64) {
        let source_surface = self
            .notifications
            .all()
            .find(|n| n.id == id)
            .map(|n| n.source_surface);
        self.notifications.mark_read(id);
        if let Some(surface_id) = source_surface {
            if !self.notifications.has_unread_for_surface(surface_id) {
                self.clear_attention(surface_id);
            }
        }
    }

    /// 모든 알림 읽음 처리(ADR-0039 Reconsideration Triggers 참고). 전부 읽음
    /// 처리되므로 엣지 케이스 없이, 읽음
    /// 처리 전 안읽음이었던 모든 알림의 source surface attention 을 지운다.
    pub(crate) fn mark_all_notifications_read(&mut self) {
        let unread_surfaces: std::collections::HashSet<u32> = self
            .notifications
            .all()
            .filter(|n| !n.read)
            .map(|n| n.source_surface)
            .collect();
        self.notifications.mark_all_read();
        for surface_id in unread_surfaces {
            self.clear_attention(surface_id);
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
}
