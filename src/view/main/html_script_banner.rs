//! html surface 스크립트 차단 배너의 발화 판정을 surface 상태에 연결한다(ADR-0053).
//!
//! 판정 규칙은 `tasty_model::html_script::banner`에 있다. 여기서는 호스트 신호만 모은다.
//! - 호스트가 webview에 URL을 넣는 로드.
//! - 포커스된 surface가 바뀐 순간. release의 에이전트 경로는 탭·surface 선택을 바꾸지 않으므로
//!   이 전이는 사용자 선택으로 본다. 창을 처음 그리는 프레임의 포커스는 선택으로 세지 않는다.
//!
//! 배너를 그리거나 포커스·활성 탭을 바꾸지 않는다.

use tasty_model::html_script::BannerPhase;

use super::MainView;

impl MainView {
    /// 호스트가 이 surface의 webview에 URL을 넣었다.
    pub(super) fn note_host_webview_load(&self, engine: &crate::core::CoreState, sid: u32) {
        if let Some(rs) = self.find_remote_surface(engine, sid) {
            rs.with_html_script(|st| st.on_host_load_requested());
        }
    }

    /// 배너가 차지하는 높이(물리 px). 배너가 없거나 아직 그리지 않았으면 0이다.
    /// 배너 카드는 egui 패스가 그리고 이 값은 그 패스가 기록한 높이를 따른다.
    pub(super) fn html_script_banner_top(&self, sid: u32, scale_factor: f64) -> f64 {
        self.state
            .html_script_banner_insets
            .get(&sid)
            .map_or(0.0, |inset| {
                f64::from(inset.to_physical(scale_factor as f32).value())
            })
    }

    /// 포커스 전이를 사용자 선택으로 알리고 html surface마다 배너 단계를 갱신한다.
    /// 단계가 바뀌면 다음 프레임에 egui가 배너를 다시 그리도록 dirty를 세운다.
    pub(super) fn update_html_script_banners(
        &mut self,
        engine: &mut crate::core::CoreState,
        all_html_ids: &[u32],
    ) {
        self.note_user_selection(engine);
        let mut phases = std::collections::HashMap::new();
        for &sid in all_html_ids {
            let Some(phase) = self.update_html_script_banner(engine, sid) else {
                continue;
            };
            if phase != BannerPhase::Hidden {
                phases.insert(sid, phase);
            }
        }
        if phases != self.html_script_phases {
            self.base.state.dirty = true;
        }
        self.html_script_phases = phases;
    }

    /// 포커스된 surface가 바뀌었으면 새 surface에 사용자 선택을 알린다.
    fn note_user_selection(&mut self, engine: &mut crate::core::CoreState) {
        let focused = self.state.focused_surface_id(&*engine);
        let selected = match self.html_script_seen_focus.replace(focused) {
            Some(prev) if prev != focused => focused,
            _ => None,
        };
        if let Some(sid) = selected
            && let Some(rs) = self.find_remote_surface(engine, sid)
            && rs.kind_static == "html"
        {
            tracing::debug!("html script banner: surface {sid} selected by the user");
            rs.with_html_script(|st| st.on_user_view());
        }
    }

    /// html surface 하나의 배너 단계를 갱신하고 바뀌었으면 기록한다.
    fn update_html_script_banner(
        &self,
        engine: &crate::core::CoreState,
        sid: u32,
    ) -> Option<BannerPhase> {
        let rs = self
            .find_remote_surface(engine, sid)
            .filter(|rs| rs.kind_static == "html")?;
        let phase = rs.with_html_script(|st| st.update_banner());
        let before = self
            .html_script_phases
            .get(&sid)
            .copied()
            .unwrap_or(BannerPhase::Hidden);
        if phase != before {
            tracing::debug!(
                "html script banner: surface {sid} {} -> {}",
                before.as_str(),
                phase.as_str()
            );
        }
        Some(phase)
    }
}
