//! 갤러리 primitive specimen 용 mock 글리프 — 디자인 `icons.json` canonical path 미러.
//!
//! 갤러리는 본체 `icons.rs` 에 의존할 수 없어(별도 binary) 필요한 glyph 만 로컬에
//! 복제한다. path 값은 디자인 `icons.json`(2026-06-21 추가) 의 canonical 정의.

/// `<svg>` children 을 담은 mock 아이콘. `image()` 로 tint 된 egui 이미지를 만든다.
#[derive(Clone, Copy)]
pub struct MockGlyph {
    uri: &'static str,
    svg: &'static str,
}

impl MockGlyph {
    /// `size` 정사각, `color` tint 의 egui 이미지(painter 클로저에서 `paint_at`).
    pub fn image(self, size: f32, color: egui::Color32) -> egui::Image<'static> {
        egui::Image::from_bytes(self.uri, self.svg.as_bytes())
            .fit_to_exact_size(egui::vec2(size, size))
            .tint(color)
    }
}

macro_rules! glyph {
    ($name:ident, $uri:literal, $body:literal) => {
        pub const $name: MockGlyph = MockGlyph {
            uri: concat!("bytes://gallery_prim_glyph_", $uri, ".svg"),
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
        };
    };
}

glyph!(SEARCH, "search", r#"<circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>"#);
glyph!(PLUS, "plus", r#"<path d="M12 5v14M5 12h14"/>"#);
glyph!(CLOSE, "close", r#"<path d="M18 6 6 18M6 6l12 12"/>"#);
glyph!(TERMINAL, "terminal", r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#);
glyph!(SETTINGS, "settings", r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#);
glyph!(TRASH, "trash", r#"<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#);
glyph!(FOLDER, "folder", r#"<path d="M4 20h16a1 1 0 0 0 1-1V8a1 1 0 0 0-1-1h-7l-2-2H4a1 1 0 0 0-1 1v13a1 1 0 0 0 1 1z"/>"#);
glyph!(FILE, "file", r#"<path d="M14 3v4a1 1 0 0 0 1 1h4"/><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z"/>"#);
