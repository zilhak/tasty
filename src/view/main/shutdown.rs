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
        self.webview_any_visible = false;
    }
}
