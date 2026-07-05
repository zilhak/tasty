//! ui_kit line-icon set.
//!
//! 2px stroke line-icon. path 는 디자인 시스템 `ui_kits/terminal/chrome.jsx` 의
//! `ic` 객체에서 그대로 가져왔다. egui_extras 의 svg 로더 (gpu.rs 의
//! `install_image_loaders`) 로 텍스처화하고, `tint` 로 테마 색을 입힌다.
//! stroke 는 white 로 고정 — tint 곱셈으로 임의 테마 색이 된다.

/// 한 개의 line-icon. `svg` 는 완성된 SVG 문서, `uri` 는 egui 이미지 캐시 키.
#[derive(Clone, Copy, Debug)]
pub struct Icon {
    svg: &'static str,
    uri: &'static str,
}

impl Icon {
    /// 정사각 `size` (logical px) + `tint` 색의 egui Image.
    pub fn image(self, size: f32, tint: egui::Color32) -> egui::Image<'static> {
        egui::Image::from_bytes(self.uri, self.svg.as_bytes())
            .fit_to_exact_size(egui::vec2(size, size))
            .tint(tint)
    }
}

macro_rules! line_icon {
    ($name:ident, $uri:literal, $body:literal) => {
        pub const $name: Icon = Icon {
            uri: concat!("bytes://tasty_icon_", $uri, ".svg"),
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
        };
    };
}

/// `line_icon` 과 동일하나 `fill="white"` — 채운 글리프(예: starFill). tint 로 색을 입힌다.
macro_rules! fill_icon {
    ($name:ident, $uri:literal, $body:literal) => {
        pub const $name: Icon = Icon {
            uri: concat!("bytes://tasty_icon_", $uri, ".svg"),
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="white" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
        };
    };
}

line_icon!(
    FOLDER,
    "folder",
    r#"<path d="M4 20h16a1 1 0 0 0 1-1V8a1 1 0 0 0-1-1h-7l-2-2H4a1 1 0 0 0-1 1v13a1 1 0 0 0 1 1z"/>"#
);
// explorer 주소표시줄 앞 폴더 아이콘 (design `ic.folderOpen`).
line_icon!(
    FOLDER_OPEN,
    "folder_open",
    r#"<path d="M3 8a1 1 0 0 1 1-1h5l2 2h7a1 1 0 0 1 1 1v1H3z M3 11h18l-1.5 8a1 1 0 0 1-1 1H5.5a1 1 0 0 1-1-1z"/>"#
);
// explorer view-mode 토글 아이콘 3종 (design `ic.grid/list/detail`).
line_icon!(
    GRID,
    "grid",
    r#"<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/>"#
);
line_icon!(
    LIST_VIEW,
    "list_view",
    r#"<path d="M8 6h13M8 12h13M8 18h13M3 6h.01M3 12h.01M3 18h.01"/>"#
);
line_icon!(
    DETAIL,
    "detail",
    r#"<path d="M3 5h18M3 12h18M3 19h18M3 5v14"/>"#
);
line_icon!(PLUS, "plus", r#"<path d="M12 5v14M5 12h14"/>"#);
// 즐겨찾기(favorites) 사이드바 항목 — lucide star (갤러리 mock STAR 과 동일 path).
line_icon!(
    STAR,
    "star",
    r#"<path d="M12 2l3.09 6.26L22 9.27l-5 4.87 1.18 6.88L12 17.77l-6.18 3.25L7 14.14l-5-4.87 6.91-1.01L12 2z"/>"#
);
// 채운 별 — 즐겨찾기 populated 행 (design `ic.starFill`, accent-warning 색).
fill_icon!(
    STAR_FILL,
    "star_fill",
    r#"<path d="m12 3 2.9 5.9 6.5.9-4.7 4.6 1.1 6.5L12 18l-5.8 3 1.1-6.5L2.6 9.8l6.5-.9z"/>"#
);
line_icon!(
    SETTINGS,
    "settings",
    r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#
);
line_icon!(
    PLUG,
    "plug",
    r#"<path d="M9 2v6M15 2v6M7 8h10v3a5 5 0 0 1-10 0V8zM12 16v6"/>"#
);
line_icon!(
    TOOLS,
    "tools",
    r#"<path d="M14.7 6.3a4 4 0 0 1-5.4 5.4L4 17v3h3l5.3-5.3a4 4 0 0 1 5.4-5.4l-2.7 2.7-2-2 2.7-2.7z"/>"#
);
line_icon!(
    CHEVRONS_LEFT,
    "chevrons_left",
    r#"<path d="m11 17-5-5 5-5M18 17l-5-5 5-5"/>"#
);
line_icon!(
    CHEVRONS_RIGHT,
    "chevrons_right",
    r#"<path d="m13 17 5-5-5-5M6 17l5-5-5-5"/>"#
);
line_icon!(CHEVRON_UP, "chevron_up", r#"<path d="m18 15-6-6-6 6"/>"#);
line_icon!(CHEVRON_DOWN, "chevron_down", r#"<path d="m6 9 6 6 6-6"/>"#);
// 단일 chevron — explorer 툴바 nav back/forward (CHEVRONS_* 더블과 별개).
// path 는 갤러리 `catalog/icons.rs` glyph! 과 동일.
line_icon!(
    CHEVRON_LEFT,
    "chevron_left",
    r#"<path d="m15 18-6-6 6-6"/>"#
);
line_icon!(
    CHEVRON_RIGHT,
    "chevron_right",
    r#"<path d="m9 18 6-6-6-6"/>"#
);
line_icon!(
    TERM,
    "term",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#
);
line_icon!(
    MD,
    "md",
    r#"<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 15V9l2.5 3L12 9v6M16 9v4m0 0 2-2m-2 2-2-2"/>"#
);
line_icon!(
    FILE,
    "file",
    r#"<path d="M14 3v4a1 1 0 0 0 1 1h4"/><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z"/>"#
);
line_icon!(
    IMAGE,
    "image",
    r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-3.1-3.1a2 2 0 0 0-2.8 0L6 21"/>"#
);
line_icon!(CLOSE, "close", r#"<path d="M18 6 6 18M6 6l12 12"/>"#);
// convert popup 의 "현재 kind" 체크마크. raw `✓`(U+2713)는 UI 폰트에 글리프가 없어
// tofu 로 렌더되던 것을 고친다. path 는 갤러리 `catalog/icons.rs` CHECK 와 바이트 동일.
line_icon!(CHECK, "check", r#"<path d="M20 6 9 17l-5-5"/>"#);
line_icon!(
    SPLIT,
    "split",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>"#
);
line_icon!(
    SEARCH,
    "search",
    r#"<circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>"#
);
line_icon!(
    CLIPBOARD,
    "clipboard",
    r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/>"#
);
line_icon!(
    COMMAND,
    "command",
    r#"<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M9 9h6v6H9z"/>"#
);
line_icon!(
    PORT,
    "port",
    r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v3m0 14v3M2 12h3m14 0h3"/>"#
);
line_icon!(
    REFRESH,
    "refresh",
    r#"<path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6"/>"#
);
line_icon!(
    EDIT,
    "edit",
    r#"<path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"/>"#
);
line_icon!(
    TRASH,
    "trash",
    r#"<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#
);
// 헤더용 bare 터미널 프롬프트(`>_`). TERM(박스형)과 달리 디자인 remote_tool.jsx
// 헤더가 쓰는 외곽선 없는 글리프.
line_icon!(
    TERMINAL_PROMPT,
    "terminal_prompt",
    r#"<path d="M4 17l6-6-6-6"/><path d="M12 19h8"/>"#
);
line_icon!(
    EYE,
    "eye",
    r#"<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z"/><circle cx="12" cy="12" r="3"/>"#
);
line_icon!(
    EYE_OFF,
    "eye_off",
    r#"<path d="M9.9 4.2A10.9 10.9 0 0 1 12 4c6.5 0 10 7 10 7a18.5 18.5 0 0 1-2.2 3.2M6.6 6.6A18.5 18.5 0 0 0 2 11s3.5 7 10 7a10.9 10.9 0 0 0 4-.7M3 3l18 18"/>"#
);
// 프로토콜 필터 버튼(remote_tool.jsx ProtocolFilter funnel). 디자인 path 그대로.
line_icon!(FUNNEL, "funnel", r#"<path d="M3 4h18l-7 8v6l-4 2v-8z"/>"#);
// 컬럼 표시/숨김(column chooser) 트리거. 세로 분할 막대 3개(테이블 컬럼 메타포).
line_icon!(
    COLUMNS,
    "columns",
    r#"<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M9 3v18M15 3v18"/>"#
);
// 마우스 캡처 배너 leading 글리프. path 는 갤러리 `catalog/icons.rs` MOUSE 와 동일.
line_icon!(
    MOUSE,
    "mouse",
    r#"<rect x="6" y="3" width="12" height="18" rx="6"/><path d="M12 7v4"/>"#
);
// webview(html) surface chrome 의 region/placeholder 글리프. path 는 갤러리
// `catalog/icons.rs` GLOBE 와 동일.
line_icon!(
    GLOBE,
    "globe",
    r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18 14 14 0 0 1 0-18z"/>"#
);
// webview(html) surface 의 error chrome 실패 글리프. path 는 갤러리
// `catalog/icons.rs` ALERT_CIRCLE 와 바이트 동일.
line_icon!(
    ALERT_CIRCLE,
    "alert_circle",
    r#"<circle cx="12" cy="12" r="9"/><path d="M12 8v4m0 4h.01"/>"#
);
// ── Scripts (Misc · Scripts, Lua 관리 창 05) ──
// 등록 스크립트 행 글리프 / 빈 상태. 디자인 `settings_window.jsx` SD.script.
// path 는 갤러리 `catalog/icons.rs` SCRIPT 와 바이트 동일.
line_icon!(
    SCRIPT,
    "script",
    r#"<path d="M14 3v4a1 1 0 0 0 1 1h4"/><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z"/><path d="M9 13h4M9 17h6"/>"#
);
// bind-shortcut IconButton (→ Keybindings). 디자인 `settings_window.jsx` SD.kbd.
// path 는 갤러리 `catalog/icons.rs` KEYBOARD 와 바이트 동일.
line_icon!(
    KEYBOARD,
    "keyboard",
    r#"<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M8 14h8"/>"#
);
// changed 배지의 warn 삼각 글리프. path 는 갤러리 `catalog/icons.rs` ALERT_TRIANGLE 와
// 바이트 동일.
line_icon!(
    ALERT_TRIANGLE,
    "alert_triangle",
    r#"<path d="M10.3 3.9 1.8 18a1 1 0 0 0 .9 1.5h18.6a1 1 0 0 0 .9-1.5L13.7 3.9a1 1 0 0 0-1.7 0z"/><path d="M12 9v4M12 17h.01"/>"#
);
