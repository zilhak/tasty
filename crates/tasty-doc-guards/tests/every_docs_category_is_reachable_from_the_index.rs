//! 문서가 있는 docs/ 하위 분류마다 index.md가 있고 docs/index.md에 해당 분류 경로가 나오는지 확인한다.
//! 시작 전 안내에서 작업 영역을 찾을 수 있게 하기 위한 검사다.
//! 분류 안의 모든 문서가 색인에 있는지와 실제로 읽었는지는 확인하지 않는다.
//! 공개 사이트는 별도 색인 규칙을 사용하므로 검사 대상이 아니다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

const ROOT_INDEX: &str = "docs/index.md";

/// 2026-09-07 실측 9분류를 기준으로 둔 수집 하한.
const MIN_CATEGORIES: usize = 6;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// `docs/` 아래 파일 순회 하한.
const DOCS_FLOOR: Floor = Floor {
    min: 162,
    measured: tasty_doc_guards::floored_walk::populations::DOCS_MD.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::DOCS_MD.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::DOCS_MD.counted_on,
    why_this_gap: "실측 214개에서 가장 큰 비-ADR 분류인 docs/features의 52개만큼 여유를 둔다. \
                   한 분류를 통합하는 작업은 허용하되 더 큰 수집 누락은 실패시킨다. \
                   ADR 구성 방식이나 검사 범위를 바꾸면 모수와 하한을 함께 다시 검토한다.",
};

/// 문서가 없는 빈 디렉터리는 대상이 아니므로 수집된 파일에서 분류를 구한다.
fn categories(root: &Path) -> BTreeSet<String> {
    categories_of(&docs_files(root, root, &DOCS_FLOOR))
}

/// 작은 합성 트리에도 같은 순회를 사용하도록 경로·하한을 인자로 받는다.
fn docs_files(root: &Path, rel_base: &Path, floor: &Floor) -> Vec<Walked> {
    walk_with_floor(
        root,
        rel_base,
        floor,
        Descend::SkipBuildCaches,
        &|w: &Walked| w.rel.starts_with("docs/") && w.rel.ends_with(".md"),
    )
    .unwrap_or_else(|e| panic!("`docs/` 순회가 실패했다 — {e}"))
}

fn categories_of(files: &[Walked]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for w in files {
        // 호출자가 넘긴 경로도 docs/ 내부인지 확인한다.
        let Some(rest) = w.rel.strip_prefix("docs/") else {
            continue;
        };
        if let Some((head, tail)) = rest.split_once('/')
            && !tail.is_empty()
        {
            out.insert(head.to_string());
        }
    }
    out
}

/// 상대 Markdown 링크 접두어 또는 저장소 기준 경로가 있는지 본다. 완전한 링크 파서는 아니다.
fn index_links_to(index: &str, cat: &str) -> bool {
    index.contains(&format!("]({cat}/")) || index.contains(&format!("docs/{cat}/"))
}

fn unlinked<'a>(cats: &'a BTreeSet<String>, index: &str) -> Vec<&'a String> {
    cats.iter().filter(|c| !index_links_to(index, c)).collect()
}

fn without_own_index<'a>(cats: &'a BTreeSet<String>, docs_dir: &Path) -> Vec<&'a String> {
    cats.iter()
        .filter(|c| !docs_dir.join(c).join("index.md").is_file())
        .collect()
}

#[test]
fn every_category_has_its_own_index() {
    let root = repo_root();
    let cats = categories(&root);
    assert!(
        cats.len() >= MIN_CATEGORIES,
        "docs/에서 분류를 {}개만 찾았다(2026-09-07 실측 9). 수집 범위와 실제 문서 삭제를 확인한다.",
        cats.len()
    );
    let missing = without_own_index(&cats, &root.join("docs"));
    assert!(
        missing.is_empty(),
        "분류 색인이 없다: {missing:?}. 해당 영역의 문서를 찾을 수 있도록 index.md를 추가한다."
    );
}

#[test]
fn every_category_is_named_in_the_root_index() {
    let root = repo_root();
    let cats = categories(&root);
    let index = std::fs::read_to_string(root.join(ROOT_INDEX))
        .unwrap_or_else(|e| panic!("{ROOT_INDEX} 를 읽지 못했다 — {e}"));
    assert!(
        cats.len() >= MIN_CATEGORIES,
        "카테고리를 {} 개만 찾았다 — 걷기가 깨졌다",
        cats.len()
    );
    let missing = unlinked(&cats, &index);
    assert!(
        missing.is_empty(),
        "{ROOT_INDEX}에서 다음 분류의 경로를 찾지 못했다: {missing:?}. 수집 대상을 제외하지 말고 색인에 진입 경로를 추가한다."
    );
}

#[test]
fn the_predicate_counts_links_not_mentions() {
    let cat = "dev-guide";
    let linked_rel = format!("| 개발 | [가이드]({cat}/index.md) |");
    let linked_abs = format!("자세한 것은 `docs/{cat}/build.md` 를 보라");
    let mentioned = format!("{cat} 은 개발 가이드다");
    assert!(index_links_to(&linked_rel, cat), "상대 링크를 못 센다");
    assert!(index_links_to(&linked_abs, cat), "레포 경로 형태를 못 센다");
    assert!(!index_links_to(&mentioned, cat), "산문 언급을 링크로 셌다");
}

/// 경로 분류만 검사하므로 디스크에 파일을 만들지 않고 Walked 값을 구성한다.
fn roster<S: AsRef<str>>(rels: &[S]) -> Vec<Walked> {
    rels.iter()
        .map(|r| Walked {
            path: PathBuf::from(r.as_ref()),
            rel: r.as_ref().to_string(),
        })
        .collect()
}

#[test]
fn the_derivation_takes_the_first_segment_and_drops_the_rootless_forms() {
    let cats = categories_of(&roster(&[
        "docs/zone-alpha/index.md",
        "docs/zone-alpha/deep/deeper/note.md",
        "docs/zone-beta/one.md",
        "docs/index.md",
        "docs/zone-ghost/",
        "zone/outside/x.md",
    ]));

    let got: Vec<&str> = cats.iter().map(String::as_str).collect();
    assert_eq!(
        got,
        vec!["zone-alpha", "zone-beta"],
        "첫 마디만, 그리고 뒤가 있는 것만 카테고리로 세야 한다"
    );
}

#[test]
fn the_two_reports_name_the_offender_and_leave_the_rest_alone() {
    // 정상 대조군에는 실제 색인이 있는 분류를 사용해, 누락만 보고하는지 확인한다.
    let control = "dev-guide";
    let cats = categories_of(&roster(&[
        &format!("docs/{control}/build.md"),
        "docs/zone-unlinked/note.md",
        "docs/zone-mentioned/note.md",
    ]));

    let index = format!(
        "| 영역 | 진입점 |\n|---|---|\n| 대조군 | [진입]({control}/index.md) |\n\
         zone-mentioned 도 문서가 있다.\n"
    );

    let missing_link = unlinked(&cats, &index);
    assert_eq!(
        missing_link.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        vec!["zone-mentioned", "zone-unlinked"],
        "산문 언급은 링크가 아니고, 아예 없는 것도 링크가 아니다. 링크된 것은 안 섞인다"
    );

    let missing_own = without_own_index(&cats, &repo_root().join("docs"));
    assert_eq!(
        missing_own.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
        vec!["zone-mentioned", "zone-unlinked"],
        "자기 `index.md` 가 없는 것만 나와야 한다 — 대조군은 안 섞인다"
    );
}
