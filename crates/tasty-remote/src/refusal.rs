//! 자기 자신(자기 포트, 또는 SSH 너머의 같은 인스턴스)으로 향한 attach 거절의 기록.
//!
//! 거절은 엔드포인트를 해석한 뒤 메인 루프에서 정해지므로 `remote.attach` 응답에 실을 수 없다.
//! 호출자는 응답에 있는 시도 번호로 이 기록을 조회한다. 자동 attach 매핑의 거절은 같은 매핑이
//! 남아 있는 동안 다시 시도하지 않도록 함께 보관한다.
//!
//! 자동 attach 매핑을 연결하지 못한 이유(자기 자신, 없는 프로필, 해석 실패)는 anchor별 안내로도
//! 남긴다. 사이드바 행 표지와 Workspace 배너가 이 안내를 읽는다. 안내는 그 anchor의 매핑이
//! 바뀌거나 지워질 때, 또는 같은 매핑으로 연결에 성공할 때 사라진다.

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
    /// 해석한 연결 포트. loopback 직결이면 이 인스턴스의 IPC 포트이고, SSH 터널이면 터널의 로컬 포트다.
    pub port: u16,
    /// SSH 터널 너머의 `instance_id`가 이 프로세스와 같아서 거절했으면 true.
    pub via_ssh: bool,
    pub reconnect: bool,
}

/// 자동 attach 매핑을 연결하지 않은 이유.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MappingNoticeKind {
    /// 대상이 이 인스턴스 자신이다.
    SelfInstance,
    /// 매핑이 가리키는 SSH 프로필이 없다.
    ProfileMissing,
    /// 프로필이나 인라인 대상의 연결 지점을 준비하지 못했다.
    Unresolved,
}

/// anchor 하나의 연결하지 않은 매핑 안내.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingNotice {
    pub kind: MappingNoticeKind,
    /// 표시용 대상. 프로필 이름 또는 인라인 대상 문자열이다.
    pub target: String,
    /// 안내를 만든 매핑. 지금 매핑과 다르면 안내를 지운다.
    pub mapping: WorkspaceAttachMapping,
    /// 사용자가 그 워크스페이스를 활성화해 시작한 시도의 안내이거나, 안내가 있는 채로 사용자가
    /// 활성화했는지. 배너는 사용자 조작의 안내라 이 값이 true일 때만 띄운다. 행 표지는 늘 보인다.
    pub announced: bool,
}

/// 매핑의 표시용 대상.
pub fn mapping_target(mapping: &WorkspaceAttachMapping) -> String {
    match &mapping.target {
        WorkspaceAttachTarget::Profile { name } => name.clone(),
        WorkspaceAttachTarget::Inline { host, .. } => host.clone(),
    }
}

#[derive(Default)]
pub struct Refusals {
    recent: VecDeque<AttachRefusal>,
    /// 인라인 대상은 대상 문자열이 포트를 정하므로 매핑이 그대로면 다시 시도해도 같은 거절이다.
    /// 프로필 대상은 프로세스 밖에서 프로필 내용이 바뀔 수 있어 여기 보관하지 않는다.
    held_inline: HashMap<u32, WorkspaceAttachMapping>,
    notices: HashMap<u32, MappingNotice>,
    /// 진행 중인 첫 attach 시도가 사용자의 워크스페이스 활성화로 시작됐는지.
    activation_attempts: std::collections::HashSet<u32>,
    /// 안내가 바뀔 때마다 오른다. 표시 쪽은 같은 세대를 다시 받지 않는다.
    notice_generation: u64,
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

    /// 첫 attach 시도를 시작할 때 사용자의 활성화로 시작했는지 남긴다.
    pub fn begin_attempt(&mut self, anchor: u32, from_activation: bool) {
        if from_activation {
            self.activation_attempts.insert(anchor);
        } else {
            self.activation_attempts.remove(&anchor);
        }
    }

    /// anchor의 안내를 새 이유로 바꾼다. 같은 매핑의 안내가 이미 알렸으면 알린 상태를 유지한다.
    pub fn note(&mut self, anchor: u32, kind: MappingNoticeKind, mapping: &WorkspaceAttachMapping) {
        let announced = self.activation_attempts.remove(&anchor)
            || self
                .notices
                .get(&anchor)
                .is_some_and(|n| n.announced && n.mapping == *mapping);
        let notice = MappingNotice {
            kind,
            target: mapping_target(mapping),
            mapping: mapping.clone(),
            announced,
        };
        if self.notices.get(&anchor) != Some(&notice) {
            self.notices.insert(anchor, notice);
            self.notice_generation += 1;
        }
    }

    /// 같은 매핑으로 연결했을 때 anchor의 안내를 지운다.
    pub fn clear_notice(&mut self, anchor: u32) {
        if self.notices.remove(&anchor).is_some() {
            self.notice_generation += 1;
        }
    }

    /// 사용자가 안내가 있는 워크스페이스를 활성화했다. 이제 배너를 띄울 수 있다.
    pub fn announce(&mut self, anchor: u32) {
        if let Some(notice) = self.notices.get_mut(&anchor)
            && !notice.announced
        {
            notice.announced = true;
            self.notice_generation += 1;
        }
    }

    pub fn notice_generation(&self) -> u64 {
        self.notice_generation
    }

    /// 지금 매핑이 안내를 만든 매핑과 같은 anchor만 남긴다. 워크스페이스가 닫혔으면 `None`이다.
    /// 지운 anchor가 있으면 true다.
    pub fn retain_current_notices<'a>(
        &mut self,
        current: impl Fn(u32) -> Option<&'a WorkspaceAttachMapping>,
    ) -> bool {
        let before = self.notices.len();
        self.notices
            .retain(|anchor, notice| current(*anchor) == Some(&notice.mapping));
        let removed = before != self.notices.len();
        if removed {
            self.notice_generation += 1;
        }
        removed
    }

    pub fn notice(&self, anchor: u32) -> Option<&MappingNotice> {
        self.notices.get(&anchor)
    }

    /// anchor 순서로 정렬한 안내.
    pub fn notices(&self) -> Vec<(u32, &MappingNotice)> {
        let mut all: Vec<_> = self.notices.iter().map(|(a, n)| (*a, n)).collect();
        all.sort_by_key(|(a, _)| *a);
        all
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
            via_ssh: false,
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
    fn a_notice_stays_until_its_mapping_changes_or_goes() {
        let mut refusals = Refusals::default();
        let refused = inline("127.0.0.1:4000");
        refusals.note(7, MappingNoticeKind::SelfInstance, &refused);
        assert_eq!(
            refusals.notice(7).map(|n| n.target.as_str()),
            Some("127.0.0.1:4000")
        );
        let noted = refusals.notice_generation();
        refusals.note(7, MappingNoticeKind::SelfInstance, &refused);
        assert_eq!(
            refusals.notice_generation(),
            noted,
            "the same notice is not news"
        );

        assert!(!refusals.retain_current_notices(|_| Some(&refused)));
        assert!(refusals.notice(7).is_some());

        let changed = inline("127.0.0.1:4001");
        assert!(refusals.retain_current_notices(|_| Some(&changed)));
        assert!(refusals.notice(7).is_none());

        refusals.note(7, MappingNoticeKind::SelfInstance, &refused);
        assert!(refusals.retain_current_notices(|_| None));
        assert!(refusals.notices().is_empty());
    }

    #[test]
    fn only_a_user_activation_announces_a_notice() {
        let mut refusals = Refusals::default();
        let refused = inline("127.0.0.1:4000");
        // 에이전트가 활성 워크스페이스에 매핑을 걸어 바로 시작한 시도.
        refusals.begin_attempt(7, false);
        refusals.note(7, MappingNoticeKind::SelfInstance, &refused);
        assert!(!refusals.notice(7).unwrap().announced);
        refusals.announce(7);
        assert!(refusals.notice(7).unwrap().announced);
        // 배경 재시도가 같은 안내를 다시 남겨도 알린 상태가 남는다.
        refusals.begin_attempt(7, false);
        refusals.note(7, MappingNoticeKind::SelfInstance, &refused);
        assert!(refusals.notice(7).unwrap().announced);

        refusals.begin_attempt(8, true);
        refusals.note(8, MappingNoticeKind::Unresolved, &inline("build-eu:7420"));
        assert!(refusals.notice(8).unwrap().announced);
    }

    #[test]
    fn a_profile_notice_names_the_profile_and_clears_on_connect() {
        let mut refusals = Refusals::default();
        let profile = WorkspaceAttachMapping::profile("prod-web", Some(1));
        refusals.note(3, MappingNoticeKind::ProfileMissing, &profile);
        let notice = refusals.notice(3).expect("noted");
        assert_eq!(notice.target, "prod-web");
        assert_eq!(notice.kind, MappingNoticeKind::ProfileMissing);
        refusals.clear_notice(3);
        assert!(refusals.notice(3).is_none());
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
