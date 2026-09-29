//! 스크립트 차단 배너의 발화 판정(ADR-0053 "배너 표시 시점").
//!
//! 배너는 사용자가 문서를 본 뒤에만 뜬다. 에이전트가 연 문서, 세션 복원, 보이지 않는 탭의 로드는
//! `pending_view`만 기록하고 사용자가 그 surface를 선택할 때 뜬다. 판정은 포커스를 바꾸지 않는다.
//!
//! 호스트는 다음 신호를 알린다.
//! - [`HtmlScriptState::on_host_load_requested`]: 호스트가 URL을 넣어 로드를 시작한다.
//! - [`HtmlScriptState::on_user_view`]: 사용자가 이 surface를 선택했다.
//!
//! 새 문서는 다음 중 하나일 때 본 것으로 한다.
//! - 문서가 없거나 로드 중일 때 사용자가 선택했다.
//! - 사용자가 보던 문서에서 페이지 안 이동(링크·스크립트)으로 바뀌었다. 호스트가 넣은 로드는 제외한다.

use super::{BannerFlags, HtmlScriptState};

/// 배너가 보여야 하는 단계. 그리기와 페이드는 호스트가 한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BannerPhase {
    Hidden,
    /// 스크립트가 차단된 기본 상태.
    Blocked,
    /// 허용한 뒤 재로드가 commit되기 전.
    Reloading,
}

impl BannerPhase {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hidden => "hidden",
            Self::Blocked => "blocked",
            Self::Reloading => "reloading",
        }
    }
}

impl HtmlScriptState {
    /// 호스트가 URL을 넣어 로드를 시작한다. 다음 문서는 이전 문서의 열람을 이어받지 않는다.
    /// 첫 문서 전의 선택은 남긴다. 사용자가 연 파일은 새 탭 선택 뒤에 첫 로드가 온다.
    pub fn on_host_load_requested(&mut self) {
        self.host_requested_load = true;
        if self.current.is_some() {
            self.viewed_before_commit = false;
        }
    }

    /// 사용자가 이 surface를 선택했다. 문서가 없거나 로드 중이면 다음 commit 문서에도 적용한다.
    pub fn on_user_view(&mut self) {
        if self.current.is_some() {
            self.banner.viewed = true;
        }
        if self.current.is_none() || self.load_in_flight {
            self.viewed_before_commit = true;
        }
    }

    /// commit이나 로드 종료로 이번 로드에 걸린 표지를 내린다.
    pub(super) fn end_load_marks(&mut self) {
        self.host_requested_load = false;
        self.viewed_before_commit = false;
        self.reloading_after_allow = false;
    }

    /// 사용자가 이 문서의 배너를 닫았다.
    pub fn dismiss_banner(&mut self) {
        self.banner.dismissed = true;
    }

    /// 사용자가 탭 표지로 배너를 다시 불렀다. 표지를 누른 것 자체가 문서를 본 것이다.
    pub fn reshow_banner(&mut self) {
        self.banner.dismissed = false;
        self.banner.viewed = true;
    }

    /// 전역 sandbox와 관계없이 이 문서에 배너가 필요한지. 닫힘도 반영한다.
    fn wants_banner(&self) -> bool {
        self.sandbox
            && self.current_detection().is_some_and(|d| d.has_scripts())
            && !self.current_is_allowed()
            && !self.banner.dismissed
    }

    /// 표지를 갱신하고 지금 보여야 할 단계를 돌려준다. 호스트가 프레임마다 부른다.
    pub fn update_banner(&mut self) -> BannerPhase {
        let wants = self.wants_banner();
        let BannerFlags {
            viewed,
            pending_view,
            shown,
            ..
        } = &mut self.banner;
        if wants && *viewed {
            *shown = true;
        }
        *pending_view = wants && !*viewed;
        self.banner_phase()
    }

    /// 표지를 바꾸지 않고 지금 단계를 읽는다.
    pub fn banner_phase(&self) -> BannerPhase {
        if !self.sandbox {
            return BannerPhase::Hidden;
        }
        if self.reloading_after_allow && self.banner.shown {
            return BannerPhase::Reloading;
        }
        if self.banner.shown && self.wants_banner() {
            BannerPhase::Blocked
        } else {
            BannerPhase::Hidden
        }
    }
}

#[cfg(test)]
mod tests;
