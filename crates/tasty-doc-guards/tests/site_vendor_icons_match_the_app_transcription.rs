//! 아이콘 **기하**의 사본 셋을 대조한다 — 사이트 사본 둘과 앱 전사본 하나.
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
//! 경로 필터 없이 매 push 돌릴 수 있는 이유가 이 크레이트의 **의존 0**(ADR-0138)이고,
//! 래스터 판정기는 usvg·resvg·tiny-skia 를 끌어와 그 성질을 깬다. 값이 아니라 **채널의
//! 존재 조건**이 판정 방식을 정한 자리다.
//!
//! ## 이 타깃이 안 보는 것
//!
//! 기하만 본다. 글리프가 **어디에 쓰이는지**(역할·그룹)는 안 본다. 그리고 두 사본이 나란히
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

/// `site/vendor/icons/<name>.svg` → 이름 → inner 마크업.
///
/// 순회는 공용 [`walk_with_floor`] 를 쓴다 — 직접 `read_dir` 를 부르면 하한을 빠뜨릴 수
/// 있고, 하한 없는 순회는 아무것도 못 모았을 때 조용히 빈 집합을 내놓는다.
fn vendor_files() -> BTreeMap<String, String> {
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
/// 의존 0 이라 파서를 쓸 수 없다(ADR-0138). 블록을 잘라 줄 단위로 읽는다 — 항목이
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
