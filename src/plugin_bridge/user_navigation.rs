//! 플러그인이 파일 열기를 사용자 동작으로 요청할 때 확인할 navigation 기록.
//! 네이티브 백엔드의 user_gesture와 페이지 작성자가 소유 플러그인인지 함께 확인한다.
//! surface마다 마지막 시도만 저장하고 플러그인·surface·URL이 맞으면 한 번 소비한다.
//! 조건이 맞지 않는 시도와 작성자가 바뀐 프레임은 이전 기록도 지운다.
//! 상세 규칙: docs/adr/0031-file-handler-routing.md.

use std::collections::HashMap;

use crate::webview::PendingNavigation;

/// 아직 소비하지 않은 navigation 기록.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserNavigation {
    pub(crate) plugin_id: String,
    pub(crate) url: String,
}

pub(crate) type UserNavigations = HashMap<u32, UserNavigation>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavigationOwner {
    pub(crate) plugin_id: String,
    /// URL을 설정한 호출자가 이 surface의 소유 플러그인인지 나타낸다.
    pub(crate) wrote_page: bool,
}

/// 사용자 제스처이며 소유 플러그인이 쓴 페이지인 경우에만 기록한다. 나머지는 이전 기록을 지운다.
pub(crate) fn record(
    records: &mut UserNavigations,
    surface_id: u32,
    owner: Option<&NavigationOwner>,
    nav: &PendingNavigation,
) {
    match owner {
        Some(owner) if nav.user_gesture && owner.wrote_page => {
            records.insert(
                surface_id,
                UserNavigation {
                    plugin_id: owner.plugin_id.clone(),
                    url: nav.url.clone(),
                },
            );
        }
        _ => {
            records.remove(&surface_id);
        }
    }
}

/// 해당 프레임의 navigation을 기록한 뒤, 외부 작성자에서 소유 플러그인으로 바뀐 경우 기록을 버린다.
/// 작성자는 클릭 때가 아니라 요청 처리 때 읽기 때문에 필요한 보정이다.
/// 같은 프레임의 정상 클릭도 제외될 수 있으며 다음 프레임에는 영향을 주지 않는다.
pub(crate) fn settle_frame(records: &mut UserNavigations, surface_id: u32, owner_took_over: bool) {
    if owner_took_over {
        records.remove(&surface_id);
    }
}

/// 플러그인·surface·URL이 모두 일치하면 기록을 소비한다. 불일치하면 기록을 유지한다.
pub(crate) fn take(
    records: &mut UserNavigations,
    plugin_id: &str,
    surface_id: u32,
    url: &str,
) -> bool {
    let matches = records
        .get(&surface_id)
        .is_some_and(|r| r.plugin_id == plugin_id && r.url == url);
    if matches {
        records.remove(&surface_id);
    }
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nav(url: &str, user_gesture: bool) -> PendingNavigation {
        PendingNavigation {
            url: url.to_string(),
            user_gesture,
        }
    }

    fn owner(plugin_id: &str, wrote_page: bool) -> NavigationOwner {
        NavigationOwner {
            plugin_id: plugin_id.to_string(),
            wrote_page,
        }
    }

    const URL: &str = "about:blank#tasty-nav:link:a.md";

    #[test]
    fn a_user_gesture_is_recorded_and_can_be_taken_once() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some(&owner("p", true)), &nav(URL, true));
        assert!(take(&mut r, "p", 3, URL));
        assert!(!take(&mut r, "p", 3, URL), "기록은 한 번 쓰면 사라진다");
    }

    #[test]
    fn a_navigation_without_a_user_gesture_is_not_recorded() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some(&owner("p", true)), &nav(URL, false));
        assert!(r.is_empty());
        assert!(!take(&mut r, "p", 3, URL));
    }

    #[test]
    fn a_navigation_without_an_owner_is_not_recorded() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, None, &nav(URL, true));
        assert!(r.is_empty());
    }

    #[test]
    fn a_gesture_on_a_page_the_owner_did_not_write_is_not_recorded() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some(&owner("p", false)), &nav(URL, true));
        assert!(r.is_empty());
        assert!(!take(&mut r, "p", 3, URL));
    }

    #[test]
    fn a_mismatched_claim_neither_succeeds_nor_consumes() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some(&owner("p", true)), &nav(URL, true));
        assert!(!take(&mut r, "other", 3, URL));
        assert!(!take(&mut r, "p", 4, URL));
        assert!(!take(&mut r, "p", 3, "about:blank#tasty-nav:link:b.md"));
        assert!(take(&mut r, "p", 3, URL));
    }

    #[test]
    fn a_later_user_gesture_replaces_the_earlier_one() {
        let mut r = UserNavigations::new();
        record(
            &mut r,
            3,
            Some(&owner("p", true)),
            &nav("about:blank#1", true),
        );
        record(
            &mut r,
            3,
            Some(&owner("p", true)),
            &nav("about:blank#2", true),
        );
        assert!(!take(&mut r, "p", 3, "about:blank#1"));
        assert!(take(&mut r, "p", 3, "about:blank#2"));
    }

    #[test]
    fn an_owner_takeover_drops_only_that_frames_record() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some(&owner("p", true)), &nav(URL, true));
        record(&mut r, 4, Some(&owner("p", true)), &nav(URL, true));
        settle_frame(&mut r, 3, true);
        settle_frame(&mut r, 4, false);
        assert!(!take(&mut r, "p", 3, URL));
        assert!(take(&mut r, "p", 4, URL));

        record(&mut r, 3, Some(&owner("p", true)), &nav(URL, true));
        settle_frame(&mut r, 3, false);
        assert!(
            take(&mut r, "p", 3, URL),
            "작성자 전이가 없는 다음 프레임의 클릭 기록을 사용할 수 있다"
        );
    }

    // 조건에 맞지 않는 새 시도가 이전 클릭 기록을 재사용하지 못하게 한다.
    #[test]
    fn an_attempt_that_is_not_evidence_clears_the_earlier_record() {
        for (who, gesture) in [
            (Some(owner("p", true)), false),
            (Some(owner("p", false)), true),
            (None, true),
        ] {
            let mut r = UserNavigations::new();
            record(&mut r, 3, Some(&owner("p", true)), &nav(URL, true));
            record(&mut r, 4, Some(&owner("p", true)), &nav(URL, true));
            record(&mut r, 3, who.as_ref(), &nav(URL, gesture));
            assert!(!take(&mut r, "p", 3, URL), "{who:?} gesture={gesture}");
            assert!(take(&mut r, "p", 4, URL));
        }
    }
}
