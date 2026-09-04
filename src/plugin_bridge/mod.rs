//! Plugin manager 가 본 바이너리 도메인 (engine / file / shortcuts / model 등)
//! 과 결합한 코드를 모아 두는 bin-side glue.
//!
//! tasty-host-plugin (manager crate) 가 본 바이너리를 역참조할 수 없으므로,
//! 본 모듈이 *protocol port impl* 의 본 바이너리 잔존 지점 역할을 한다.

#[cfg(feature = "gui")]
pub mod banner_render;
pub mod egui_mesh_surface;
#[cfg(feature = "gui")]
pub mod key_dispatch;
#[cfg(feature = "gui")]
pub mod manifest_validate;
pub mod mesh_forward;
#[cfg(feature = "gui")]
pub mod popup_render;
#[cfg(feature = "gui")]
pub mod remote_kind;
pub mod remote_surface;
#[cfg(feature = "gui")]
pub mod wire_scroll;

// host_cmd / host_actions 는 tasty-host-plugin crate 가 owning (manager 가 채널
// 송신자). 본 바이너리에서는 그대로 같은 경로로 노출하기 위해 re-export.
// host_actions 는 gui-only (keybindings_tab/plugins), host_cmd 는 headless 도
// 사용 (remote_surface).
#[cfg(feature = "gui")]
pub use tasty_host_plugin::host_actions;
pub use tasty_host_plugin::host_cmd;

/// egui 가 준 논리 사각형을 mesh 합성용 물리 사각형으로 올린다.
///
/// plugin mesh(배너·popup)는 egui 좌표로 배치되고 GPU 합성은 물리 픽셀로 하므로 매
/// 프레임 이 경계를 넘는다. 네 변에 각각 `× ppp` 를 곱하던 자리를 한 번의
/// `LogicalRect::to_physical` 로 모은다 — 곱셈이 네 번이면 하나를 빠뜨려도 컴파일이
/// 통과하고, 그 결과는 mesh 가 화면의 엉뚱한 자리에 붙는 형태로만 드러난다.
#[cfg(feature = "gui")]
pub(crate) fn mesh_region_of(
    content_rect: egui::Rect,
    pixels_per_point: f32,
) -> crate::model::PhysicalRect {
    use crate::model::{LogicalPx, LogicalRect};
    LogicalRect {
        x: LogicalPx(content_rect.min.x),
        y: LogicalPx(content_rect.min.y),
        width: LogicalPx(content_rect.width()),
        height: LogicalPx(content_rect.height()),
    }
    .to_physical(pixels_per_point)
}
