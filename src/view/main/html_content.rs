//! View-owned html content geometry. Script state and native callback sequencing are App-owned.
use super::MainView;
impl MainView {
    /// 주소 띠와 스크립트 배너가 차지하는 높이(물리 px). 아직 그리지 않았으면 0이다.
    /// 띠와 배너는 egui 패스가 그리고 이 값은 그 패스가 기록한 높이를 따른다.
    pub(crate) fn html_content_top(&self, sid: u32, scale_factor: f64) -> f64 {
        self.state
            .html_content_insets
            .get(&sid)
            .map_or(0.0, |inset| {
                f64::from(inset.to_physical(scale_factor as f32).value())
            })
    }
}
