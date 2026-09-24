//! Surface의 attention은 알림 패널과 별도 상태다. 생성은 raise_attention,
//! 로컬 사용자 확인은 clear_attention_local을 거친다. mirror는 서버 push만 반영한다.

use std::collections::HashMap;
use std::time::Instant;

use super::CoreState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AttentionKind {
    Completion,
    /// 사용자 응답이 필요한 상태는 완료 상태보다 높은 표시 우선순위를 갖는다.
    NeedsInput,
}

impl AttentionKind {
    /// tasty-ipc는 host 타입에 의존하지 않으므로 attach 경계에서 변환한다.
    pub(crate) fn to_wire(self) -> tasty_ipc::stream::AttentionKindWire {
        match self {
            AttentionKind::Completion => tasty_ipc::stream::AttentionKindWire::Completion,
            AttentionKind::NeedsInput => tasty_ipc::stream::AttentionKindWire::NeedsInput,
        }
    }

    pub(crate) fn from_wire(wire: tasty_ipc::stream::AttentionKindWire) -> Self {
        match wire {
            tasty_ipc::stream::AttentionKindWire::Completion => AttentionKind::Completion,
            tasty_ipc::stream::AttentionKindWire::NeedsInput => AttentionKind::NeedsInput,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AttentionRecord {
    kind: AttentionKind,
    #[allow(dead_code)] // 이유: 기록 시각을 보관하지만 현재 읽는 곳은 없다.
    raised_at: Instant,
}

/// 여러 surface의 대표 색을 고를 우선순위. 선언 순서가 Ord가 되므로
/// 디자인 rank 토큰의 오름차순을 따른다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum AttentionLevel {
    /// --tasty-attention-rank-completion = 10.
    Completion,
    /// --tasty-attention-rank-needs-input = 30.
    NeedsInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AttentionEffects {
    pub(crate) level: AttentionLevel,
    /// 현재 값은 로그에만 쓰인다. 알림 패널 항목은 producer가 별도로 생성한다.
    pub(crate) panel_item: bool,
    /// 현재 OS 알림을 실행하는 데 사용하지 않는다.
    pub(crate) os_notify: bool,
    /// 실제 소리는 알림 설정과 bell-source 조건으로 별도 판단한다.
    pub(crate) sound: bool,
}

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
            panel_item: false,
            os_notify: false,
            sound: false,
        },
    }
}

/// surface마다 최신 레코드 하나를 보관한다. NotificationStore와는 독립적이다.
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

    fn clear(&mut self, surface_id: u32) -> bool {
        self.records.remove(&surface_id).is_some()
    }

    fn kind_of(&self, surface_id: u32) -> Option<AttentionKind> {
        self.records.get(&surface_id).map(|r| r.kind)
    }

    #[cfg(any(feature = "gui", test))]
    fn count_of_kind(&self, kind: AttentionKind, surface_ids: &[u32]) -> usize {
        surface_ids
            .iter()
            .filter(|sid| self.records.get(sid).map(|r| r.kind) == Some(kind))
            .count()
    }

    #[cfg(any(feature = "gui", test))]
    fn dominant_kind(&self, surface_ids: &[u32]) -> Option<AttentionKind> {
        surface_ids
            .iter()
            .filter_map(|sid| self.records.get(sid).map(|r| r.kind))
            .max_by_key(|k| effects_of(*k).level)
    }
}

impl CoreState {
    /// ID 0과 mirror는 제외한다. mirror 바이트를 다시 파싱해도 로컬 attention은 만들지 않는다.
    /// 이 제한은 별도로 만드는 알림 패널 항목·토스트·훅에는 적용되지 않는다.
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

    /// 실제로 제거했으면 true다. mirror의 제거는 서버에 보낼 큐에도 넣는다.
    /// 포커스와 알림 읽음 처리가 같은 경로를 사용하도록 여기서 큐에 넣는다.
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

    /// 하드 점유 중에는 로컬 사용자가 확인 처리할 수 없다. soft 점유는 제외한다.
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn local_attention_clear_allowed(&self, surface_id: u32) -> bool {
        !self.attach.is_hard_occupied(surface_id)
    }

    /// 로컬 사용자 확인에만 하드 점유 제한을 적용한다. 검증된 holder의 요청까지
    /// 막지 않도록 clear_attention 자체에는 이 제한을 두지 않는다.
    #[cfg(any(feature = "gui", test))]
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

    /// 서버 값을 반영한다. 로컬 생성 제한을 적용하거나 해제를 서버로 돌려보내지 않는다.
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn set_mirror_surface_attention(
        &mut self,
        surface_id: u32,
        kind: Option<AttentionKind>,
    ) {
        match kind {
            Some(k) => self.attention.raise(surface_id, k),
            None => {
                self.attention.clear(surface_id);
            }
        }
    }

    /// 사라진 mirror의 레코드를 버린다. 사용자 확인이 아니므로 서버로 해제를 보내지 않는다.
    #[cfg(any(feature = "gui", test))]
    pub(crate) fn forget_mirror_surface_attention(&mut self, surface_id: u32) {
        self.attention.clear(surface_id);
    }

    /// 하드 점유한 surface의 (holder, ID, kind) 전송 후보. 최초 값과 holder·kind 변경을 담는다.
    /// 점유가 끝난 캐시는 지우며, 같은 호출 간격 안에 holder만 바뀐 경우도 구분한다.
    /// 후보 생성 시 캐시를 갱신하므로 실패한 전송을 같은 값으로 다시 시도하지 않는다.
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
            let record = (lock.holder, self.attention.kind_of(sid));
            if self.last_forwarded_attention.get(&sid) != Some(&record) {
                self.last_forwarded_attention.insert(sid, record);
                out.push((record.0, sid, record.1));
            }
        }
        out
    }

    pub(crate) fn attention_kind(&self, surface_id: u32) -> Option<AttentionKind> {
        self.attention.kind_of(surface_id)
    }

    #[cfg(any(feature = "gui", test))]
    pub(crate) fn attention_count_of_kind(
        &self,
        kind: AttentionKind,
        surface_ids: &[u32],
    ) -> usize {
        self.attention.count_of_kind(kind, surface_ids)
    }

    /// 목록의 대표 kind. NeedsInput을 Completion보다 우선한다.
    #[cfg(any(feature = "gui", test))]
    pub fn attention_dominant_kind(&self, surface_ids: &[u32]) -> Option<AttentionKind> {
        self.attention.dominant_kind(surface_ids)
    }

    /// 알림을 읽음 처리하고 같은 surface의 안읽음 알림이 없으면 로컬 attention 해제를 요청한다.
    /// 하드 점유 중에는 attention이 남지만 알림의 읽음 상태는 바뀐다.
    #[cfg(any(feature = "gui", test))]
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

    /// 모든 알림을 읽음 처리한다. 이전에 안읽음 알림이 있던 surface만 해제를 요청하며
    /// 하드 점유 중인 surface의 attention은 유지한다.
    #[cfg(any(feature = "gui", test))]
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

    /// 같은 surface의 알림을 별개 항목으로 검사하기 위해 합치기 시간을 0으로 둔다.
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

    #[test]
    fn mark_notification_read_clears_attention_when_no_unread_left() {
        let mut s = state();
        let id = s.notifications.add(1, 100, "t".into(), "b".into()).unwrap();
        s.raise_attention(100, AttentionKind::Completion);
        assert!(s.attention_dominant_kind(&[100]).is_some());

        s.mark_notification_read(id);

        assert!(!s.attention_dominant_kind(&[100]).is_some());
    }

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

    #[test]
    fn mark_notification_read_unknown_id_is_noop() {
        let mut s = state();
        s.raise_attention(100, AttentionKind::Completion);
        s.mark_notification_read(9999);
        assert!(s.attention_dominant_kind(&[100]).is_some());
    }

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
            "100에는 안읽음 알림이 없었으므로 별도로 생성한 attention을 유지해야 한다"
        );
        assert!(!s.attention_dominant_kind(&[200]).is_some());
    }

    #[test]
    fn attention_forwards_only_on_change() {
        let mut e = state();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock 획득");

        assert_eq!(e.attention_forwards(), vec![(7, sid, None)]);

        assert!(e.attention_forwards().is_empty());

        e.raise_attention(sid, AttentionKind::NeedsInput);
        assert_eq!(
            e.attention_forwards(),
            vec![(7, sid, Some(AttentionKind::NeedsInput))]
        );
        assert!(e.attention_forwards().is_empty());

        e.raise_attention(sid, AttentionKind::Completion);
        assert_eq!(
            e.attention_forwards(),
            vec![(7, sid, Some(AttentionKind::Completion))]
        );
        assert!(e.attention_forwards().is_empty());

        e.clear_attention(sid);
        assert_eq!(e.attention_forwards(), vec![(7, sid, None)]);
        assert!(e.attention_forwards().is_empty());
    }

    #[test]
    fn attention_forwards_ignores_unoccupied_surfaces() {
        let mut e = state();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.raise_attention(sid, AttentionKind::NeedsInput);
        assert!(e.attention_forwards().is_empty());
    }

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
        assert!(e.attention_forwards().is_empty());

        e.attach.acquire(sid, 9).expect("다른 client 재획득");
        assert_eq!(
            e.attention_forwards(),
            vec![(9, sid, Some(AttentionKind::NeedsInput))],
            "재획득 후에는 값이 이전과 같아도 새 holder 에게 다시 push"
        );
    }

    /// 해제와 재획득 사이에 전송 후보 조회가 없어도 새 holder를 구분해야 한다.
    #[test]
    fn attention_forwards_holder_swap_within_one_tick_pushes_to_the_new_holder() {
        let mut e = state();
        let sid = e.workspaces[0].all_surface_ids()[0];
        e.attach.acquire(sid, 7).expect("lock 획득");
        e.raise_attention(sid, AttentionKind::NeedsInput);
        assert_eq!(
            e.attention_forwards(),
            vec![(7, sid, Some(AttentionKind::NeedsInput))]
        );

        e.attach.release(sid, 7).expect("release");
        e.attach
            .acquire(sid, 9)
            .expect("같은 tick 창 안의 다른 client 획득");
        assert_eq!(
            e.attention_forwards(),
            vec![(9, sid, Some(AttentionKind::NeedsInput))],
            "값이 그대로여도 새 holder 는 초기 push 를 받아야 한다"
        );
        assert!(e.attention_forwards().is_empty());
    }

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

        s.set_mirror_surface_attention(11, None);
        assert_eq!(s.attention_kind(11), None);
    }

    #[test]
    fn forget_mirror_surface_attention_drops_the_record() {
        let mut s = state();
        s.set_mirror_surface_attention(12, Some(AttentionKind::Completion));
        s.forget_mirror_surface_attention(12);
        assert_eq!(s.attention_kind(12), None);
    }

    #[test]
    fn mirror_apply_does_not_touch_forward_cache() {
        let mut s = state();
        s.set_mirror_surface_attention(13, Some(AttentionKind::NeedsInput));
        assert!(s.last_forwarded_attention.is_empty());
        s.forget_mirror_surface_attention(13);
        assert!(s.last_forwarded_attention.is_empty());
    }

    #[test]
    fn attention_kind_wire_roundtrips() {
        for k in [AttentionKind::Completion, AttentionKind::NeedsInput] {
            assert_eq!(AttentionKind::from_wire(k.to_wire()), k);
        }
        assert_eq!(
            serde_json::to_value(AttentionKind::NeedsInput.to_wire()).unwrap(),
            serde_json::Value::String("needs_input".into())
        );
        assert_eq!(
            serde_json::to_value(AttentionKind::Completion.to_wire()).unwrap(),
            serde_json::Value::String("completion".into())
        );
    }

    /// 정책 반환값만 확인한다. producer의 실제 패널 생성이나 소리 실행을 검사하지 않는다.
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

    #[test]
    fn dominant_kind_prefers_needs_input_over_completion() {
        let mut s = state();
        s.raise_attention(1, AttentionKind::Completion);
        s.raise_attention(2, AttentionKind::NeedsInput);
        assert_eq!(
            s.attention_dominant_kind(&[1, 2]),
            Some(AttentionKind::NeedsInput)
        );
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

    #[test]
    fn raise_again_replaces_kind() {
        let mut s = state();
        s.raise_attention(1, AttentionKind::NeedsInput);
        assert_eq!(s.attention_kind(1), Some(AttentionKind::NeedsInput));
        s.raise_attention(1, AttentionKind::Completion);
        assert_eq!(s.attention_kind(1), Some(AttentionKind::Completion));
    }

    /// 실제 attach 없이 workspace의 mirror 플래그로 분기만 검사한다.
    fn mirror_state() -> (CoreState, u32) {
        let mut s = state();
        s.workspaces[0].mirror = true;
        let sid = s.workspaces[0].all_surface_ids()[0];
        (s, sid)
    }

    #[test]
    fn clear_attention_reports_the_removal_edge_only_once() {
        let mut s = state();
        assert!(!s.clear_attention(7), "레코드가 없으면 제거 결과는 false다");

        s.raise_attention(7, AttentionKind::Completion);
        assert!(s.clear_attention(7), "레코드를 제거했으면 true다");
        assert!(
            !s.clear_attention(7),
            "이미 제거한 레코드를 다시 지울 수는 없다"
        );
    }

    #[test]
    fn mirror_clear_queues_exactly_one_forward_edge() {
        let (mut s, sid) = mirror_state();
        s.set_mirror_surface_attention(sid, Some(AttentionKind::NeedsInput));
        assert!(s.pending_attention_clear_forward.is_empty());

        assert!(s.clear_attention(sid));
        assert_eq!(
            s.pending_attention_clear_forward
                .iter()
                .copied()
                .collect::<Vec<_>>(),
            vec![sid],
            "mirror를 해제하면 서버에 보낼 큐에 들어가야 한다"
        );

        s.pending_attention_clear_forward.clear(); // App 이 drain 한 상태를 모사
        assert!(!s.clear_attention(sid));
        assert!(
            s.pending_attention_clear_forward.is_empty(),
            "레코드가 없으면 프레임이 다시 나가지 않는다"
        );
    }

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

    /// 렌더링 없이 로컬 해제 허용 조건만 검사한다.
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

    #[test]
    fn soft_occupancy_does_not_gate_the_local_clear() {
        let mut s = state();
        s.attach
            .acquire_soft(42, 7, Some("child".into()))
            .expect("soft lock");
        assert!(
            s.local_attention_clear_allowed(42),
            "soft 점유 중에도 로컬 해제는 허용된다"
        );

        s.raise_attention(42, AttentionKind::Completion);
        assert!(s.clear_attention_local(42), "soft 점유 중 로컬 해제는 성립");
        assert_eq!(s.attention_kind(42), None);
    }

    #[test]
    fn hard_occupied_surface_survives_the_local_focus_clear() {
        let mut s = state();
        s.raise_attention(42, AttentionKind::NeedsInput);
        s.attach.acquire(42, 1).expect("hard lock");

        assert!(
            !s.clear_attention_local(42),
            "하드 점유 중에는 로컬 해제가 false를 반환한다"
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
            "전제: 로컬 사용자의 해제는 차단돼야 한다"
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

        // 앞선 알림은 이미 읽었으므로 새 안읽음 알림으로 같은 경로를 다시 검사한다.
        s.attach.release(100, 1).expect("release");
        let after_release = s
            .notifications
            .add(1, 100, "t2".into(), "b2".into())
            .unwrap();
        s.mark_notification_read(after_release);
        assert_eq!(
            s.attention_kind(100),
            None,
            "점유 해제 후에는 알림 읽음으로 attention을 지울 수 있다"
        );
    }

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

        // 앞선 알림은 모두 읽었으므로 새 안읽음 알림으로 같은 경로를 다시 검사한다.
        s.attach.release(100, 1).expect("release");
        s.notifications.add(1, 100, "t3".into(), "b3".into());
        s.mark_all_notifications_read();
        assert_eq!(
            s.attention_kind(100),
            None,
            "점유 해제 후에는 모두 읽음으로 attention을 지울 수 있다"
        );
    }

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
            "mirror의 해제 요청은 서버에 보낼 큐에 들어가야 한다"
        );
    }

    fn rank_token_of(level: AttentionLevel) -> f32 {
        match level {
            AttentionLevel::Completion => {
                tasty_design_tokens::generated::semantic::ATTENTION_RANK_COMPLETION
            }
            AttentionLevel::NeedsInput => {
                tasty_design_tokens::generated::semantic::ATTENTION_RANK_NEEDS_INPUT
            }
        }
    }

    /// 비교 대상 목록. 새 등급이 추가될 때 이 배열의 누락까지 자동 검출하지는 못한다.
    const ALL_LEVELS: [AttentionLevel; 2] =
        [AttentionLevel::Completion, AttentionLevel::NeedsInput];

    /// Rust의 Ord와 디자인 토큰의 값 순서가 같은지 비교한다.
    /// Rust enum만 보는 검사로는 토큰 값이 바뀐 경우를 검출할 수 없다.
    #[test]
    fn the_declaration_order_mirrors_the_rank_tokens() {
        assert!(
            ALL_LEVELS.len() >= 2,
            "순서를 비교하려면 등급이 둘 이상이어야 한다"
        );
        for pair in ALL_LEVELS.windows(2) {
            let (lo, hi) = (pair[0], pair[1]);
            assert!(
                lo < hi,
                "선언 순서가 derived `Ord` 와 어긋났다: {lo:?} < {hi:?} 가 거짓이다"
            );
            assert!(
                rank_token_of(lo) < rank_token_of(hi),
                "선언 순서가 rank 토큰 값의 오름차순이 아니다 — {:?}={} · {:?}={}. \
                 토큰이 정본이므로 토큰을 고치지 말고 `AttentionLevel` 의 선언 순서를 \
                 토큰에 맞춰라(재도출 금지 계약).",
                lo,
                rank_token_of(lo),
                hi,
                rank_token_of(hi)
            );
        }
    }
}
