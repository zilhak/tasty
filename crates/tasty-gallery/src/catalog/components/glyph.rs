//! 갤러리 primitive specimen 이 쓰는 글리프 — **canonical 정의는 `catalog::icons`**.
//!
//! 단일 소스화: 디자인 `icons.json` 29 글리프 전체가 `catalog/icons.rs` 에 정의되어
//! 있고, 여기서는 primitive 가 참조하는 것만 재노출한다(중복 path 정의 제거).

pub use crate::catalog::icons::{
    ALERT_CIRCLE, ALERT_TRIANGLE, CLOSE, COPY, FILE, FOLDER, FOLDER_OPEN, MARKDOWN, MockGlyph,
    PLUG, PLUS, SEARCH, SETTINGS, SPLIT, TERMINAL, TRASH,
};
