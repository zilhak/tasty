//! 줄 번호로 코드를 가리키는 인용의 증가를 막는다(docs/documentation-model.md).
//! 인용이 가리키는 코드가 맞는지는 확인하지 않는다. 새 인용에는 경로와 심볼 이름을 쓴다.
//!
//! ADR의 Context·Decision 절, Markdown 코드펜스와 진단 출력은 세지 않는다.
//! 과거 결정의 근거와 출력 예제는 현재 코드 위치로 바꾸면 뜻이 달라질 수 있다.
//! 파일 머리의 범위(:1-N)와 검사 형식을 설명하는 이 파일도 제외한다.
//!
//! 인용 정리를 위해 BAND만큼의 감소는 허용하고, 더 줄면 CAP을 낮추도록 요구한다.
//! BAND는 문서 한 편을 정리할 수 있도록 2026-09-07 파일별 최대 인용 수 15에서 정했다.
//! 여유를 무제한으로 남기면 이후 새 인용이 늘어도 통과할 수 있다.

use std::path::{Path, PathBuf};
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, normalized_rel, walk_with_floor};

/// 2026-09-24 ADR 통폐합 후 측정값. 늘거나 BAND 넘게 줄면 실패한다.
const CAP: usize = 21;

const BAND: usize = 15;

/// 디렉터리별 하한으로 한 디렉터리의 순회 실패를 다른 결과가 가리지 않게 한다.
const WHY_GAP: &str = "파일 분리·통폐합에 따른 감소를 허용하려고 실측보다 낮은 하한을 둔다. 이 값은 순회 실패를 찾는 용도이며 수집의 완전성을 보장하지 않는다.";

/// `(뿌리, 하한, 실측)` — 실측은 2026-09-07 값이고 하한은 그 1/3 언저리다.
const ROOTS: &[(&str, usize, usize)] = &[
    ("docs", 120, 386),
    ("src", 200, 598),
    ("crates", 200, 650),
    ("tests", 20, 59),
    ("scripts", 8, 23),
];

fn floor_for(min: usize, measured: usize) -> Floor {
    Floor {
        min,
        measured,
        measured_on: "2026-09-07",
        counted_on: tasty_doc_guards::floored_walk::CountedOn::SyntheticTree,
        why_this_gap: WHY_GAP,
    }
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/<name> 아래이므로 조상 둘이 레포 루트다")
}

fn is_self(rel: &str) -> bool {
    rel.ends_with("tests/line_number_citations_do_not_grow.rs")
}

/// `<경로 또는 파일명>.rs:<숫자>[-<숫자>]`를 모은다.
fn citations(line: &str) -> Vec<(usize, bool)> {
    let bytes: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 <= bytes.len() {
        if bytes[i] == '.' && bytes[i + 1..].starts_with(&['r', 's', ':']) {
            let named = i > 0 && (bytes[i - 1].is_alphanumeric() || bytes[i - 1] == '_');
            let mut j = i + 4;
            let mut start = 0usize;
            let mut digits = 0;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                start = start * 10 + bytes[j].to_digit(10).expect("ascii digit") as usize;
                digits += 1;
                j += 1;
            }
            if named && digits > 0 {
                let ranged = j < bytes.len()
                    && bytes[j] == '-'
                    && bytes.get(j + 1).is_some_and(char::is_ascii_digit);
                out.push((start, ranged));
            }
            i = j.max(i + 4);
            continue;
        }
        i += 1;
    }
    out
}

/// 한 파일에서 세야 할 인용 수. 마크다운이면 절과 코드펜스를 보고, 소스면 주석 줄만 본다.
fn count_in(rel: &str, text: &str) -> usize {
    let markdown = rel.ends_with(".md");
    let adr = rel.starts_with("docs/adr/")
        && rel
            .rsplit('/')
            .next()
            .is_some_and(|f| f.len() > 4 && f[..4].chars().all(|c| c.is_ascii_digit()));
    let mut section = String::new();
    let mut fenced = false;
    let mut n = 0;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if markdown {
            if trimmed.starts_with("```") {
                fenced = !fenced;
                continue;
            }
            if let Some(h) = line.strip_prefix("## ") {
                section = h.trim().to_string();
            }
            if fenced || line.contains("-->") {
                continue;
            }
        } else if !(trimmed.starts_with("//") || trimmed.starts_with('#')) {
            continue;
        }
        if markdown && adr && (section == "Context" || section == "Decision") {
            continue;
        }
        n += citations(line)
            .into_iter()
            .filter(|&(start, ranged)| !(ranged && start == 1))
            .count();
    }
    n
}

fn walk(dir: &str, floor: &Floor, keep: &dyn Fn(&Walked) -> bool) -> Vec<Walked> {
    let root = root();
    walk_with_floor(&root.join(dir), root, floor, Descend::SkipBuildCaches, keep)
        .unwrap_or_else(|why| panic!("{why}"))
}

fn population() -> Vec<(String, PathBuf)> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for &(dir, min, measured) in ROOTS {
        let floor = floor_for(min, measured);
        let want_md = dir == "docs";
        files.extend(
            walk(dir, &floor, &|w: &Walked| {
                let ext_ok = if want_md {
                    w.rel.ends_with(".md")
                } else {
                    w.rel.ends_with(".rs") || w.rel.ends_with(".sh")
                };
                ext_ok && !is_self(&w.rel)
            })
            .into_iter()
            .map(|w| (w.rel, w.path)),
        );
    }

    for name in ["README.md", "README.ko.md", "CLAUDE.md", "CHANGELOG.md"] {
        let p = root().join(name);
        if p.is_file() {
            files.push((normalized_rel(&p, root()), p));
        }
    }
    files
}

#[test]
fn line_number_citations_do_not_grow() {
    let mut total = 0usize;
    let mut per_file: Vec<(String, usize)> = Vec::new();
    for (rel, path) in population() {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let n = count_in(&rel, &text);
        if n > 0 {
            total += n;
            per_file.push((rel, n));
        }
    }
    per_file.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let listing: String = per_file
        .iter()
        .map(|(r, n)| format!("\n    {n:3}  {r}"))
        .collect();

    assert!(
        total <= CAP,
        "줄 번호 인용이 늘었다: {total}건, 상한 {CAP}. 새 인용은 경로와 함수·타입 이름으로 적는다(docs/documentation-model.md).\n현재 인용:{listing}"
    );
    assert!(
        total + BAND >= CAP,
        "줄 번호 인용이 상한보다 {}건 적다({total}, 상한 {CAP}, 허용 차이 {BAND}). 정리된 결과를 확인하고 CAP을 {total}으로 낮춘다.\n현재 인용:{listing}",
        CAP - total
    );
}
