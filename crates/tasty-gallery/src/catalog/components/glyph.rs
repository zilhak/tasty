//! 갤러리 primitive specimen 이 쓰는 글리프 — **canonical 지오메트리는 `tasty-icons`
//! 크레이트(수기 전사)가 소유**한다. `catalog::icons` 는 그걸 재노출·전시할 뿐.
//!
//! 여기서는 `catalog::icons`(= `tasty-icons` 재노출)에서 primitive 가 참조하는 글리프만
//! 다시 재노출한다(중복 path 정의 없음).

pub use crate::catalog::icons::{
    ALERT_CIRCLE, ALERT_TRIANGLE, ARROW_RIGHT, CLOSE, COPY, FILE, FOLDER, FOLDER_OPEN, MARKDOWN,
    MockGlyph, PLUG, PLUS, SEARCH, SETTINGS, SPLIT, TERMINAL, TRASH,
};
