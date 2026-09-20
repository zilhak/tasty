//! 파일 작업 — 형식 식별 / 핸들러 / 드래그 / 디스패치.

pub mod dispatch;
#[cfg(feature = "gui")]
pub mod drag;
pub mod handler;
#[cfg(feature = "gui")]
pub mod identify_worker;

/// 파일 형식 식별 — 실체는 `tasty-file-format` 크레이트에 있다(호스트 결합이 0 이라
/// leaf 로 올렸다). 본체 코드가 `crate::file::format::…` 로 계속 부르도록 이름만 잇는다.
pub use tasty_file_format as format;
