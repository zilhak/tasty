#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
// 이유: 테스트 본문의 `let _ =` 는 정책이 사유를 요구하지 않는 자리라
// `clippy::let_underscore_must_use` 명부에 섞이면 안 된다 — 그 명부는 프로덕션에서
// 값을 버리는 자리의 목록이고, 테스트가 늘 때마다 숫자만 흔들리면 새 프로덕션
// 자리가 그 안에 묻힌다(docs/dev-guide/error-handling.md). `cfg_attr(test, ..)` 라
// 라이브러리 타깃의 판정은 그대로다 — 프로덕션 자리는 여전히 명부에 오른다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]
// 이유: headless(no-default-features) 빌드는 gui 사용처가 사라져 코어 타입 다수가 dead 로
// 판정된다 — 컴파일 가드 빌드일 뿐이므로 headless 한정 침묵 (handler.rs 의 모듈 단위
// 관행을 crate 단위로 일반화). gui 빌드에서는 dead_code=deny 가 그대로 강제된다.
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

mod adapters;
mod app;
mod boot;
// 셀 색 해석 — gui 게이트 밖이다. 렌더러가 쓰지만 순수 계산이라
// headless 의 `debug.glyph_color` 도 같은 함수로 답한다.
mod cell_palette;
mod clipboard;
mod close_trace;
mod completion_strategy;
mod core;
mod db;
#[cfg(test)]
mod design_token_guard;
#[cfg(test)]
mod dpi_conversion_guard;
mod file;
mod fullscreen_stages;
#[cfg(feature = "gui")]
mod gfx;
mod hook_handler;
mod host_api;
mod hub;
mod i18n;
mod intent;
mod model;
mod platform;
mod plugin_bridge;
mod ports;
#[cfg(test)]
mod source_guards;
mod state;
mod store;
#[cfg(test)]
mod test_support;
#[cfg(feature = "gui")]
mod view;
mod waker;
mod webhook;

use anyhow::Result;

/// 락 poison 복구 헬퍼 — 실체는 `tasty-utils` 에 있다(소비 크레이트가 셋이라 leaf 로
/// 올렸다). 본체 코드가 `crate::poison::…` 로 계속 부르도록 이름만 잇는다.
pub(crate) use tasty_utils::poison;

pub use tasty_font as font;
pub use tasty_settings as settings;
#[cfg(feature = "gui")]
use tasty_terminal as terminal;
pub use tasty_themes as theme;
pub use tasty_utils::path as paths;

pub(crate) use crate::core::output_observer;
pub(crate) use crate::core::surface_registry::meta as surface_meta;
pub(crate) use adapters::cli;
pub(crate) use adapters::ipc;
pub(crate) use adapters::plugin;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::input::click_cursor;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::input::double_tap;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::input::shortcuts;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::preset as preset_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::surface::empty as empty_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::surface::explorer as explorer_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::surface::webview_chrome as webview_chrome_ui;
#[cfg(feature = "gui")]
pub(crate) use adapters::ui::terminal_link;
#[cfg(feature = "gui")]
pub(crate) use app::App;
pub(crate) use app::event::AppEvent;
#[cfg(feature = "gui")]
pub(crate) use boot::waker as waker_factory_winit;
#[cfg(feature = "gui")]
pub(crate) use clipboard::ClipboardContext;
#[cfg(feature = "gui")]
pub(crate) use file::dispatch as file_dispatch;
#[cfg(feature = "gui")]
pub(crate) use file::identify_worker;
#[cfg(feature = "gui")]
pub(crate) use gfx::gpu;
#[cfg(feature = "gui")]
pub(crate) use gfx::renderer;
pub(crate) use host_api::hooks;
pub(crate) use host_api::hooks::global as global_hooks;
#[cfg(feature = "gui")]
pub(crate) use host_api::webview;
#[cfg(feature = "gui")]
pub(crate) use platform::app_icon;
pub(crate) use platform::crash_report;
#[cfg(all(debug_assertions, feature = "gui"))]
pub(crate) use platform::debug_info;
#[cfg(all(windows, feature = "gui"))]
pub(crate) use platform::jump_list;
#[cfg(all(target_os = "macos", feature = "gui"))]
pub(crate) use platform::macos_delegate;
#[cfg(feature = "gui")]
pub(crate) use platform::macos_permissions;
#[cfg(feature = "gui")]
pub(crate) use platform::stall_watchdog;
#[cfg(all(
    any(windows, target_os = "macos", target_os = "linux"),
    feature = "gui"
))]
pub(crate) use platform::system_tray;
pub(crate) use state::search as search_state;
#[cfg(feature = "gui")]
pub(crate) use state::selection;
pub(crate) use store::notification;
pub(crate) use store::recent_files;
pub(crate) use store::scrollback as scrollback_store;
#[cfg(feature = "gui")]
pub(crate) use view as window;
#[cfg(feature = "gui")]
pub(crate) use view::plugins::ui as plugins_ui;
#[cfg(feature = "gui")]
pub(crate) use view::settings::ui as settings_ui;

/// dhat heap 프로파일링 (opt-in): `cargo build --features dhat-heap` 빌드에서
/// 전 할당을 계측해 종료 시 `dhat-heap.json` 을 남긴다. UMDH 없는 환경에서의
/// 크로스플랫폼 heap attribution 용 — docs/dev-guide/memory-leak-soak.md 참조.
/// release 배포 경로와 무관 (기본 feature 아님).
#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() -> Result<()> {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();
    boot::run()
}
