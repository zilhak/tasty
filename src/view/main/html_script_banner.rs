//! View-owned banner geometry. Script state and native callback sequencing are App-owned.
use super::MainView;
impl MainView {
    /// 배너가 차지하는 높이(물리 px). 배너가 없거나 아직 그리지 않았으면 0이다.
    /// 배너 카드는 egui 패스가 그리고 이 값은 그 패스가 기록한 높이를 따른다.
    pub(crate) fn html_script_banner_top(&self, sid: u32, scale_factor: f64) -> f64 {
        self.state
            .html_script_banner_insets
            .get(&sid)
            .map_or(0.0, |inset| {
                f64::from(inset.to_physical(scale_factor as f32).value())
            })
    }
}
