//! plugin webview 에서 **사람이 낸** navigation 의 기록.
//!
//! plugin 은 자기 webview 안의 링크 클릭을 `webview.navigation_attempt` 로 받고, 그 결과로 host 를
//! 부른다(markdown 문서 안의 파일 링크 → `file_handler.dispatch`). host 가 그 호출을 사용자 행동으로
//! 칠 근거는 plugin 의 주장이 아니라 host 가 직접 본 두 사실이다 — native backend 가 그 시도를
//! 사용자 제스처로 봤고([`crate::webview::PendingNavigation::user_gesture`]), 그 surface 의 지금
//! 페이지를 쓴 `webview.set_url` 호출자가 소유 plugin 이었다([`NavigationOwner::wrote_page`]). release
//! 에는 webview 입력 주입이 없으므로(원칙 1 ②) 제스처는 사람이 만든 것이다.
//!
//! 기록은 surface 마다 **가장 최근 시도 한 건에 대한 것**이고 **한 번 쓰면 사라진다.** 근거가 못 되는
//! 시도(제스처가 아님 · 소유 plugin 이 쓴 페이지가 아님 · 통지할 소유자가 없음)가 오면 그 surface 의
//! 기록은 **지워진다** — 앞선 클릭의 기록이 남아 있으면, 그 뒤 에이전트가 `webview.set_url` 로 쓴
//! 스크립트가 같은 URL 의 navigation 을 내고 plugin 이 그것을 되대어 사용자 행동을 얻는다. 작성자는
//! host 가 시도를 drain 하는 시점에 읽으므로, 직전 drain 이후 작성자가 소유 plugin 으로 **바뀐** surface 는
//! 그 프레임의 기록도 버린다([`settle_frame`]). plugin 이
//! 사용자 행동을 주장하려면 그 시도의 URL 을 그대로 되대야 한다 — 그 surface 를 소유한 plugin 만,
//! 그 한 번만. 근거·대안은
//! `docs/adr/0631-file-handler-routing.md`.

use std::collections::HashMap;

use crate::webview::PendingNavigation;

/// surface 하나의 아직 안 쓴 사용자 navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserNavigation {
    /// host 가 이 시도를 통지한 plugin — 그 surface 의 소유자다.
    pub(crate) plugin_id: String,
    pub(crate) url: String,
}

/// surface id → 그 surface 의 마지막 사용자 navigation.
pub(crate) type UserNavigations = HashMap<u32, UserNavigation>;

/// host 가 시도를 통지한 plugin 과, 그 plugin 이 그 surface 의 지금 페이지를 썼는가.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavigationOwner {
    /// 그 surface 의 소유 plugin — host 가 시도를 통지한 곳이다.
    pub(crate) plugin_id: String,
    /// 지금 페이지를 쓴 `webview.set_url` 호출자가 이 plugin 이었는가. 외부 호출자가 쓴 페이지
    /// 위의 클릭은 무엇을 열지를 그 호출자가 정한 것이라 근거가 못 된다.
    pub(crate) wrote_page: bool,
}

/// native backend 가 낸 시도 하나를 반영한다. `owner` 는 host 가 그 시도를 통지한 plugin 이다.
///
/// 그 시도가 근거가 되면 surface 의 기록을 그것으로 바꾸고, 못 되면 그 surface 의 기록을 **지운다.**
/// 근거가 되는 것은 셋이 다 맞을 때뿐이다: 엔진이 사용자 제스처로 봤다, 통지할 소유자가 있다, 그
/// 소유자가 지금 페이지를 썼다.
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

/// 한 프레임의 시도를 [`record`] 로 다 반영한 **뒤** 부른다. 직전 프레임 이후 그 surface 의 페이지
/// 작성자가 소유 plugin 이 아닌 쪽에서 소유 plugin 으로 바뀌었으면(`owner_took_over`) 그 surface 의
/// 기록을 지운다.
///
/// [`record`] 는 작성자를 클릭 시점이 아니라 host 가 시도를 drain 하는 시점에 읽는다. 에이전트가 쓴
/// 페이지 위의 클릭이 drain 전에 소유 plugin 의 재작성으로 덮이면 그 클릭은 소유 페이지 위의 제스처로
/// 보인다 — 이 함수가 그 프레임의 기록을 버린다. 대가는 같은 프레임 간격 안에 에이전트 페이지가 소유
/// 페이지로 다시 바뀐 경우의 정당한 클릭이 에이전트로 떨어지는 것이고, 다음 프레임부터는 영향이 없다.
pub(crate) fn settle_frame(records: &mut UserNavigations, surface_id: u32, owner_took_over: bool) {
    if owner_took_over {
        records.remove(&surface_id);
    }
}

/// `plugin_id` 가 `surface_id` 에서 받은 사용자 navigation `url` 을 근거로 댔다. 맞으면 그 기록을
/// 쓰고(지우고) 참을 낸다. 소유자 · surface · URL 중 하나라도 다르면 기록을 건드리지 않고 거짓이다.
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

    /// 엔진이 사용자 제스처로 안 본 시도 — 사람의 입력 없이 스크립트만으로 낸 것 — 는 근거가 안 된다.
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

    /// 소유 plugin 이 아닌 호출자(에이전트)가 `webview.set_url` 로 쓴 페이지 위의 클릭은 사람이
    /// 눌렀어도 근거가 안 된다 — 무엇을 열지는 그 페이지를 쓴 쪽이 정했다.
    #[test]
    fn a_gesture_on_a_page_the_owner_did_not_write_is_not_recorded() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some(&owner("p", false)), &nav(URL, true));
        assert!(r.is_empty());
        assert!(!take(&mut r, "p", 3, URL));
    }

    /// 근거는 소유자 · surface · URL 셋이 다 맞아야 쓰이고, 어긋난 시도는 기록을 소비하지 않는다.
    #[test]
    fn a_mismatched_claim_neither_succeeds_nor_consumes() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some(&owner("p", true)), &nav(URL, true));
        assert!(!take(&mut r, "other", 3, URL));
        assert!(!take(&mut r, "p", 4, URL));
        assert!(!take(&mut r, "p", 3, "about:blank#tasty-nav:link:b.md"));
        assert!(take(&mut r, "p", 3, URL));
    }

    /// surface 마다 마지막 한 건만 남는다 — 앞선 클릭은 뒤 클릭에 덮인다.
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

    /// drain 전에 소유 plugin 이 페이지를 되찾은 프레임의 기록은 버려진다 — 그 클릭은 에이전트가 쓴
    /// 페이지 위에서 났을 수 있다. 버리는 것은 그 프레임뿐이고, 다음 프레임의 클릭은 다시 근거가
    /// 된다. 다른 surface 의 기록은 그대로다.
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
            "전이가 없는 다음 프레임의 클릭은 근거다"
        );
    }

    /// 근거가 못 되는 시도가 오면 그 surface 의 앞선 기록이 지워진다 — 쓰이지 않은 클릭 기록이
    /// 남아 있다가 뒤의 비-제스처 navigation 이 그 URL 을 되대는 것으로 쓰이지 않게. 다른 surface
    /// 의 기록은 그대로다.
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
