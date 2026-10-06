//! 자기 포트로 향한 attach 거절의 기록.
//!
//! 거절은 엔드포인트를 해석한 뒤 메인 루프에서 정해지므로 `remote.attach` 응답에 실을 수 없다.
//! 호출자는 응답에 있는 시도 번호로 이 기록을 조회한다. 자동 attach 매핑의 거절은 같은 매핑이
//! 남아 있는 동안 다시 시도하지 않도록 함께 보관한다.

use std::collections::{HashMap, VecDeque};

use tasty_model::{WorkspaceAttachMapping, WorkspaceAttachTarget};

/// 보관하는 최근 거절 수. 넘으면 오래된 것부터 버린다.
pub const REFUSAL_HISTORY: usize = 32;

/// 자기 포트라서 연결하지 않은 attach 시도 하나.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AttachRefusal {
    /// `remote.attach` 응답의 `attempt`와 같은 번호.
    pub attempt: u64,
    /// 자동 attach 매핑을 가진 로컬 workspace. 수동 `remote.attach`는 `None`이다.
    pub anchor_workspace: Option<u32>,
    pub remote_workspace: u32,
    /// 해석한 연결 포트. 이 인스턴스의 IPC 포트와 같았다.
    pub port: u16,
    pub reconnect: bool,
}

#[derive(Default)]
pub struct Refusals {
    recent: VecDeque<AttachRefusal>,
    /// 인라인 대상은 대상 문자열이 포트를 정하므로 매핑이 그대로면 다시 시도해도 같은 거절이다.
    /// 프로필 대상은 프로세스 밖에서 프로필 내용이 바뀔 수 있어 여기 보관하지 않는다.
    held_inline: HashMap<u32, WorkspaceAttachMapping>,
}

impl Refusals {
    pub fn record(&mut self, refusal: AttachRefusal, mapping: Option<&WorkspaceAttachMapping>) {
        if let (Some(anchor), Some(mapping)) = (refusal.anchor_workspace, mapping)
            && matches!(mapping.target, WorkspaceAttachTarget::Inline { .. })
        {
            self.held_inline.insert(anchor, mapping.clone());
        }
        if self.recent.len() == REFUSAL_HISTORY {
            self.recent.pop_front();
        }
        self.recent.push_back(refusal);
    }

    /// 이 anchor의 지금 매핑이 이미 자기 포트로 거절된 인라인 대상인지.
    pub fn holds(&self, anchor: u32, mapping: &WorkspaceAttachMapping) -> bool {
        self.held_inline.get(&anchor) == Some(mapping)
    }

    /// 오래된 것부터.
    pub fn recent(&self) -> impl Iterator<Item = &AttachRefusal> {
        self.recent.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(attempt: u64, anchor: Option<u32>) -> AttachRefusal {
        AttachRefusal {
            attempt,
            anchor_workspace: anchor,
            remote_workspace: 1,
            port: 4000,
            reconnect: false,
        }
    }

    fn inline(host: &str) -> WorkspaceAttachMapping {
        WorkspaceAttachMapping {
            target: WorkspaceAttachTarget::Inline {
                host: host.into(),
                remote_tasty: None,
                port_mode: None,
                port_file: None,
            },
            remote_workspace: Some(1),
        }
    }

    #[test]
    fn an_inline_mapping_stays_held_until_it_changes() {
        let mut refusals = Refusals::default();
        let refused = inline("127.0.0.1:4000");
        refusals.record(refusal(1, Some(7)), Some(&refused));
        assert!(refusals.holds(7, &refused));
        assert!(!refusals.holds(7, &inline("127.0.0.1:4001")));
        assert!(!refusals.holds(8, &refused));
    }

    #[test]
    fn a_profile_mapping_is_listed_but_not_held() {
        let mut refusals = Refusals::default();
        let profile = WorkspaceAttachMapping::profile("me", Some(1));
        refusals.record(refusal(1, Some(7)), Some(&profile));
        assert!(!refusals.holds(7, &profile));
        assert_eq!(refusals.recent().count(), 1);
    }

    #[test]
    fn history_keeps_the_newest_refusals() {
        let mut refusals = Refusals::default();
        for attempt in 0..(REFUSAL_HISTORY as u64 + 3) {
            refusals.record(refusal(attempt, None), None);
        }
        let attempts: Vec<_> = refusals.recent().map(|r| r.attempt).collect();
        assert_eq!(attempts.len(), REFUSAL_HISTORY);
        assert_eq!(attempts.first(), Some(&3));
        assert_eq!(attempts.last(), Some(&(REFUSAL_HISTORY as u64 + 2)));
    }
}
