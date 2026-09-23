//! plugin webview 에서 **사람이 낸** navigation 의 기록.
//!
//! plugin 은 자기 webview 안의 링크 클릭을 `webview.navigation_attempt` 로 받고, 그 결과로 host 를
//! 부른다(markdown 문서 안의 파일 링크 → `file_handler.dispatch`). host 가 그 호출을 사용자 행동으로
//! 칠 근거는 plugin 의 주장이 아니라 **엔진의 보고**다 — native backend 가 그 시도를 사용자
//! 제스처로 봤을 때만([`crate::webview::PendingNavigation::user_gesture`]) 여기에 오른다. release 에는
//! webview 입력 주입이 없으므로(원칙 1 ②) 여기 오른 시도는 사람이 만든 것이다.
//!
//! 기록은 surface 마다 **마지막 한 건**이고 **한 번 쓰면 사라진다.** plugin 이 사용자 행동을
//! 주장하려면 그 시도의 URL 을 그대로 되대야 한다 — 그 surface 를 소유한 plugin 만, 그 한 번만.
//! 근거·대안은 `docs/adr/0568-a-user-gesture-navigation-in-a-plugin-webview-makes-its-file-dispatch-a-user-action.md`.

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

/// native backend 가 낸 시도 하나를 기록한다. `owner` 는 host 가 그 시도를 통지한 plugin 이다 —
/// 통지할 소유자가 없으면(surface 가 이미 사라졌거나 plugin surface 가 아니면) 기록하지 않는다.
/// 사용자 제스처가 아닌 시도는 기록을 바꾸지 않는다 — 그 surface 의 앞선 클릭 기록은 그대로다.
pub(crate) fn record(
    records: &mut UserNavigations,
    surface_id: u32,
    owner: Option<&str>,
    nav: &PendingNavigation,
) {
    if !nav.user_gesture {
        return;
    }
    let Some(plugin_id) = owner else {
        return;
    };
    records.insert(
        surface_id,
        UserNavigation {
            plugin_id: plugin_id.to_string(),
            url: nav.url.clone(),
        },
    );
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

    const URL: &str = "about:blank#tasty-nav:link:a.md";

    #[test]
    fn a_user_gesture_is_recorded_and_can_be_taken_once() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some("p"), &nav(URL, true));
        assert!(take(&mut r, "p", 3, URL));
        assert!(!take(&mut r, "p", 3, URL), "기록은 한 번 쓰면 사라진다");
    }

    /// 엔진이 사용자 제스처로 안 본 시도 — plugin 의 페이지 스크립트가 낸 것 — 는 근거가 안 된다.
    #[test]
    fn a_navigation_without_a_user_gesture_is_not_recorded() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some("p"), &nav(URL, false));
        assert!(r.is_empty());
        assert!(!take(&mut r, "p", 3, URL));
    }

    #[test]
    fn a_navigation_without_an_owner_is_not_recorded() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, None, &nav(URL, true));
        assert!(r.is_empty());
    }

    /// 근거는 소유자 · surface · URL 셋이 다 맞아야 쓰이고, 어긋난 시도는 기록을 소비하지 않는다.
    #[test]
    fn a_mismatched_claim_neither_succeeds_nor_consumes() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some("p"), &nav(URL, true));
        assert!(!take(&mut r, "other", 3, URL));
        assert!(!take(&mut r, "p", 4, URL));
        assert!(!take(&mut r, "p", 3, "about:blank#tasty-nav:link:b.md"));
        assert!(take(&mut r, "p", 3, URL));
    }

    /// surface 마다 마지막 한 건만 남는다 — 앞선 클릭은 뒤 클릭에 덮인다.
    #[test]
    fn a_later_user_gesture_replaces_the_earlier_one() {
        let mut r = UserNavigations::new();
        record(&mut r, 3, Some("p"), &nav("about:blank#1", true));
        record(&mut r, 3, Some("p"), &nav("about:blank#2", true));
        record(&mut r, 3, Some("p"), &nav("about:blank#3", false));
        assert!(!take(&mut r, "p", 3, "about:blank#1"));
        assert!(take(&mut r, "p", 3, "about:blank#2"));
    }
}
