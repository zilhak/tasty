//! mirror 터미널 크기 동기화 실패의 창별 표시 상태. 실패 목록은 App이 세션마다 넘기고,
//! 이 창의 활성 워크스페이스가 그 mirror면 Workspace 배너로 보인다.
//! 이름은 그릴 때 이 창 engine의 탭 제목에서 구한다. 규칙: docs/dev-guide/attach-behavior.md#크기-요청의-응답과-재시도.

use std::collections::HashMap;

use super::banner::{BannerManager, BannerScope, BannerState};

/// 한 mirror 워크스페이스의 실패 상태.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SizeSyncFailure {
    /// 실패했거나 다시 시도 중인 로컬 surface. 원격 ID 순이다.
    pub(crate) surfaces: Vec<u32>,
    /// 배너의 다시 시도 응답을 기다리는 중인지.
    pub(crate) retrying: bool,
}

#[derive(Default)]
pub(crate) struct AttachSizeSyncs {
    by_workspace: HashMap<u32, SizeSyncFailure>,
}

impl AttachSizeSyncs {
    /// App이 계산한 상태로 바꾼다. 내용이 바뀌었으면 true다.
    pub(crate) fn replace(&mut self, failures: HashMap<u32, SizeSyncFailure>) -> bool {
        if self.by_workspace == failures {
            return false;
        }
        self.by_workspace = failures;
        true
    }

    /// 활성 워크스페이스(이 창의 인덱스와 ID)에 맞춰 배너를 표시하거나 거둔다.
    /// `name`은 로컬 surface의 탭 제목이다. 포커스와 선택은 바꾸지 않는다.
    pub(crate) fn sync_banner(
        &self,
        banners: &mut BannerManager,
        active: Option<(usize, u32)>,
        name: impl Fn(u32) -> Option<String>,
    ) {
        let desired = active.and_then(|(index, id)| {
            let failure = self.by_workspace.get(&id)?;
            let names: Vec<String> = failure.surfaces.iter().filter_map(|&s| name(s)).collect();
            if names.is_empty() {
                return None;
            }
            Some(BannerState::attach_size_sync(
                BannerScope::Workspace(index),
                id,
                names,
                failure.retrying,
            ))
        });
        banners.sync_attach_size_sync(desired);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn syncs(surfaces: Vec<u32>, retrying: bool) -> AttachSizeSyncs {
        let mut s = AttachSizeSyncs::default();
        assert!(s.replace(HashMap::from([(7, SizeSyncFailure { surfaces, retrying })])));
        s
    }

    fn name(id: u32) -> Option<String> {
        (id != 99).then(|| format!("tab-{id}"))
    }

    fn shown(banners: &BannerManager) -> Vec<BannerState> {
        banners.shown_banners().cloned().collect()
    }

    #[test]
    fn the_banner_follows_the_active_mirror_with_a_failure() {
        let s = syncs(vec![3, 4], false);
        let mut banners = BannerManager::new();
        s.sync_banner(&mut banners, Some((2, 7)), name);
        let banner = shown(&banners);
        assert_eq!(banner.len(), 1);
        assert_eq!(
            banner[0],
            BannerState::attach_size_sync(
                BannerScope::Workspace(2),
                7,
                vec!["tab-3".into(), "tab-4".into()],
                false
            )
        );

        s.sync_banner(&mut banners, Some((0, 3)), name);
        assert!(shown(&banners).is_empty());
    }

    #[test]
    fn a_changed_retry_state_replaces_the_banner_and_an_unnamed_failure_shows_none() {
        let mut s = syncs(vec![3], false);
        let mut banners = BannerManager::new();
        s.sync_banner(&mut banners, Some((2, 7)), name);
        assert!(!s.replace(HashMap::from([(
            7,
            SizeSyncFailure {
                surfaces: vec![3],
                retrying: false
            }
        )])));
        assert!(s.replace(HashMap::from([(
            7,
            SizeSyncFailure {
                surfaces: vec![3],
                retrying: true
            }
        )])));
        s.sync_banner(&mut banners, Some((2, 7)), name);
        let banner = shown(&banners);
        assert_eq!(banner.len(), 1);
        assert_eq!(
            banner[0],
            BannerState::attach_size_sync(BannerScope::Workspace(2), 7, vec!["tab-3".into()], true)
        );

        let s = syncs(vec![99], false);
        s.sync_banner(&mut banners, Some((2, 7)), name);
        assert!(
            shown(&banners).is_empty(),
            "이름을 찾지 못한 surface만 남으면 거둔다"
        );
    }
}
