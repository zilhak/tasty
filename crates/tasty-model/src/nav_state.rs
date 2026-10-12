//! webview surface 의 탐색(navigation) 생명주기 상태.

/// webview 탐색 상태. native backend가 갱신하고 호스트가 로딩·오류 UI와 표시 여부를 결정한다.
/// 모델과 backend·UI가 함께 쓰므로 OS·GUI 타입을 포함하지 않는다. 실패 사유는 별도 로그에 남긴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NavState {
    /// 아직 navigation 시작 전(URL 미지정 직후). placeholder/boundary chrome.
    #[default]
    Idle,
    /// navigation 진행 중. overlay 숨기고 egui spinner 노출.
    Loading,
    /// navigation 성공 완료. overlay reveal(native 페이지가 보임).
    Done,
    /// navigation 실패. overlay 숨긴 채 error chrome. (사유는 tracing 로그로만)
    Failed,
}

/// 주소 표시용 탐색 정보. backend가 관측한 값을 호스트가 surface에 사본으로 둔다.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WebViewNavInfo {
    /// main frame에 commit된 문서의 주소. 리다이렉트 뒤 최종 주소와 `#fragment`를 포함한다.
    /// 로드가 commit 전에 실패하면 이전 주소가 남는다.
    pub url: Option<String>,
    /// 세션 히스토리에서 뒤로 갈 수 있다.
    pub can_go_back: bool,
    /// 세션 히스토리에서 앞으로 갈 수 있다.
    pub can_go_forward: bool,
}

/// 지금 보일 문서 주소를 고른다.
///
/// 로드가 끝난 상태(`Done`·`Idle`)에서는 엔진의 현재 주소를 쓴다. 같은 문서 안 이동
/// (`#fragment`, history API)은 commit 신호 없이 이 값만 바꾸기 때문이다. 로드 중이거나
/// 실패한 상태에서는 엔진 주소가 commit 전 목적지일 수 있어 마지막 commit 주소를 쓴다.
pub fn committed_document_url(
    nav: NavState,
    live: Option<String>,
    last_commit: Option<String>,
) -> Option<String> {
    match nav {
        NavState::Done | NavState::Idle => live.or(last_commit),
        NavState::Loading | NavState::Failed => last_commit,
    }
}

/// `target`이 지금 문서 안의 `#fragment` 이동인지. 엔진은 이 이동을 새 로드 없이 처리한다.
pub fn is_same_document_fragment_move(current: Option<&str>, target: &str) -> bool {
    use crate::html_script::strip_fragment;
    target.contains('#') && current.is_some_and(|cur| strip_fragment(cur) == strip_fragment(target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fragment_of_the_current_document_is_a_same_document_move() {
        let cur = Some("file:///a.html#top");
        assert!(is_same_document_fragment_move(cur, "file:///a.html#sec"));
        assert!(is_same_document_fragment_move(
            Some("file:///a.html"),
            "file:///a.html#sec"
        ));
        assert!(!is_same_document_fragment_move(cur, "file:///a.html"));
        assert!(!is_same_document_fragment_move(cur, "file:///b.html#sec"));
        assert!(!is_same_document_fragment_move(None, "file:///a.html#sec"));
        // 쿼리나 origin이 다르면 다른 문서다.
        assert!(!is_same_document_fragment_move(
            Some("file:///a.html#s"),
            "file:///a.html?x#s"
        ));
        assert!(!is_same_document_fragment_move(
            Some("https://a.test/p.html"),
            "https://b.test/p.html#s"
        ));
        assert!(!is_same_document_fragment_move(
            Some("http://a.test/p.html"),
            "https://a.test/p.html#s"
        ));
    }

    fn s(v: &str) -> Option<String> {
        Some(v.to_string())
    }

    #[test]
    fn a_finished_load_shows_the_engine_address_with_its_fragment() {
        let url =
            committed_document_url(NavState::Done, s("file:///a.html#x"), s("file:///a.html"));
        assert_eq!(url, s("file:///a.html#x"));
    }

    #[test]
    fn a_load_in_flight_keeps_the_last_committed_address() {
        let url = committed_document_url(
            NavState::Loading,
            s("https://next.example/"),
            s("file:///a.html"),
        );
        assert_eq!(url, s("file:///a.html"));
    }

    #[test]
    fn a_failed_load_keeps_the_previous_document() {
        let url = committed_document_url(
            NavState::Failed,
            s("https://offline.example/"),
            s("file:///a.html"),
        );
        assert_eq!(url, s("file:///a.html"));
        assert_eq!(
            committed_document_url(NavState::Failed, s("https://offline.example/"), None),
            None
        );
    }

    #[test]
    fn a_finished_load_without_an_engine_address_falls_back_to_the_commit() {
        assert_eq!(
            committed_document_url(NavState::Done, None, s("file:///a.html")),
            s("file:///a.html")
        );
        assert_eq!(committed_document_url(NavState::Idle, None, None), None);
    }
}
