//! 파일 형식 식별·처리기·열기·드래그.

// 파일 열기는 GUI 전용이며 링크 해석·대상 판정은 headless 검사에서도 사용한다.
#[cfg(any(feature = "gui", test))]
pub mod dispatch;
#[cfg(feature = "gui")]
pub mod drag;
#[cfg(feature = "gui")]
pub mod identify_worker;

pub use tasty_file_format as format;

pub use tasty_file_handler as handler;
