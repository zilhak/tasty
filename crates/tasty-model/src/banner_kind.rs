//! GUI에 의존하지 않는 배너 ID와 표시 범위. 실제 표시 상태와 관리는 호스트 UI가 맡는다.

/// Banner 인스턴스의 고유 식별자. 정의 시점에 고정되는 static 문자열.
///
/// 배너에는 Info/Warning/Error 같은 범용 kind 분류가 없다 — **id 자체가 kind**
/// 역할을 하며, 심각도 표현은 각 배너 정의가 자체적으로 처리한다.
pub type BannerId = &'static str;

/// 배너의 표시 영역과 겹침 우선순위. View > Workspace > Pane > Tab > Surface다.
/// View는 이 타입의 기존 이름이며 팝업의 PopupScope::Window와 별개의 범위다.
/// 상위 범위의 배너가 표시되면 하위 배너는 흐리게 표시한다.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum BannerScope {
    /// View 자체(최상위). 워크스페이스 전환과 무관하게 그 View 의 플레이스홀더
    /// 위치에 유지된다. Modal(Settings/Quit/Plugins) 도 View 의 한 형태라 이 스코프.
    View,
    /// 지정된 워크스페이스가 활성일 때만 표시 (콘텐츠 영역 폭 100%).
    Workspace(usize),
    /// 지정된 pane 이 보일 때만 표시 (pane 영역 폭 100%, 탭바 바로 아래).
    Pane(u32),
    /// 지정된 pane 의 지정된 tab 이 활성일 때만 표시 (pane 영역 폭 100%).
    Tab(u32, usize),
    /// 지정된 surface 가 보일 때만 표시 (surface 영역 폭 100%).
    Surface(u32),
}

impl BannerScope {
    /// 값이 클수록 앞에 표시하며 하위 배너의 흐림 처리에 사용한다.
    pub fn priority(&self) -> u8 {
        match self {
            BannerScope::View => 4,
            BannerScope::Workspace(_) => 3,
            BannerScope::Pane(_) => 2,
            BannerScope::Tab(_, _) => 1,
            BannerScope::Surface(_) => 0,
        }
    }

    /// IPC/CLI 직렬화용 토큰. 사람이 입력·로깅하기 쉬운 콜론 구분 형식.
    ///
    /// `view` / `workspace:<i>` / `pane:<id>` / `tab:<pane>:<i>` / `surface:<id>`.
    /// [`Self::from_token`] 의 역연산. (GUI 비의존이라 model 에 둔다.)
    pub fn to_token(&self) -> String {
        match self {
            BannerScope::View => "view".to_string(),
            BannerScope::Workspace(i) => format!("workspace:{i}"),
            BannerScope::Pane(id) => format!("pane:{id}"),
            BannerScope::Tab(pane, i) => format!("tab:{pane}:{i}"),
            BannerScope::Surface(id) => format!("surface:{id}"),
        }
    }

    /// [`Self::to_token`] 토큰을 파싱한다. 형식 불일치/숫자 파싱 실패 시 `None`.
    pub fn from_token(token: &str) -> Option<Self> {
        let mut parts = token.split(':');
        match parts.next()? {
            "view" => Some(BannerScope::View),
            "workspace" => Some(BannerScope::Workspace(parts.next()?.parse().ok()?)),
            "pane" => Some(BannerScope::Pane(parts.next()?.parse().ok()?)),
            "tab" => {
                let pane = parts.next()?.parse().ok()?;
                let idx = parts.next()?.parse().ok()?;
                Some(BannerScope::Tab(pane, idx))
            }
            "surface" => Some(BannerScope::Surface(parts.next()?.parse().ok()?)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn priority_orders_view_above_surface() {
        assert!(BannerScope::View.priority() > BannerScope::Workspace(0).priority());
        assert!(BannerScope::Workspace(0).priority() > BannerScope::Pane(1).priority());
        assert!(BannerScope::Pane(1).priority() > BannerScope::Tab(1, 0).priority());
        assert!(BannerScope::Tab(1, 0).priority() > BannerScope::Surface(2).priority());
    }

    #[test]
    fn scope_equality_distinguishes_targets() {
        assert_eq!(BannerScope::Pane(1), BannerScope::Pane(1));
        assert_ne!(BannerScope::Pane(1), BannerScope::Pane(2));
        assert_ne!(BannerScope::Tab(1, 0), BannerScope::Tab(1, 1));
    }

    #[test]
    fn token_roundtrips_every_variant() {
        let cases = [
            BannerScope::View,
            BannerScope::Workspace(2),
            BannerScope::Pane(7),
            BannerScope::Tab(7, 3),
            BannerScope::Surface(42),
        ];
        for scope in cases {
            let token = scope.to_token();
            assert_eq!(
                BannerScope::from_token(&token),
                Some(scope.clone()),
                "{token}"
            );
        }
    }

    #[test]
    fn from_token_rejects_malformed() {
        assert_eq!(BannerScope::from_token("nope"), None);
        assert_eq!(BannerScope::from_token("workspace"), None);
        assert_eq!(BannerScope::from_token("pane:abc"), None);
        assert_eq!(BannerScope::from_token("tab:1"), None);
    }
}
