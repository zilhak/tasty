//! 연결하지 않은 자동 attach 매핑의 안내를 정리하고 각 창에 넘긴다.
//! 안내 자체는 `tasty_remote::refusal::Refusals`가 보관한다.

use std::collections::HashMap;

use tasty_remote::refusal::MappingNoticeKind;

use crate::adapters::ui::attach_notice::AttachNoticeView;
use crate::app::App;
use crate::view::ui::View as _;

/// 매핑이 가리키는 SSH 프로필이 없다. 해석 실패 가운데 이것만 안내 이유가 따로 있다.
#[derive(Debug)]
pub(super) struct ProfileNotFound(pub(super) String);

impl std::fmt::Display for ProfileNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&crate::i18n::t_fmt("cli.remote_profile.not_found", &self.0))
    }
}

impl std::error::Error for ProfileNotFound {}

/// 첫 attach의 해석 실패 이유. 자기 자신 거절은 이 경로로 오지 않는다.
pub(super) fn resolve_failure_kind(error: &anyhow::Error) -> MappingNoticeKind {
    if error.downcast_ref::<ProfileNotFound>().is_some() {
        MappingNoticeKind::ProfileMissing
    } else {
        MappingNoticeKind::Unresolved
    }
}

impl App {
    /// 매핑이 바뀌었거나 지워졌거나 워크스페이스가 닫힌 anchor의 안내를 지우고,
    /// 세대가 바뀐 창에 남은 안내를 넘긴다. 창과 parked engine의 매핑을 모두 본다.
    pub(super) fn sync_mapping_notices(&mut self) {
        if !self.remote.refusals.notices().is_empty() {
            let live: HashMap<u32, crate::model::WorkspaceAttachMapping> = self
                .engines()
                .sessions()
                .flat_map(|(_, engine)| {
                    engine
                        .workspaces()
                        .into_iter()
                        .filter_map(|ws| ws.attach_mapping.clone().map(|m| (ws.id, m)))
                        .collect::<Vec<_>>()
                })
                .collect();
            self.remote
                .refusals
                .retain_current_notices(|anchor| live.get(&anchor));
        }
        let generation = self.remote.refusals.notice_generation();
        let notices: HashMap<u32, AttachNoticeView> = self
            .remote
            .refusals
            .notices()
            .into_iter()
            .map(|(anchor, notice)| {
                (
                    anchor,
                    AttachNoticeView {
                        kind: notice.kind,
                        target: notice.target.clone(),
                        announced: notice.announced,
                    },
                )
            })
            .collect();
        for (_, main, _) in self.engines_mut().window_pairs() {
            if main.state.attach_notices.generation() == Some(generation) {
                continue;
            }
            if main
                .state
                .attach_notices
                .replace(generation, notices.clone())
            {
                main.mark_dirty();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_profile_has_its_own_reason_and_other_failures_are_unresolved() {
        let missing = anyhow::Error::new(ProfileNotFound("prod-web".into()));
        assert_eq!(
            resolve_failure_kind(&missing),
            MappingNoticeKind::ProfileMissing
        );
        let tunnel = anyhow::anyhow!("ssh: connect to host build-eu port 22: timed out");
        assert_eq!(resolve_failure_kind(&tunnel), MappingNoticeKind::Unresolved);
    }
}
