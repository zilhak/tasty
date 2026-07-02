//! Typed 간격/마진 헬퍼 — 간격 리터럴 유입 차단의 1차 방어선.
//!
//! UI 간격은 raw f32 가 아니라 `LogicalPx`(`&Theme` 의 `spacing_*` 등)로만
//! 소비한다. `ui.add_space(8.0)` / `Frame::inner_margin(8)` 직접 호출 금지 —
//! 이 파일이 design-tokens 시리즈 03 lint/guard 게이트의 **유일한 허용 지점**이며,
//! 파일 밖의 직접 호출은 게이트가 차단한다.
//!
//! `vspace`/`hspace` 는 기능이 같다(egui `add_space` 는 현재 layout 방향을 따른다)
//! — 이름은 호출부의 의도(세로/가로 간격)를 표기하기 위한 구분이다.

use tasty_type_geometry::length::LogicalPx;

/// 세로 간격 (수직 layout 컨텍스트).
pub fn vspace(ui: &mut egui::Ui, px: LogicalPx) {
    // 시리즈 03 게이트 예외 — typed 헬퍼의 실제 구현 지점 (vspace/hspace 공용 정책).
    ui.add_space(px.0);
}

/// 가로 간격 (`ui.horizontal` 등 수평 layout 컨텍스트).
pub fn hspace(ui: &mut egui::Ui, px: LogicalPx) {
    // 시리즈 03 게이트 예외 — typed 헬퍼의 실제 구현 지점 (vspace/hspace 공용 정책).
    ui.add_space(px.0);
}

/// 4면 동일 마진. egui 0.31 `Margin` 필드는 `i8` — Theme 값은 zoom 반올림을
/// 거치지만 방어적으로 round 후 캐스팅한다 (`tokens.rs` 의 i8 캐스팅 선례).
pub fn margin_all(px: LogicalPx) -> egui::Margin {
    egui::Margin::same(px.0.round() as i8)
}

/// 좌우 `x` / 상하 `y` 대칭 마진.
pub fn margin_sym(x: LogicalPx, y: LogicalPx) -> egui::Margin {
    egui::Margin::symmetric(x.0.round() as i8, y.0.round() as i8)
}
