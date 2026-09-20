//! 파일 작업 — 형식 식별 / 핸들러 / 드래그 / 디스패치.

pub mod dispatch;
#[cfg(feature = "gui")]
pub mod drag;
#[cfg(feature = "gui")]
pub mod identify_worker;
/// 파일 피커 path bar 의 치수. `tasty-ui-widgets` 를 링크하므로 gui 조합에만 있다.
#[cfg(feature = "gui")]
pub mod picker_caps;

/// 파일 형식 식별 — 실체는 `tasty-file-format` 크레이트에 있다(호스트 결합이 0 이라
/// leaf 로 올렸다). 본체 코드가 `crate::file::format::…` 로 계속 부르도록 이름만 잇는다.
pub use tasty_file_format as format;

/// 파일 핸들러 정책·레지스트리 — 실체는 `tasty-file-handler` 크레이트에 있다(호스트·GUI
/// 결합이 0 이라 도메인-IO 로 내렸다). 본체 코드가 `crate::file::handler::…` 로 계속
/// 부르도록 이름만 잇는다.
pub use tasty_file_handler as handler;
