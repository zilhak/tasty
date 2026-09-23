//! 아이콘 **기하**와 그 **그릇**의 사본 셋을 대조한다 — 사이트 사본 둘과 앱 전사본 하나.
//!
//! 토큰에 [`site_vendor_tokens_track_the_app_export`] 가 있는 것과 같은 자리이고, 같은
//! 이유로 있다: 두 사본이 **둘 다 레포 안**이라 원격 접근 없이 판정된다.
//!
//! ## 사본이 셋이다
//!
//! 원격 킷은 글리프 기하를 `icons/<name>.svg` 에 한 파일씩 두고(그쪽이 SoT), 같은 것을
//! `icons.json` 매니페스트와 `components/core/Icon.jsx` 의 `ICON_PATHS` 레지스트리로
//! 내보낸다. 이 레포로 내려온 것은 그중 둘이다 — `site/vendor/icons/*.svg` 와
//! `site/vendor/components/core/Icon.jsx`. 셋째 `icons.json` 은 사본에 없다
//! (`site/vendor/README.md` 의 "원본에서 제외한 것" 참조).
//!
//! 앱은 네 번째 자리다. `crates/tasty-icons/src/lib.rs` 가 같은 기하를 **손으로 전사한**
//! 것이고(생성기가 없다 — `build.rs` 도 `bin` 도 없다), 그 모듈 주석이 출처로 `icons.json`
//! 을 적는다.
//!
//! ## ★ 사이트가 그리는 것은 `.svg` 파일이 **아니다**
//!
//! 변환기(`site/scripts/vendor-to-esm.mjs`)는 `icons/*.svg` 를 읽지 않는다. 사이트는
//! `Icon.jsx` 를 번들하고 그 안의 문자열 리터럴을 그린다. 그래서 `.svg` 만 고치면
//! **화면은 안 바뀌는데 고친 것처럼 보인다.** 이 타깃이 두 사본을 서로도 대조하는 이유다.
//!
//! ## 짝은 이름에서 **도출할 수 없다** — 명부여야 한다
//!
//! 두 가지가 도출을 막는다.
//!
//! - **이름이 겹치면서 어긋난다.** 사이트의 `list` 는 앱의 `log` 이고, 사이트의 `listView`
//!   가 앱의 `list` 다. camelCase↔snake_case 정규화로 짝지으면 `list` 가 서로 다른 글리프에
//!   붙어 **거짓 불일치**를 내고, 그 처방("사이트를 앱에 맞춰라")을 따르면 사이트가 깨진다.
//!   이름이 다른 짝은 넷이다 — `filter`/`funnel` · `list`/`log` · `listView`/`list` ·
//!   `scriptFile`/`script`.
//! - **기하가 같은데 다른 글리프가 있다.** `star` 와 `starFill` 은 `d` 가 한 글자도 다르지
//!   않고 **채움 여부만** 다르다. 그래서 기하로도 짝을 정할 수 없고, 채움도 함께 재야 한다.
//!
//! ## 왜 그림이 아니라 글자로 재는가
//!
//! 두 사본은 각자 그린 것이 아니라 **같은 문자열을 옮겨 적은 것**이다. 실측: 아래
//! 정규화(자기닫기 형태 통일 + 태그 사이 공백 제거) 뒤 65 짝이 **전부 글자까지 같다.**
//!
//! 오차 방향도 안전한 쪽이다. 글자가 같으면 그림도 반드시 같으므로 **놓치는 일은 없고**,
//! 표기만 달라진 경우(소수점 표기·경로 순서)에 **더 잡는 쪽으로** 틀린다. 그 오탐의 처방은
//! "받아오거나 명부에 적어라" 라서 무엇도 헐겁게 만들지 않는다.
//!
//! 래스터로 재는 선택지는 **이 크레이트에서 불가능하다.** `doc-guards.yml` 이 이 타깃을
//! 경로 필터 없이 매 push 돌릴 수 있는 이유가 이 크레이트의 **의존 0**(ADR-0647)이고,
//! 래스터 판정기는 usvg·resvg·tiny-skia 를 끌어와 그 성질을 깬다. 값이 아니라 **채널의
//! 존재 조건**이 판정 방식을 정한 자리다.
//!
//! ## 그릇도 본다 — 기하만 보면 65 짝이 다 같아도 화면이 다르다
//!
//! 여는 `<svg …>` 태그가 정하는 `viewBox` · `stroke-width` · `stroke-linecap` ·
//! `stroke-linejoin` 은 글리프 **전부**에 한 번에 걸린다. 그 한 글자가 어긋나면 65 짝의
//! inner 마크업이 한 글자도 안 다르면서 그려지는 그림은 전부 달라진다 — 기하 대조는
//! **그것을 못 본다.** 실측으로 확인했다: `Icon.jsx` 의 `viewBox` 를 `0 0 25 24` 로
//! 바꿔도 기하 대조 다섯은 전부 초록이었다.
//!
//! 색 축은 갈리는 것이 정상이라 사유와 함께 [`DIVERGENT_ENVELOPE`] 에 적는다(사이트는
//! `currentColor` 상속, 앱은 `white` 고정 + egui tint). `xmlns` 는 문서 사본 둘만 갖는다
//! ([`DOCUMENT_ONLY_ENVELOPE`]). 두 명부 다 **여유 0** 이다 — 갈림이 사라지면 그 항목을
//! 지워야 하고, 분류 안 된 속성이 한 사본에만 생겨도 실패한다.
//!
//! ## 이 타깃이 안 보는 것
//!
//! 기하와 그릇만 본다. 글리프가 **어디에 쓰이는지**(역할·그룹)는 안 본다. 그리고 두 사본이 나란히
//! 낡는 것도 못 잡는다 — 원격이 앞서간 것은 원격을 받아와야 드러나고, 그것은 판정기가
//! 아니라 `site/vendor/README.md` 의 "vendor 갱신 절차" 가 맡는다. 이 타깃의 초록은
//! **"사이트 사본과 앱 전사본이 서로 맞다"** 일 뿐 "최신" 이 아니다.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use tasty_doc_guards::floored_walk::{CountedOn, Descend, Floor, Walked, walk_with_floor};

/// (사이트 이름, 앱 이름). 65 짝 — 사이트 사본이 가진 글리프 전부.
///
/// 넷은 이름이 다르다(`filter`/`funnel` · `list`/`log` · `listView`/`list` ·
/// `scriptFile`/`script`). 나머지는 camelCase↔snake_case 지만, 그 규칙으로 **도출하지
/// 않는다** — 위 모듈 주석의 `list` 충돌 때문이다.
const PAIRS: &[(&str, &str)] = &[
    ("plus", "plus"),
    ("close", "close"),
    ("refresh", "refresh"),
    ("edit", "edit"),
    ("trash", "trash"),
    ("copy", "copy"),
    ("check", "check"),
    ("search", "search"),
    ("filter", "funnel"),
    ("swap", "swap"),
    ("more", "more"),
    ("download", "download"),
    ("star", "star"),
    ("starFill", "star_fill"),
    ("chevronRight", "chevron_right"),
    ("chevronDown", "chevron_down"),
    ("chevronUp", "chevron_up"),
    ("chevronLeft", "chevron_left"),
    ("chevronsLeft", "chevrons_left"),
    ("chevronsRight", "chevrons_right"),
    ("move", "move"),
    ("terminal", "terminal"),
    ("markdown", "markdown"),
    ("html", "html"),
    ("split", "split"),
    ("paneEmpty", "pane_empty"),
    ("splitH", "split_h"),
    ("folder", "folder"),
    ("folderOpen", "folder_open"),
    ("file", "file"),
    ("image", "image"),
    ("list", "log"),
    ("layoutGrid", "layout_grid"),
    ("layoutDetail", "layout_detail"),
    ("listView", "list"),
    ("layers", "layers"),
    ("columns", "columns"),
    ("clipboard", "clipboard"),
    ("textLeft", "text_left"),
    ("scriptFile", "script"),
    ("remote", "remote"),
    ("port", "port"),
    ("gitBranch", "git_branch"),
    ("gitTree", "git_tree"),
    ("eye", "eye"),
    ("eyeOff", "eye_off"),
    ("lock", "lock"),
    ("alertTriangle", "alert_triangle"),
    ("alertCircle", "alert_circle"),
    ("helpCircle", "help_circle"),
    ("shieldCheck", "shield_check"),
    ("bell", "bell"),
    ("tools", "tools"),
    ("settings", "settings"),
    ("plug", "plug"),
    ("rocket", "rocket"),
    ("command", "command"),
    ("theme", "theme"),
    ("keyboard", "keyboard"),
    ("mouse", "mouse"),
    ("sun", "sun"),
    ("hash", "hash"),
    ("cmdKey", "cmd_key"),
    ("optionKey", "option_key"),
    ("shiftKey", "shift_key"),
];

/// 앱에만 있고 사이트 사본에 없는 글리프. 여유 0 의 양방향 래칫이다 — 늘면 앱이 킷에 없는
/// 글리프를 새로 그린 것이고(킷에 올려야 한다), 줄면 받아온 것이니 그만큼 [`PAIRS`] 로
/// 옮긴다.
const APP_ONLY: &[&str] = &["arrow_down", "arrow_right", "fit", "minus", "redo", "undo"];

/// 채운 글리프 — 사이트 `FILL_GLYPHS` 와 앱 `fill_icon!` 이 같은 집합을 가리켜야 한다.
/// 사이트 이름으로 적는다.
const FILLED: &[&str] = &["starFill"];

const VENDOR_ICON_DIR: &str = "site/vendor/icons";
const VENDOR_REGISTRY: &str = "site/vendor/components/core/Icon.jsx";
const APP_ICONS: &str = "crates/tasty-icons/src/lib.rs";

/// 판정기가 실제로 뭔가를 셌는지 보는 바닥. 파서가 조용히 0 을 내놓으면 모든 집합이 비어
/// 등식이 전부 성립한다 — 그때 초록은 "맞다" 가 아니라 "안 봤다" 다.
///
/// 세 사본이 같은 좌변(글리프 수)을 재므로 **값은 하나만 둔다** — 순회 하한이 그 하나이고,
/// 파서 둘은 그 `min` 을 빌려 쓴다.
const PARSE_FLOOR: usize = VENDOR_ICON_FLOOR.min;

/// `site/vendor/icons/` 의 `.svg` 순회 하한.
///
/// 이 모수는 **킷의 글리프 수**다. 디자인이 글리프를 더하거나 빼야만 움직이고, 그 두 방향은
/// 이 파일의 [`PAIRS`]·[`APP_ONLY`] 가 이미 **정확히** 고정한다. 그래서 이 하한이 잡는 것은
/// 수의 변화가 아니라 **순회가 죽는 것**뿐이고(디렉토리 이름이 바뀌거나 확장자 필터가
/// 어긋나는 경우), 여유는 그 목적에 맞춰 넓게 둔다. 좁혀 봐야 명부가 먼저 울므로 두 번
/// 우는 자리만 생긴다.
const VENDOR_ICON_FLOOR: Floor = Floor {
    min: 60,
    measured: 65,
    measured_on: "2026-09-20",
    counted_on: CountedOn::Tree("2527d1920 — `site/vendor/icons/*.svg` 의 추적 계수"),
    why_this_gap: "글리프 수는 디자인 결정으로만 움직이고 한 회차에 한둘이다. 정확한 값은 \
                   `PAIRS`(65 짝)와 `APP_ONLY` 가 양방향으로 고정하므로, 이 하한은 그 \
                   등식이 아니라 순회 자체가 살아 있는지만 본다.",
};

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 를 못 읽었다: {e}", path.display()))
}

/// 렌더에 영향 없는 표기 차이를 지운다 — 자기닫기 형태(`></path>` ↔ `/>`)와 태그 사이
/// 공백. `d` 속성 **안**의 공백은 좌표를 가르므로 건드리지 않는다(한 칸으로 줄이기만 한다).
fn normalize(body: &str) -> String {
    let mut s = body.to_string();
    for tag in [
        "circle", "path", "rect", "line", "polyline", "polygon", "ellipse",
    ] {
        s = s.replace(&format!("></{tag}>"), "/>");
    }
    // 공백 런을 한 칸으로
    let mut out = String::with_capacity(s.len());
    let mut prev_ws = false;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_ws {
                out.push(' ');
            }
            prev_ws = true;
        } else {
            out.push(ch);
            prev_ws = false;
        }
    }
    // 태그 경계의 공백 제거
    let out = out.replace("> <", "><");
    out.trim().to_string()
}

/// `site/vendor/icons/<name>.svg` → 이름 → 파일 **전문**.
///
/// 기하(inner 마크업)를 보는 [`vendor_files`] 와 envelope(여는 태그)를 보는
/// [`vendor_svg_envelopes`] 가 같은 순회를 두 번 돌지 않게 여기서 한 번만 읽는다.
///
/// 순회는 공용 [`walk_with_floor`] 를 쓴다 — 직접 `read_dir` 를 부르면 하한을 빠뜨릴 수
/// 있고, 하한 없는 순회는 아무것도 못 모았을 때 조용히 빈 집합을 내놓는다.
fn vendor_svg_sources() -> BTreeMap<String, String> {
    let root = repo_root();
    let walked = walk_with_floor(
        &root.join(VENDOR_ICON_DIR),
        &root,
        &VENDOR_ICON_FLOOR,
        Descend::Everything,
        &|w: &Walked| w.rel.ends_with(".svg"),
    )
    .unwrap_or_else(|why| panic!("{VENDOR_ICON_DIR} 순회: {why}"));

    let mut map = BTreeMap::new();
    for w in walked {
        let path = w.path;
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("파일 이름")
            .to_string();
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{} 를 못 읽었다: {e}", path.display()));
        map.insert(name, text);
    }
    map
}

/// `site/vendor/icons/<name>.svg` → 이름 → inner 마크업.
fn vendor_files() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (name, text) in vendor_svg_sources() {
        let open_end = text
            .find('>')
            .unwrap_or_else(|| panic!("{name}.svg: `<svg …>` 를 못 찾았다"));
        let close = text
            .rfind("</svg>")
            .unwrap_or_else(|| panic!("{name}.svg: `</svg>` 를 못 찾았다"));
        map.insert(name, normalize(&text[open_end + 1..close]));
    }
    map
}

/// `Icon.jsx` 의 `ICON_PATHS` 레지스트리 → 이름 → inner 마크업.
///
/// 의존 0 이라 파서를 쓸 수 없다(ADR-0647). 블록을 잘라 줄 단위로 읽는다 — 항목이
/// `  name: '…',` 한 줄 형태인 데 기대고, 그 형태가 깨지면 [`PARSE_FLOOR`] 가 잡는다.
fn vendor_registry(src: &str) -> BTreeMap<String, String> {
    let start = src
        .find("export const ICON_PATHS = {")
        .expect("Icon.jsx: ICON_PATHS 선언을 못 찾았다");
    let rest = &src[start..];
    let end = rest
        .find("\n};")
        .expect("Icon.jsx: ICON_PATHS 의 끝을 못 찾았다");
    let mut map = BTreeMap::new();
    for line in rest[..end].lines().skip(1) {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        let Some((name, rhs)) = line.split_once(':') else {
            continue;
        };
        let Some(open) = rhs.find('\'') else { continue };
        let Some(close) = rhs.rfind('\'') else {
            continue;
        };
        if close <= open {
            continue;
        }
        map.insert(name.trim().to_string(), normalize(&rhs[open + 1..close]));
    }
    map
}

/// `Icon.jsx` 의 `FILL_GLYPHS` 집합.
fn vendor_filled(src: &str) -> BTreeSet<String> {
    let start = src
        .find("FILL_GLYPHS = new Set([")
        .expect("Icon.jsx: FILL_GLYPHS 선언을 못 찾았다");
    let rest = &src[start..];
    let end = rest
        .find("])")
        .expect("Icon.jsx: FILL_GLYPHS 의 끝을 못 찾았다");
    let mut set = BTreeSet::new();
    let mut chunk = &rest[..end];
    while let Some(open) = chunk.find('"') {
        let after = &chunk[open + 1..];
        let Some(close) = after.find('"') else { break };
        set.insert(after[..close].to_string());
        chunk = &after[close + 1..];
    }
    set
}

/// 앱 전사본 → 이름 → (inner 마크업, 채움 여부).
///
/// `stroke_icon!(NAME, "uri", r#"…"#)` / `fill_icon!(…)` 두 형태를 읽는다. 매크로 호출이
/// 여러 줄에 걸쳐 있을 수 있어 줄 단위로는 못 읽고, 여는 자리부터 손으로 훑는다.
fn app_icons(src: &str) -> BTreeMap<String, (String, bool)> {
    let mut map = BTreeMap::new();
    for (macro_name, filled) in [("stroke_icon!(", false), ("fill_icon!(", true)] {
        let mut from = 0usize;
        while let Some(hit) = src[from..].find(macro_name) {
            let at = from + hit + macro_name.len();
            from = at;
            // 선언(매크로 정의)은 `macro_rules!` 뒤에 오므로 인자 형태가 다르다 — uri 자리에
            // 문자열 리터럴이 없으면 건너뛴다.
            let Some(uri_open) = src[at..].find('"') else {
                continue;
            };
            let uri_start = at + uri_open + 1;
            let Some(uri_len) = src[uri_start..].find('"') else {
                continue;
            };
            let uri = &src[uri_start..uri_start + uri_len];
            let tail = &src[uri_start + uri_len..];
            let Some(body_open) = tail.find("r#\"") else {
                continue;
            };
            let body_start = body_open + 3;
            let Some(body_len) = tail[body_start..].find("\"#") else {
                continue;
            };
            let body = &tail[body_start..body_start + body_len];
            map.insert(uri.to_string(), (normalize(body), filled));
        }
    }
    map
}

/// 세 사본이 **같아야 하는** 여는 태그 속성 — (SVG 이름, JSX 이름).
///
/// 이름이 갈리는 이유는 JSX 가 DOM 프로퍼티 표기를 쓰기 때문이다(`stroke-width` ↔
/// `strokeWidth`). 그 짝을 **도출하지 않고 명부로 적는다** — [`PAIRS`] 와 같은 이유다.
///
/// 이 네 축이 글리프의 **그릇**을 정한다. `viewBox` 가 어긋나면 모든 글리프가 잘리거나
/// 어긋나 그려지고, `stroke-width` 가 어긋나면 전부 굵기가 달라진다. 즉 여기 한 글자가
/// 65 짝의 기하 전부를 무효로 만드는데, 기하 대조는 inner 마크업만 보므로 **그것을 못 본다.**
const SHARED_ENVELOPE: &[(&str, &str)] = &[
    ("viewBox", "viewBox"),
    ("stroke-width", "strokeWidth"),
    ("stroke-linecap", "strokeLinecap"),
    ("stroke-linejoin", "strokeLinejoin"),
];

/// 세 사본이 **달라도 되는** 축과 그 사유. **여유 0 의 양방향 명부다** — 여기 없는 축이
/// 갈리면 실패하고, 여기 있는 축이 세 사본에서 같아지면 (통일된 것이므로) 지워야 한다.
/// 두 **SVG 문서** 사본(`.svg` 파일 · 앱 매크로 접두)만 갖는 축과 그 사유.
///
/// JSX 는 문서가 아니라 인라인 엘리먼트라 이 축이 없는 것이 정상이다. 그래서 세 사본
/// 등식에는 안 넣고, 두 문서 사본끼리만 같은지 본다. **여기도 여유 0 이다** — JSX 에
/// 생기면 분류가 틀린 것이고, 두 문서 사본에서 갈리면 실패한다.
const DOCUMENT_ONLY_ENVELOPE: &[(&str, &str)] = &[(
    "xmlns",
    "독립 문서에만 필요한 네임스페이스 선언이다. `.svg` 파일은 파일로 열리고 앱 접두는 \
     `egui_extras` 로더와 `usvg` 가 문서로 읽는다. JSX 는 DOM 에 인라인되므로 브라우저가 \
     부모에서 상속한다",
)];

const DIVERGENT_ENVELOPE: &[(&str, &str)] = &[
    (
        "stroke",
        "선 색이다. 사이트 둘은 CSS 의 `currentColor` 를 상속받고, 앱은 `white` 로 고정해 \
         egui 가 tint 로 색을 입힌다. 색은 이 타깃의 축이 아니다",
    ),
    (
        "fill",
        "채운 글리프의 색이다 — 같은 이유로 갈린다. 채움 **여부**는 색과 별개이고 \
         `FILLED` 명부와 `FILL_GLYPHS` 가 이미 그 축을 잰다",
    ),
];

/// 여는 `<svg …>` 태그에서 속성을 모은다.
///
/// 값이 따옴표면 그 문자열을, JSX 의 `name={…}` 이면 [`JSX_EXPR`] 를 넣는다. 의존 0 이라
/// 파서를 못 쓰므로(ADR-0647) 손으로 훑는다 — 이 좌변은 속성 몇 개짜리 여는 태그 하나다.
fn open_tag_attrs(tag: &str) -> BTreeMap<String, String> {
    let b = tag.as_bytes();
    let mut out = BTreeMap::new();
    let mut i = 0usize;
    while i < b.len() {
        if !(b[i].is_ascii_alphabetic()) {
            i += 1;
            continue;
        }
        let start = i;
        while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'-' || b[i] == b':') {
            i += 1;
        }
        let name = &tag[start..i];
        let mut j = i;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= b.len() || b[j] != b'=' {
            continue;
        }
        j += 1;
        while j < b.len() && b[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < b.len() && b[j] == b'"' {
            let vs = j + 1;
            let Some(len) = tag[vs..].find('"') else {
                break;
            };
            out.insert(name.to_string(), tag[vs..vs + len].to_string());
            i = vs + len + 1;
        } else if j < b.len() && b[j] == b'{' {
            let mut depth = 0usize;
            let mut k = j;
            while k < b.len() {
                match b[k] {
                    b'{' => depth += 1,
                    b'}' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                k += 1;
            }
            out.insert(name.to_string(), JSX_EXPR.to_string());
            i = (k + 1).min(b.len());
        }
    }
    out
}

/// JSX 에서 값이 식인 속성의 표지. 값 자체는 이 타깃의 축이 아니다.
const JSX_EXPR: &str = "{식}";

/// `<svg` 로 시작하는 여는 태그를 통째로 잘라낸다. `>` 까지이고, 따옴표 안의 `>` 는 센다.
fn svg_open_tag(src: &str, what: &str) -> String {
    let at = src
        .find("<svg")
        .unwrap_or_else(|| panic!("{what}: `<svg` 를 못 찾았다"));
    let rest = &src[at..];
    let b = rest.as_bytes();
    let mut i = 0usize;
    let mut in_quote = false;
    while i < b.len() {
        match b[i] {
            b'"' => in_quote = !in_quote,
            b'>' if !in_quote => return rest[..i].to_string(),
            _ => {}
        }
        i += 1;
    }
    panic!("{what}: 여는 `<svg …>` 태그가 안 닫힌다");
}

/// 사이트 사본 하나 — `.svg` 파일 65 개의 여는 태그.
fn vendor_svg_envelopes() -> BTreeMap<String, BTreeMap<String, String>> {
    vendor_svg_sources()
        .into_iter()
        .map(|(name, text)| {
            let tag = svg_open_tag(&text, &format!("{name}.svg"));
            (name, open_tag_attrs(&tag))
        })
        .collect()
}

/// 사이트 사본 둘째 — `Icon.jsx` 의 컴포넌트가 실제로 그리는 `<svg …>`.
///
/// 사이트가 번들하는 것은 이쪽이다. `.svg` 파일만 고치면 화면이 안 바뀐다는 이 타깃의
/// 전제가 envelope 에도 그대로 적용된다.
fn registry_envelope(src: &str) -> BTreeMap<String, String> {
    let at = src
        .find("export function Icon(")
        .expect("Icon.jsx: `export function Icon(` 을 못 찾았다");
    open_tag_attrs(&svg_open_tag(&src[at..], "Icon.jsx 의 컴포넌트"))
}

/// 앱 전사본 — 두 매크로가 붙이는 접두 `<svg …>`. 키는 채움 여부다.
fn app_envelopes(src: &str) -> BTreeMap<bool, BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for (macro_def, filled) in [
        ("macro_rules! stroke_icon", false),
        ("macro_rules! fill_icon", true),
    ] {
        let at = src
            .find(macro_def)
            .unwrap_or_else(|| panic!("{APP_ICONS}: `{macro_def}` 을 못 찾았다"));
        let tag = svg_open_tag(&src[at..], macro_def);
        out.insert(filled, open_tag_attrs(&tag));
    }
    out
}

/// 파서가 살아 있는지 — 이것이 먼저 통과해야 아래 envelope 판정의 초록이 뜻을 갖는다.
#[test]
fn the_envelope_readers_see_all_three_copies() {
    let files = vendor_svg_envelopes();
    assert!(
        files.len() >= PARSE_FLOOR,
        "`.svg` 를 {} 개만 읽었다 (바닥 {PARSE_FLOOR})",
        files.len()
    );
    let registry = registry_envelope(&read(VENDOR_REGISTRY));
    let app = app_envelopes(&read(APP_ICONS));
    assert_eq!(app.len(), 2, "앱 매크로 접두를 둘 다 못 읽었다: {app:?}");

    // 비영 대조 — 세 사본 각각에서 **같은 축 하나**가 실제로 잡히는지 본다. 위 계수만으로는
    // 파서가 엉뚱한 것을 세고 있어도 통과한다.
    let (svg_axis, jsx_axis) = SHARED_ENVELOPE[0];
    for (what, attrs) in [
        (
            format!("{VENDOR_ICON_DIR} 의 첫 파일"),
            files.values().next().expect("파일 하나").clone(),
        ),
        (VENDOR_REGISTRY.to_string(), registry),
    ] {
        let axis = if what == VENDOR_REGISTRY {
            jsx_axis
        } else {
            svg_axis
        };
        assert!(
            attrs.contains_key(axis),
            "{what} 의 여는 태그에서 `{axis}` 를 못 읽었다 — 파서가 죽었다면 아래 등식은 \
             안 봐서 나온 초록이다: {attrs:?}"
        );
    }
    for (filled, attrs) in &app {
        assert!(
            attrs.contains_key(svg_axis),
            "앱 매크로(채움={filled})의 접두에서 `{svg_axis}` 를 못 읽었다: {attrs:?}"
        );
    }
}

/// 명부가 실재하는 축을 가리키는가 — 그리고 **죽은 항목이 없는가.**
///
/// `DIVERGENT_ENVELOPE` 에 실제로는 안 갈리는 축이 남으면 그 자리는 영구히 안 보이게 된다.
#[test]
fn the_divergence_roster_has_no_dead_weight() {
    let files = vendor_svg_envelopes();
    let app = app_envelopes(&read(APP_ICONS));
    let mut seen = BTreeSet::new();
    for (axis, why) in DIVERGENT_ENVELOPE {
        assert!(seen.insert(*axis), "`{axis}` 가 명부에 두 번 있다");
        assert!(!why.trim().is_empty(), "`{axis}` 에 사유가 없다");
        let mut values: BTreeSet<Option<&str>> = BTreeSet::new();
        for attrs in files.values().chain(app.values()) {
            values.insert(attrs.get(*axis).map(String::as_str));
        }
        assert!(
            values.len() > 1,
            "`{axis}` 는 순수 SVG 사본들에서 값이 하나뿐이다({values:?}) — 갈리지 않으므로 \
             이 명부가 아니라 `SHARED_ENVELOPE` 에 있어야 한다. 명부에 남겨 두면 그 축은 \
             영구히 안 보인다"
        );
    }
    for (svg_axis, _) in SHARED_ENVELOPE {
        assert!(
            !seen.contains(svg_axis),
            "`{svg_axis}` 가 두 명부에 다 있다 — 같아야 하면서 달라도 된다는 뜻이 된다"
        );
    }

    // 문서 전용 축 — 두 문서 사본에서 같고, JSX 에는 없어야 한다. 둘 중 하나라도 어긋나면
    // 분류가 틀린 것이다.
    let registry = registry_envelope(&read(VENDOR_REGISTRY));
    for (axis, why) in DOCUMENT_ONLY_ENVELOPE {
        assert!(!why.trim().is_empty(), "`{axis}` 에 사유가 없다");
        assert!(
            !seen.contains(axis),
            "`{axis}` 가 갈려도 되는 축이면서 문서 전용 축이다 — 하나만 골라라"
        );
        let values: BTreeSet<Option<&str>> = files
            .values()
            .chain(app.values())
            .map(|a| a.get(*axis).map(String::as_str))
            .collect();
        assert_eq!(
            values.len(),
            1,
            "`{axis}` 가 두 문서 사본에서 갈린다: {values:?}"
        );
        assert!(
            values.iter().all(Option::is_some),
            "`{axis}` 가 일부 문서 사본에 없다 — 문서 전용 축이 아니다"
        );
        assert!(
            !registry.contains_key(*axis),
            "`{axis}` 가 {VENDOR_REGISTRY} 의 인라인 엘리먼트에도 생겼다 — 문서 전용이 \
             아니게 됐으므로 `SHARED_ENVELOPE` 로 옮기고 JSX 이름을 짝지어라"
        );
    }
}

/// 순수 SVG 사본(`.svg` 파일 · 앱 매크로 접두)의 속성이 **전부 분류돼 있는가.**
///
/// 새 속성이 한 사본에만 생기면 여기서 걸린다 — 두 명부 중 어디로 갈지는 사람이 정한다.
#[test]
fn every_envelope_attribute_is_classified() {
    let classified: BTreeSet<&str> = SHARED_ENVELOPE
        .iter()
        .map(|(svg, _)| *svg)
        .chain(DOCUMENT_ONLY_ENVELOPE.iter().map(|(a, _)| *a))
        .chain(DIVERGENT_ENVELOPE.iter().map(|(a, _)| *a))
        .collect();

    let mut unclassified: BTreeSet<String> = BTreeSet::new();
    let mut missing: Vec<String> = Vec::new();
    let files = vendor_svg_envelopes();
    let app = app_envelopes(&read(APP_ICONS));

    for (what, attrs) in files
        .iter()
        .map(|(n, a)| (format!("{VENDOR_ICON_DIR}/{n}.svg"), a))
        .chain(
            app.iter()
                .map(|(f, a)| (format!("{APP_ICONS} 의 매크로(채움={f})"), a)),
        )
    {
        for name in attrs.keys() {
            if !classified.contains(name.as_str()) {
                unclassified.insert(format!("{name} ({what})"));
            }
        }
        for (svg_axis, _) in SHARED_ENVELOPE {
            if !attrs.contains_key(*svg_axis) {
                missing.push(format!("{svg_axis} 가 {what} 에 없다"));
            }
        }
    }

    assert!(
        unclassified.is_empty(),
        "여는 태그에 분류 안 된 속성이 있다: {unclassified:?}. 세 사본이 같아야 하면 \
         `SHARED_ENVELOPE` 에, 갈려도 되면 사유와 함께 `DIVERGENT_ENVELOPE` 에 넣어라"
    );
    assert!(
        missing.is_empty(),
        "공유 축이 빠진 자리가 있다 — 빠진 축은 브라우저·usvg 기본값으로 떨어져 \
         사본마다 다르게 그려진다: {missing:?}"
    );
}

/// 세 사본의 그릇이 같은가. **이 타깃의 기하 대조가 안 보는 축이다.**
#[test]
fn the_three_copies_share_one_render_envelope() {
    let files = vendor_svg_envelopes();
    let registry = registry_envelope(&read(VENDOR_REGISTRY));
    let app = app_envelopes(&read(APP_ICONS));

    let mut problems: Vec<String> = Vec::new();
    for (svg_axis, jsx_axis) in SHARED_ENVELOPE {
        // 좌변 하나당 (자리 이름, 값) 전수를 모아 값으로 묶는다.
        let mut by_value: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (name, attrs) in &files {
            if let Some(v) = attrs.get(*svg_axis) {
                by_value
                    .entry(v.clone())
                    .or_default()
                    .push(format!("{name}.svg"));
            }
        }
        for (filled, attrs) in &app {
            if let Some(v) = attrs.get(*svg_axis) {
                by_value
                    .entry(v.clone())
                    .or_default()
                    .push(format!("앱 매크로(채움={filled})"));
            }
        }
        if let Some(v) = registry.get(*jsx_axis) {
            by_value
                .entry(v.clone())
                .or_default()
                .push(VENDOR_REGISTRY.to_string());
        } else {
            problems.push(format!(
                "`{jsx_axis}` 가 {VENDOR_REGISTRY} 의 여는 태그에 없다 — 사이트가 그리는 것은 \
                 이쪽이라, 여기서 빠지면 `.svg` 가 맞아도 화면이 다르다"
            ));
        }

        if by_value.len() > 1 {
            let spread: Vec<String> = by_value
                .iter()
                .map(|(v, wheres)| {
                    let head: Vec<&str> = wheres.iter().take(3).map(String::as_str).collect();
                    let more = wheres.len().saturating_sub(head.len());
                    if more > 0 {
                        format!("{v:?} ← {} 외 {more} 자리", head.join(" · "))
                    } else {
                        format!("{v:?} ← {}", head.join(" · "))
                    }
                })
                .collect();
            problems.push(format!("[{svg_axis}] {}", spread.join(" / ")));
        }
    }

    assert!(
        problems.is_empty(),
        "세 사본의 **그릇**이 갈린다. 여기 한 글자가 글리프 65 짝 전부를 다르게 그리는데, \
         inner 마크업 대조는 그것을 못 본다. 어느 쪽이 맞는지는 이 타깃이 모른다 — 원격 킷이 \
         정본이므로 `site/vendor/README.md` 의 갱신 절차로 사본을 맞추고, 앱이 뒤처졌으면 \
         앱 전사본을 고친다:\n  {}",
        problems.join("\n  ")
    );
}

#[test]
fn the_judge_actually_parses_all_three_copies() {
    let files = vendor_files();
    let registry = vendor_registry(&read(VENDOR_REGISTRY));
    let app = app_icons(&read(APP_ICONS));
    for (what, n) in [
        ("site/vendor/icons/*.svg", files.len()),
        ("Icon.jsx ICON_PATHS", registry.len()),
        ("crates/tasty-icons", app.len()),
    ] {
        assert!(
            n >= PARSE_FLOOR,
            "{what} 에서 {n} 개만 읽었다 (바닥 {PARSE_FLOOR}) — 파서가 형태를 놓쳤다. \
             0 에 가까우면 아래 등식들이 전부 빈 집합끼리 비교돼 조용히 초록이 된다"
        );
    }
    assert!(
        !vendor_filled(&read(VENDOR_REGISTRY)).is_empty(),
        "FILL_GLYPHS 를 빈 집합으로 읽었다 — 채움 축이 안 재진다"
    );
}

#[test]
fn every_roster_entry_names_a_real_glyph() {
    let files = vendor_files();
    let registry = vendor_registry(&read(VENDOR_REGISTRY));
    let app = app_icons(&read(APP_ICONS));

    let mut seen_site = BTreeSet::new();
    let mut seen_app = BTreeSet::new();
    for (site, app_name) in PAIRS {
        assert!(
            seen_site.insert(*site),
            "명부에 사이트 이름 {site} 가 두 번 있다"
        );
        assert!(
            seen_app.insert(*app_name),
            "명부에 앱 이름 {app_name} 가 두 번 있다"
        );
        assert!(
            files.contains_key(*site),
            "명부의 {site} 에 대응하는 {VENDOR_ICON_DIR}/{site}.svg 가 없다"
        );
        assert!(
            registry.contains_key(*site),
            "명부의 {site} 가 Icon.jsx ICON_PATHS 에 없다"
        );
        assert!(
            app.contains_key(*app_name),
            "명부의 앱 이름 {app_name} 가 {APP_ICONS} 에 없다"
        );
    }
    for name in APP_ONLY {
        assert!(
            app.contains_key(*name),
            "APP_ONLY 의 {name} 가 {APP_ICONS} 에 없다"
        );
        assert!(
            !seen_app.contains(name),
            "{name} 가 APP_ONLY 와 PAIRS 양쪽에 있다"
        );
    }
    for name in FILLED {
        assert!(
            seen_site.contains(name),
            "FILLED 의 {name} 가 PAIRS 에 없다"
        );
    }
}

#[test]
fn the_two_vendor_copies_carry_the_same_geometry() {
    let files = vendor_files();
    let registry = vendor_registry(&read(VENDOR_REGISTRY));

    let only_files: Vec<_> = files
        .keys()
        .filter(|k| !registry.contains_key(*k))
        .collect();
    let only_registry: Vec<_> = registry
        .keys()
        .filter(|k| !files.contains_key(*k))
        .collect();
    assert!(
        only_files.is_empty() && only_registry.is_empty(),
        "사이트 사본 둘의 글리프 **이름**이 어긋난다 — 파일에만: {only_files:?} · \
         ICON_PATHS 에만: {only_registry:?}"
    );

    let mismatched: Vec<_> = files
        .iter()
        .filter(|(name, body)| registry.get(*name) != Some(*body))
        .map(|(name, body)| {
            format!(
                "\n  {name}\n    icons/{name}.svg  {body}\n    ICON_PATHS        {}",
                registry.get(name).map(String::as_str).unwrap_or("<없음>")
            )
        })
        .collect();
    assert!(
        mismatched.is_empty(),
        "사이트 사본 둘의 **기하**가 어긋난다. 사이트가 그리는 것은 ICON_PATHS 쪽이므로, \
         `.svg` 만 고친 상태면 화면은 안 바뀐 채 고친 것처럼 보인다:{}",
        mismatched.join("")
    );
}

#[test]
fn the_app_transcription_matches_the_vendored_kit() {
    let registry = vendor_registry(&read(VENDOR_REGISTRY));
    let app = app_icons(&read(APP_ICONS));

    let mismatched: Vec<_> = PAIRS
        .iter()
        .filter_map(|(site, app_name)| {
            let want = registry.get(*site)?;
            let (got, _) = app.get(*app_name)?;
            (want != got).then(|| format!("\n  {site} ↔ {app_name}\n    킷  {want}\n    앱  {got}"))
        })
        .collect();
    assert!(
        mismatched.is_empty(),
        "앱 전사본과 사이트 사본의 기하가 어긋난다. 어느 쪽이 맞는지는 이 타깃이 모른다 — \
         원격 킷이 정본이고, 앱만 받았으면 `site/vendor/README.md` 의 갱신 절차로 사본을 \
         따라오게 하고, 앱이 안 받았으면 앱을 고친다:{}",
        mismatched.join("")
    );

    let site_filled = vendor_filled(&read(VENDOR_REGISTRY));
    let want_filled: BTreeSet<String> = FILLED.iter().map(|s| (*s).to_string()).collect();
    assert_eq!(
        site_filled, want_filled,
        "사이트의 FILL_GLYPHS 가 명부와 다르다"
    );
    let app_filled: BTreeSet<String> = PAIRS
        .iter()
        .filter(|(_, a)| app.get(*a).map(|(_, f)| *f).unwrap_or(false))
        .map(|(s, _)| (*s).to_string())
        .collect();
    assert_eq!(
        app_filled, want_filled,
        "앱의 `fill_icon!` 집합이 사이트의 FILL_GLYPHS 와 다르다 — `star`/`starFill` 처럼 \
         **기하가 같고 채움만 다른** 짝이 있어, 이 축이 어긋나면 같은 그림이 다르게 그려진다"
    );
}

#[test]
fn the_app_has_exactly_the_recorded_extra_glyphs() {
    let app = app_icons(&read(APP_ICONS));
    let paired: BTreeSet<&str> = PAIRS.iter().map(|(_, a)| *a).collect();
    let extra: BTreeSet<&str> = app
        .keys()
        .map(String::as_str)
        .filter(|k| !paired.contains(k))
        .collect();
    let recorded: BTreeSet<&str> = APP_ONLY.iter().copied().collect();
    assert_eq!(
        extra, recorded,
        "앱에만 있는 글리프 집합이 기록과 다르다. 늘었으면 킷에 없는 글리프를 앱이 새로 \
         그린 것이고(킷에 올려야 한다), 줄었으면 사본이 그만큼 따라온 것이니 PAIRS 로 옮겨라"
    );
}
