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
