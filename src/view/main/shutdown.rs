//! Native child visibility at the irreversible shutdown boundary.

use super::MainView;

impl MainView {
    /// Hide every owned view, including inactive tabs, without destroying it or
    /// requesting keyboard focus. Quit cancellation and minimization never enter
    /// this path; backend destruction retains its existing teardown order.
    pub(crate) fn hide_webviews_for_shutdown(&mut self) {
        for webview in self.webviews.values() {
            webview.set_visible(false);
        }
        // 마지막 오버레이 그림이 종료 화면에 남지 않게 host 도 버린다.
        self.base.gpu.detach_overlay_surface();
        self.state.webview_overlay.host = crate::state::webview_overlay::OverlayHostSlot::Absent;
        self.webview_any_visible = false;
    }
}
