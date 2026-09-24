//! 휠 Line 환산 파일에서 egui의 line_scroll_speed를 읽는 표지를 확인한다(ADR-0015).
//! 기본값 상수를 직접 쓰면 사용자가 바꾼 설정을 반영하지 못한다.
//! 실제 컨텍스트 값을 읽는 동작은 src/plugin_bridge/wire_scroll.rs의 one_notch_per_context가 검사한다.
//! 여기서는 새 경로도 등록 규칙을 따르는지 파일별 소스 표지로 확인한다.
// 테스트의 값 무시를 출하 코드의 lint 목록에서 제외한다.
#![allow(clippy::let_underscore_must_use)]

use tasty_doc_guards::cfg_predicate as cfg_span;

use std::path::{Path, PathBuf};
use tasty_doc_guards::temp_scratch::Scratch;

/// 단위를 그대로 전달하는 팝업·배너에는 Line 이름이 없을 수 있어 환산 함수 이름도 찾는다.
const LINE_UNIT_MARKS: [&str; 3] = [
    "MouseWheelUnit::Line",
    "MouseScrollDelta::LineDelta",
    "wheel_delta_to_points(",
];

/// 런타임 단일 출처를 읽는 형태.
const RUNTIME_SOURCE_MARKS: [&str; 2] = ["line_scroll(", "line_scroll_speed"];

/// 런타임 값이 아니라 **기본값 상수**를 쓰는 형태 — 설정을 바꿔도 안 따라온다.
const FROZEN_DEFAULT: &str = "DEFAULT_WHEEL_LINE_SCROLL";

/// Line 단위를 다루지만 환산하지 않고 전달하는 파일과 예외 근거.
const ALLOWLIST: &[(&str, &str)] = &[(
    "src/view/main/debug_input.rs",
    "debug 입력기는 요청 단위를 to_egui/to_winit_delta로 그대로 전달한다. 여기서 환산하면 입력 단위 재현이 달라진다. 같은 파일의 단위 보존 시험으로 확인한다.",
)];

/// 기존 환산 파일5개를 측정한 뒤 정상적인 파일 정리1개를 허용한 하한4다.
const MIN_CONVERSION_SITES: usize = 4;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

fn gather_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            gather_rs(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

struct Site {
    rel: String,
    reads_runtime_source: bool,
    frozen_default_lines: Vec<usize>,
}

/// 본체 src만 검사한다. 테스트에서는 환산값을 직접 지정할 수 있어 제외한다.
fn conversion_sites() -> Vec<Site> {
    conversion_sites_under(&repo_root())
}

/// 작은 합성 트리에도 같은 수집·판정을 적용하도록 루트를 인자로 받는다.
fn conversion_sites_under(root: &Path) -> Vec<Site> {
    let root = root.to_path_buf();
    let mut files = Vec::new();
    gather_rs(&root.join("src"), &mut files);
    files.sort();

    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else {
            continue;
        };
        let rel = f
            .strip_prefix(&root)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        if ALLOWLIST.iter().any(|(p, _)| *p == rel) {
            continue;
        }
        let lines: Vec<&str> = text.lines().collect();
        let gated = cfg_span::cfg_gated_lines(&lines, "test");
        // 주석으로 시작하는 줄은 표지 존재 여부에서 제외한다. 리터럴 전체를 마스킹하지는 않는다.
        let is_code = |i: usize| !gated[i] && !lines[i].trim_start().starts_with("//");
        let live = |needle: &str| {
            lines
                .iter()
                .enumerate()
                .any(|(i, l)| is_code(i) && l.contains(needle))
        };
        if !LINE_UNIT_MARKS.iter().any(|m| live(m)) {
            continue;
        }
        out.push(Site {
            reads_runtime_source: RUNTIME_SOURCE_MARKS.iter().any(|m| live(m)),
            frozen_default_lines: lines
                .iter()
                .enumerate()
                .filter(|(i, l)| is_code(*i) && l.contains(FROZEN_DEFAULT))
                .map(|(i, _)| i + 1)
                .collect(),
            rel,
        });
    }
    out
}

#[test]
fn the_population_of_conversion_sites_is_not_empty() {
    let n = conversion_sites().len();
    assert!(
        n >= MIN_CONVERSION_SITES,
        "휠 Line 처리 파일을 {n}개만 찾았다(하한 {MIN_CONVERSION_SITES}). 기존 wire_scroll/mouse/modifier_hint_overlay/popup_render/banner_render와 실제 수집 목록을 대조한다. 파일 이동·삭제와 표지 변경을 구별하고 하한 변경에는 근거를 남긴다."
    );
}

#[test]
fn every_conversion_site_reads_the_runtime_option() {
    let bad: Vec<String> = conversion_sites()
        .into_iter()
        .filter(|s| !s.reads_runtime_source)
        .map(|s| format!("  {}", s.rel))
        .collect();
    assert!(
        bad.is_empty(),
        "휠 Line 처리 파일에서 런타임 line_scroll_speed를 읽는 표지를 찾지 못했다. 현재 egui 설정으로 환산해야 한다(ADR-0015). 단위만 전달하는 파일은 근거와 함께 ALLOWLIST에 등록한다:\n{}",
        bad.join("\n")
    );
}

#[test]
fn no_conversion_site_freezes_the_notch_at_its_default() {
    let bad: Vec<String> = conversion_sites()
        .into_iter()
        .filter(|s| !s.frozen_default_lines.is_empty())
        .map(|s| format!("  {}:{:?}", s.rel, s.frozen_default_lines))
        .collect();
    assert!(
        bad.is_empty(),
        "휠 환산에서 기본값 {FROZEN_DEFAULT}를 직접 읽는다. 사용자가 변경한 현재 값은 egui 컨텍스트에서 읽는다(ADR-0015):\n{}",
        bad.join("\n")
    );
}

/// 합성 트리로 표지별 검출과 대상 밖 파일의 제외를 확인한다.
#[test]
fn a_stale_mark_or_a_dead_root_shrinks_the_population() {
    let probe = Scratch::new("wheel-notch");
    let dir = probe.path();
    let src = dir.join("src");
    std::fs::create_dir_all(src.join("inner")).expect("합성 트리를 만들지 못했다");

    let bodies: [(&str, String); 3] = [
        (
            "alpha.rs",
            format!(
                "fn a(u: X) {{ match u {{ {} => {}(1.0), _ => {{}} }} }}\n",
                LINE_UNIT_MARKS[0], RUNTIME_SOURCE_MARKS[0]
            ),
        ),
        (
            "beta.rs",
            format!(
                "fn b(d: X) {{ if let {}(x, y) = d {{ let _ = ({}, x, y); }} }}\n",
                LINE_UNIT_MARKS[1], RUNTIME_SOURCE_MARKS[1]
            ),
        ),
        (
            "inner/gamma.rs",
            format!(
                "fn c() {{ let _ = {}1.0); let _ = {}0.0); }}\n",
                LINE_UNIT_MARKS[2], RUNTIME_SOURCE_MARKS[0]
            ),
        ),
    ];
    for (name, body) in &bodies {
        std::fs::write(src.join(name), body).expect("합성 소스를 쓰지 못했다");
    }
    std::fs::write(src.join("delta.rs"), "fn d() {}\n").expect("합성 소스를 쓰지 못했다");
    // 확장자 필터가 넓어지는 오류는 수집 하한만으로 찾지 못한다.
    std::fs::write(
        src.join("zeta.md"),
        format!("{} {}\n", LINE_UNIT_MARKS[0], RUNTIME_SOURCE_MARKS[0]),
    )
    .expect("합성 문서를 쓰지 못했다");
    std::fs::write(
        src.join("epsilon.rs"),
        format!(
            "// {} 를 여기서는 안 쓴다\nfn e() {{}}\n",
            LINE_UNIT_MARKS[0]
        ),
    )
    .expect("합성 소스를 쓰지 못했다");

    let sites = conversion_sites_under(dir);
    let mut got: Vec<&str> = sites.iter().map(|s| s.rel.as_str()).collect();
    got.sort();
    assert_eq!(
        got,
        vec!["src/alpha.rs", "src/beta.rs", "src/inner/gamma.rs"],
        "합성 트리의 환산 파일 목록이 다르다. 표지별 검출과 대상 파일의 범위를 확인한다."
    );
    assert!(
        sites.iter().all(|s| s.reads_runtime_source),
        "합성 코드의 런타임 설정 표지를 찾지 못했다"
    );

    assert!(
        conversion_sites_under(&dir.join("does-not-exist")).is_empty(),
        "없는 루트에서 환산 파일을 수집했다. 읽기 실패 때 빈 목록을 반환해야 한다."
    );
}
