//! Banner 분류 enum / id — UI 본문이 아닌 *분류* 만 model 에 둔다.
//!
//! `BannerState` / `BannerManager` (UI 동작 본문) 는 [`crate::adapters::ui::banner`]
//! 에 잔류한다 (Toast 가 `ToastManager` 로, Popup 이 `PopupManager` 로 분리된 것과
//! 동일한 구조). headless 빌드에서도 분류를 참조할 수 있도록 GUI 의존 0 으로 유지.
//!
//! 배너는 Modal / Popup / Toast 에 이은 4번째 오버레이 개념이다. 설계 문서는
//! `docs/design/systems/banner.md`, 결정 근거는
//! `docs/adr/0024-banner-fourth-overlay-concept.md`.

/// Banner 인스턴스의 고유 식별자. 정의 시점에 고정되는 static 문자열.
///
/// 배너에는 Info/Warning/Error 같은 범용 kind 분류가 없다 — **id 자체가 kind**
/// 역할을 하며, 심각도 표현은 각 배너 정의가 자체적으로 처리한다.
pub type BannerId = &'static str;

/// Banner 의 대상 스코프. 어느 영역 위에 떠오르고 어디로 clamp 되는지, 그리고
/// 계층 z-order(상위가 앞)와 디밍을 결정한다.
///
/// ⚠️ 최상위가 [`PopupScope::Window`](crate::popup_kind::PopupScope) 와 달리
/// **`View`** 다. ubiquitous-language 상 `Window` = OS 창이므로, 워크스페이스에
/// 비종속인 View-level 배너는 `View` 로 명명한다. (popup_kind 의 `Window` 는 화면
/// 전체 clamp 라는 다른 의미라 재사용하지 않는다.)
///
/// 계층(상위 → 하위): **View > Workspace > Pane > Tab > Surface**. 상위 배너가
/// 뜨면 하위 배너는 디밍(약 60% 투명)되어 뒤로 물러난다 — [`Self::priority`].
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
    /// 계층 우선순위 — 클수록 상위(앞에, 높은 z-index). 서로 다른 스코프의 배너가
    /// 동시에 떠 있을 때 상·하위 판정과 디밍에 쓴다.
    ///
    /// View(4) > Workspace(3) > Pane(2) > Tab(1) > Surface(0).
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
