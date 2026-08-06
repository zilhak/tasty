//! Canonical line/fill icon set — Tasty 의 아이콘 단일 소스.
//!
//! 아이콘 한 개는 `Icon` const 하나로 노출되며, 다음 두 소비 경로가 **바이트 동일한
//! 같은 `<svg>` 문자열**을 공유한다:
//!
//! · host / gallery 런타임 (egui_extras svg 로더): `feature = "egui"` 를 켜고
//!   [`Icon::image`] 으로 `<Image>` 를 만들어 `tint` 로 테마 색을 입힌다.
//! · plugin build.rs (빌드타임 usvg 베이크): `[build-dependencies]` 로 이 크레이트를
//!   egui 없이 링크해 [`Icon::svg`]/[`Icon::body`] 를 usvg 에 먹인다.
//!
//! path 는 디자인 시스템 번들(`Tasty Design System`)의 `icons.json` 매니페스트
//! (개별 `icons/<name>.svg` 글리프의 machine-readable SoT, `components/core/Icon.jsx`
//! `ICON_PATHS` 와 동기) 를 전사했다. 각 항목의 `fill` boolean 으로 stroke/filled 를
//! 분기한다. 24×24 viewBox, 2px stroke, round cap/join. stroke 는 white 로 고정하고
//! 소비처가 `tint` 로 currentColor 를 재현한다 — 색을 글리프에 박지 않는다.

/// line/fill 아이콘 한 개. 아래 필드는 모두 `&'static str`/`bool` 이라 egui 없이도
/// 컴파일된다(build.rs 는 egui 를 링크하지 않고 `svg`/`body` 만 읽는다).
#[derive(Clone, Copy, Debug)]
pub struct Icon {
    /// 완성 `<svg viewBox="0 0 24 24" …>` 문서. egui_extras 로더 + build.rs usvg 가
    /// 이 문자열을 읽는다.
    pub svg: &'static str,
    /// inner 마크업만(`<path>/<rect>/<circle>` 시퀀스). 디자인 `d` 와 바이트 동일 —
    /// 검증/대체 소비자용.
    pub body: &'static str,
    /// egui 이미지 캐시 키(`bytes://tasty_icon_<uri>.svg`).
    pub uri: &'static str,
    /// true = 채운 글리프(`fill="white"`), false = stroke-only.
    pub filled: bool,
}

#[cfg(feature = "egui")]
impl Icon {
    /// 정사각 `size` (logical px) + `tint` 색의 egui `Image`.
    ///
    /// 실제 SVG 텍스처화는 앱이 설치한 egui_extras 로더가 담당한다
    /// (host `install_image_loaders`, gallery 동일).
    pub fn image(self, size: f32, tint: egui::Color32) -> egui::Image<'static> {
        egui::Image::from_bytes(self.uri, self.svg.as_bytes())
            .fit_to_exact_size(egui::vec2(size, size))
            .tint(tint)
    }
}

/// stroke-only 글리프 const 를 만든다(`fill="none" stroke="white"`).
macro_rules! stroke_icon {
    ($name:ident, $uri:literal, $body:literal) => {
        pub const $name: Icon = Icon {
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
            body: $body,
            uri: concat!("bytes://tasty_icon_", $uri, ".svg"),
            filled: false,
        };
    };
}

/// `stroke_icon` 과 동일하나 `fill="white"` — 채운 글리프(예: starFill). tint 로 색을 입힌다.
macro_rules! fill_icon {
    ($name:ident, $uri:literal, $body:literal) => {
        pub const $name: Icon = Icon {
            svg: concat!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="white" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
                $body,
                "</svg>"
            ),
            body: $body,
            uri: concat!("bytes://tasty_icon_", $uri, ".svg"),
            filled: true,
        };
    };
}

// ── Actions ──
stroke_icon!(PLUS, "plus", r#"<path d="M12 5v14M5 12h14"/>"#);
stroke_icon!(CLOSE, "close", r#"<path d="M18 6 6 18M6 6l12 12"/>"#);
stroke_icon!(CHECK, "check", r#"<path d="m20 6-11 11-5-5"/>"#);
stroke_icon!(
    REFRESH,
    "refresh",
    r#"<path d="M21 12a9 9 0 1 1-2.6-6.4M21 3v6h-6"/>"#
);
stroke_icon!(
    EDIT,
    "edit",
    r#"<path d="M12 20h9M16.5 3.5a2.12 2.12 0 0 1 3 3L7 19l-4 1 1-4z"/>"#
);
stroke_icon!(
    TRASH,
    "trash",
    r#"<path d="M3 6h18M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>"#
);
// 클립보드 복사 글리프(design `copy`). 뷰어용 `CLIPBOARD` 와는 별개 글리프.
stroke_icon!(
    COPY,
    "copy",
    r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h8"/>"#
);
stroke_icon!(
    SEARCH,
    "search",
    r#"<circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/>"#
);
// 상태 필터 깔때기(design `filter`).
stroke_icon!(FUNNEL, "funnel", r#"<path d="M3 4h18l-7 8v6l-4 2v-8z"/>"#);
// 컬럼 표시/숨김(column chooser). 세로 분할 막대 3개.
stroke_icon!(
    COLUMNS,
    "columns",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16M15 4v16"/>"#
);
// 방향 전환/교체(design `swap`).
stroke_icon!(SWAP, "swap", r#"<path d="M7 7h11l-3-3M17 17H6l3 3"/>"#);
// 수평 3-dot "더보기" 트리거(design `more`, 마우스 캡처 배너 컨텍스트 메뉴 트리거).
stroke_icon!(MORE, "more", r#"<path d="M5 12h.01M12 12h.01M19 12h.01"/>"#);
// 디스크로 저장/내려받기(design `download`).
stroke_icon!(
    DOWNLOAD,
    "download",
    r#"<path d="M12 3v12m0 0 4-4m-4 4-4-4M4 21h16"/>"#
);

// ── Navigation & disclosure ──
stroke_icon!(
    CHEVRON_LEFT,
    "chevron_left",
    r#"<path d="m15 18-6-6 6-6"/>"#
);
stroke_icon!(
    CHEVRON_RIGHT,
    "chevron_right",
    r#"<path d="m9 18 6-6-6-6"/>"#
);
stroke_icon!(CHEVRON_UP, "chevron_up", r#"<path d="m18 15-6-6-6 6"/>"#);
stroke_icon!(CHEVRON_DOWN, "chevron_down", r#"<path d="m6 9 6 6 6-6"/>"#);
stroke_icon!(
    CHEVRONS_LEFT,
    "chevrons_left",
    r#"<path d="m11 17-5-5 5-5M18 17l-5-5 5-5"/>"#
);
stroke_icon!(
    CHEVRONS_RIGHT,
    "chevrons_right",
    r#"<path d="m13 17 5-5-5-5M6 17l5-5-5-5"/>"#
);
// markdown 주소창 go(design `mdGo`). 신규 글리프.
stroke_icon!(
    ARROW_RIGHT,
    "arrow_right",
    r#"<path d="M5 12h14M13 6l6 6-6 6"/>"#
);
// 4방향 이동/재배치(design `move`).
stroke_icon!(
    MOVE,
    "move",
    r#"<path d="M5 9l-3 3 3 3M9 5l3-3 3 3M15 19l-3 3-3-3M19 9l3 3-3 3M2 12h20M12 2v20"/>"#
);

// ── Surfaces & workspace ──
stroke_icon!(
    TERMINAL,
    "terminal",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="m7 9 3 3-3 3M13 15h4"/>"#
);
stroke_icon!(
    MARKDOWN,
    "markdown",
    r#"<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M7 15V9l2.5 3L12 9v6M16 9v4m0 0 2-2m-2 2-2-2"/>"#
);
stroke_icon!(
    SPLIT,
    "split",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M12 4v16"/>"#
);
stroke_icon!(
    FOLDER,
    "folder",
    r#"<path d="M4 20h16a1 1 0 0 0 1-1V8a1 1 0 0 0-1-1h-7l-2-2H4a1 1 0 0 0-1 1v13a1 1 0 0 0 1 1z"/>"#
);
// explorer 주소표시줄 앞 폴더(design `folderOpen`).
stroke_icon!(
    FOLDER_OPEN,
    "folder_open",
    r#"<path d="m6 14 1.5-2.9A2 2 0 0 1 9.24 10H21a2 2 0 0 1 1.94 2.5l-1.55 6a2 2 0 0 1-1.94 1.5H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h3.9a2 2 0 0 1 1.69.9l.81 1.2a2 2 0 0 0 1.67.9H18a2 2 0 0 1 2 2v2"/>"#
);
stroke_icon!(
    FILE,
    "file",
    r#"<path d="M14 3v4a1 1 0 0 0 1 1h4"/><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z"/>"#
);
stroke_icon!(
    IMAGE,
    "image",
    r#"<rect x="3" y="3" width="18" height="18" rx="2"/><circle cx="9" cy="9" r="2"/><path d="m21 15-5-5L5 21"/>"#
);
// 헤더용 bare 터미널 프롬프트(`>_`, design `remote`). host 는 `TERMINAL_PROMPT` 로 별칭.
stroke_icon!(
    REMOTE,
    "remote",
    r#"<path d="M4 17l6-6-6-6"/><path d="M12 19h8"/>"#
);
stroke_icon!(LOG, "log", r#"<path d="M4 6h16M4 10h16M4 14h10M4 18h7"/>"#);
stroke_icon!(
    PORT,
    "port",
    r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v3m0 14v3M2 12h3m14 0h3"/>"#
);
stroke_icon!(
    HTML,
    "html",
    r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a13.8 13.8 0 0 1 3.6 9 13.8 13.8 0 0 1-3.6 9 13.8 13.8 0 0 1-3.6-9 13.8 13.8 0 0 1 3.6-9z"/>"#
);
// surface 없는 빈 pane(design `paneEmpty`).
stroke_icon!(
    PANE_EMPTY,
    "pane_empty",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M8 12h8"/>"#
);
// 가로 분할선 split(design `splitH`).
stroke_icon!(
    SPLIT_H,
    "split_h",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 12h18"/>"#
);
// 레이아웃 프리셋/적층 레이어(design `layers`).
stroke_icon!(
    LAYERS,
    "layers",
    r#"<path d="m12 2 9 5-9 5-9-5 9-5zM3 12l9 5 9-5M3 17l9 5 9-5"/>"#
);
// 텍스트 콘텐츠/문단(design `textLeft`).
stroke_icon!(
    TEXT_LEFT,
    "text_left",
    r#"<path d="M4 7h16M4 12h16M4 17h10"/>"#
);
// git 브랜치(design `gitBranch`).
stroke_icon!(
    GIT_BRANCH,
    "git_branch",
    r#"<circle cx="6" cy="6" r="2.5"/><circle cx="6" cy="18" r="2.5"/><circle cx="18" cy="9" r="2.5"/><path d="M18 11.5a6 6 0 0 1-6 6H8.5M6 8.5v7"/>"#
);
// git 트리/계보(design `gitTree`).
stroke_icon!(
    GIT_TREE,
    "git_tree",
    r#"<path d="M12 3v6m0 0a3 3 0 1 0 0 6m0-6a3 3 0 1 1 0 6m0 0v6"/>"#
);
// 클립보드 뷰어(design `clipboard`). copy 글리프와 별개 — 뷰어 표면/툴 항목용.
stroke_icon!(
    CLIPBOARD,
    "clipboard",
    r#"<rect x="8" y="3" width="8" height="4" rx="1"/><path d="M9 5H6a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h12a1 1 0 0 0 1-1V6a1 1 0 0 0-1-1h-3"/>"#
);

// ── View modes & favorites (explorer) ──
// host 는 GRID / LIST_VIEW / DETAIL 로 별칭한다.
stroke_icon!(
    LAYOUT_GRID,
    "layout_grid",
    r#"<rect x="3" y="3" width="7" height="7" rx="1"/><rect x="14" y="3" width="7" height="7" rx="1"/><rect x="14" y="14" width="7" height="7" rx="1"/><rect x="3" y="14" width="7" height="7" rx="1"/>"#
);
// design `listView` — 행 + leading 마커 점. host 는 LIST_VIEW 로 별칭. (log 라인은 `LOG`=design `list`.)
stroke_icon!(
    LIST,
    "list",
    r#"<path d="M8 6h13M8 12h13M8 18h13"/><path d="M3 6h.01M3 12h.01M3 18h.01"/>"#
);
stroke_icon!(
    LAYOUT_DETAIL,
    "layout_detail",
    r#"<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M7 13h10M7 16.5h6"/>"#
);
// 즐겨찾기 outline star — design `star`(채운 STAR_FILL 과 같은 지오메트리).
stroke_icon!(
    STAR,
    "star",
    r#"<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.68a2.12 2.12 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.12 2.12 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.12 2.12 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.12 2.12 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.12 2.12 0 0 0 1.597-1.16z"/>"#
);
// 채운 별 — 즐겨찾기 populated 행 (design `starFill`, accent-warning 색).
fill_icon!(
    STAR_FILL,
    "star_fill",
    r#"<path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.68a2.12 2.12 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.12 2.12 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.12 2.12 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.12 2.12 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.12 2.12 0 0 0 1.597-1.16z"/>"#
);

// ── Visibility ──
stroke_icon!(
    EYE,
    "eye",
    r#"<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z"/><circle cx="12" cy="12" r="3"/>"#
);
stroke_icon!(
    EYE_OFF,
    "eye_off",
    r#"<path d="M9.9 4.2A10.9 10.9 0 0 1 12 4c6.5 0 10 7 10 7a18.5 18.5 0 0 1-2.2 3.2M6.6 6.6A18.5 18.5 0 0 0 2 11s3.5 7 10 7a10.9 10.9 0 0 0 4-.7M3 3l18 18"/>"#
);
// 잠김/비공개 보류(design `lock`).
stroke_icon!(
    LOCK,
    "lock",
    r#"<rect x="5" y="11" width="14" height="9" rx="2"/><path d="M8 11V7a4 4 0 0 1 8 0v4"/>"#
);

// ── Status & alerts ──
stroke_icon!(
    ALERT_TRIANGLE,
    "alert_triangle",
    r#"<path d="M10.3 3.9 1.8 18a1 1 0 0 0 .9 1.5h18.6a1 1 0 0 0 .9-1.5L13.7 3.9a1 1 0 0 0-1.7 0z"/><path d="M12 9v4M12 17h.01"/>"#
);
stroke_icon!(
    ALERT_CIRCLE,
    "alert_circle",
    r#"<circle cx="12" cy="12" r="9"/><path d="M12 8v4m0 4h.01"/>"#
);
stroke_icon!(
    SHIELD_CHECK,
    "shield_check",
    r#"<path d="M12 3l7 3v6c0 4-3 6.5-7 9-4-2.5-7-5-7-9V6z"/><path d="M9 12l2 2 4-4"/>"#
);
stroke_icon!(
    BELL,
    "bell",
    r#"<path d="M18 8a6 6 0 0 0-12 0c0 7-3 9-3 9h18s-3-2-3-9M13.7 21a2 2 0 0 1-3.4 0"/>"#
);
stroke_icon!(
    HELP_CIRCLE,
    "help_circle",
    r#"<circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/>"#
);
// 마우스 캡처 배너 leading 글리프.
stroke_icon!(
    MOUSE,
    "mouse",
    r#"<rect x="6" y="3" width="12" height="18" rx="6"/><path d="M12 7v4"/>"#
);

// ── Tools & system ──
stroke_icon!(
    TOOLS,
    "tools",
    r#"<path d="M14.7 6.3a4 4 0 0 1-5.4 5.4L4 17v3h3l5.3-5.3a4 4 0 0 1 5.4-5.4l-2.7 2.7-2-2 2.7-2.7z"/>"#
);
stroke_icon!(
    SETTINGS,
    "settings",
    r#"<circle cx="12" cy="12" r="3"/><path d="M12 2v2m0 16v2M4.9 4.9l1.4 1.4m11.4 11.4 1.4 1.4M2 12h2m16 0h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>"#
);
stroke_icon!(
    PLUG,
    "plug",
    r#"<path d="M9 2v6M15 2v6M7 8h10v3a5 5 0 0 1-10 0V8zM12 16v6"/>"#
);
stroke_icon!(
    ROCKET,
    "rocket",
    r#"<path d="M5 13c-1.5 1.5-2 5-2 5s3.5-.5 5-2a3.5 3.5 0 1 0-3-3zM12 15l-3-3a14 14 0 0 1 9-9 14 14 0 0 1-3 9zM9 12l3 3"/>"#
);
stroke_icon!(
    COMMAND,
    "command",
    r#"<rect x="4" y="4" width="16" height="16" rx="2"/><path d="M9 9h6v6H9z"/>"#
);
// 등록 스크립트 행 글리프(design `scriptFile`). host/gallery 는 SCRIPT 로 쓴다.
stroke_icon!(
    SCRIPT,
    "script",
    r#"<path d="M14 3v4a1 1 0 0 0 1 1h4"/><path d="M17 21H7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h7l5 5v11a2 2 0 0 1-2 2z"/><path d="M9 13h4M9 17h6"/>"#
);
stroke_icon!(
    KEYBOARD,
    "keyboard",
    r#"<rect x="2" y="6" width="20" height="12" rx="2"/><path d="M6 10h.01M10 10h.01M14 10h.01M18 10h.01M8 14h8"/>"#
);
// image 편집 툴바 undo/redo. 신규 글리프(design plugins.jsx ImgBtn).
stroke_icon!(
    UNDO,
    "undo",
    r#"<path d="M9 14 4 9l5-5M4 9h11a5 5 0 0 1 0 10h-1"/>"#
);
stroke_icon!(
    REDO,
    "redo",
    r#"<path d="m15 14 5-5-5-5M20 9H9a5 5 0 0 0 0 10h1"/>"#
);
// 테마 토글 Mocha/Latte(design `theme`).
stroke_icon!(
    THEME,
    "theme",
    r#"<path d="M12 3a9 9 0 1 0 9 9c-2 0-3-1-3-3s1-3-1-5-3-1-4-1z"/>"#
);
// 빈 상태/설정 없음(design `sun`).
stroke_icon!(
    SUN,
    "sun",
    r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2"/>"#
);
// 숫자/탭 전환 digits(design `hash`).
stroke_icon!(
    HASH,
    "hash",
    r#"<path d="M4 9h16M4 15h16M10 3 8 21M16 3l-2 18"/>"#
);
