//! LogicalPx 간격을 egui에 전달하는 헬퍼. 호출부는 Theme의 간격 값을 사용한다.
//! vspace와 hspace는 같은 add_space를 호출하며 이름으로 의도한 배치 방향을 구분한다.

use tasty_type_geometry::length::LogicalPx;

/// 세로 간격 (수직 layout 컨텍스트).
pub fn vspace(ui: &mut egui::Ui, px: LogicalPx) {
    ui.add_space(px.0);
}

/// 가로 간격 (`ui.horizontal` 등 수평 layout 컨텍스트).
pub fn hspace(ui: &mut egui::Ui, px: LogicalPx) {
    ui.add_space(px.0);
}

/// 같은 여백을 네 면에 적용한다. egui의 i8 필드로 바꾸기 전에 반올림한다.
pub fn margin_all(px: LogicalPx) -> egui::Margin {
    egui::Margin::same(px.0.round() as i8)
}

/// 좌우 `x` / 상하 `y` 대칭 마진.
pub fn margin_sym(x: LogicalPx, y: LogicalPx) -> egui::Margin {
    egui::Margin::symmetric(x.0.round() as i8, y.0.round() as i8)
}
