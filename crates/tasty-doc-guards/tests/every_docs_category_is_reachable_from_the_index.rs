//! `docs/` 의 카테고리를 **절차대로 따라가서 찾을 수 있는가.**
//!
//! ## 왜 이 축인가
//!
//! `CLAUDE.md` 의 "시작 전 (필수)" 는 세 걸음이다 — `docs/identity.md` 를 읽고,
//! `docs/concepts/ubiquitous-language.md` 를 읽고, **`docs/index.md` 에서 해당 작업 영역의
//! 가이드 문서를 확인**한다. 앞 둘은 "읽었는가" 라서 판정할 수 없다. 셋째는 다르다:
//! **그 절차를 정확히 따라도 못 찾는 카테고리가 있는가**는 판정된다.
//!
//! 새 카테고리 디렉토리가 색인 없이 생기면 셋째 걸음이 **조용히 실패한다.** 절차를
//! 지킨 사람이 그 문서를 못 찾고, 못 찾은 사람은 자기가 절차를 어겼다고 생각하지 않는다 —
//! 색인에 없으니 없는 줄 안다.
//!
//! 두 걸음을 다 본다. 카테고리는 자기 색인(`docs/<cat>/index.md`)으로 들어가고,
//! `docs/index.md` 는 그 색인들의 진입점이다. 어느 한쪽이 비면 경로가 끊긴다.
//!
//! ## 이 가드가 **안 묻는 것**
//!
//! - **읽었는가.** 절차의 앞 두 걸음이고, 집행 가능한 형태가 아니다.
//! - **색인이 그 카테고리의 문서를 다 담는가.** 카테고리 안쪽 완전성은 다른 물음이고
//!   카테고리마다 규칙이 다르다(ADR 은 `adr_index_parity` 가 행 단위로 본다).
//!   여기서 묻는 것은 **경로가 이어져 있는가** 하나다.
//! - **`docs/` 밖.** `site/content/` 는 독자가 다르고 색인 규칙도 다르다.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, walk_with_floor};

const ROOT_INDEX: &str = "docs/index.md";

/// 카테고리 하한. 2026-09-07 실측 9.
///
/// 걷기가 죽으면 아래 대조가 "빠진 것 없음" 으로 공짜 통과한다.
const MIN_CATEGORIES: usize = 6;

fn repo_root() -> PathBuf {
    tasty_doc_guards::repo_root()
}

/// `docs/` 아래 파일 순회 하한.
const DOCS_FLOOR: Floor = Floor {
    min: 338,
    // 좌변의 사실은 `populations::DOCS_MD` 하나가 갖는다 — 같은 모수를 재는 자리가
    // `no_checkbox_in_docs` 에 하나 더 있고, 두 값이 200 과 380 으로 갈려 있었다.
    measured: tasty_doc_guards::floored_walk::populations::DOCS_MD.measured,
    measured_on: tasty_doc_guards::floored_walk::populations::DOCS_MD.measured_on,
    counted_on: tasty_doc_guards::floored_walk::populations::DOCS_MD.counted_on,
    why_this_gap: "실측(`8bdbf1bdb` 직전 1215 커밋): 이 모수는 354..402 로 \
                   움직였고 **감소가 한 번도 없었다** — 47 개 커밋에서 다 늘기만 했고 최대 \
                   증가가 2 다. 감소 진폭이 관측되지 않았으므로 '진폭 × 몇 배' 를 쓸 근거가 \
                   없고, 대신 아직 안 일어난 사건의 크기에 건다: 문서 카테고리 하나가 접히면 그 \
                   아래 `.md` 가 통째로 빠지고, 지금 트리에서 그 크기의 최대는 `docs/features` \
                   의 64 다. 여유 64 = 사건 하나. `docs/adr` 211 은 곱수에 안 넣는다 — 그것이 \
                   접히는 것은 카테고리 하나가 접히는 사건이 아니라 결정 기록 방식이 바뀌는 \
                   일이고, 그때 할 일은 하한을 견디는 것이 아니라 이 수를 다시 재는 것이다. 이 \
                   가드는 인덱스에서 못 닿는 카테고리를 찾으므로, 순회가 절반만 모으면 **닿을 \
                   것이 적어져** 위반 0 으로 초록이 된다 — 하한이 그 부분 사망을 잡는 자리다. \
                   앞선 판은 폭 102 를 아무 수에서도 안 뽑았다",
};

/// `docs/` 바로 아래의 카테고리 이름 — **파일이 있는 디렉토리**로 도출한다.
///
/// 디렉토리를 직접 열거하지 않는 이유가 둘이다. 하나는 공용 순회를 쓰기 위해서고
/// (통합 테스트 타깃의 직접 `read_dir` 은 상한 래칫이 붙들고 있다), 하나는 **빈
/// 디렉토리를 세지 않기 위해서**다 — 문서가 하나도 없는 디렉토리는 "시작 전" 절차가
/// 찾아갈 것이 없으므로 이 축의 대상이 아니다.
fn categories(root: &Path) -> BTreeSet<String> {
    categories_of(&docs_files(root, root, &DOCS_FLOOR))
}

/// `docs/` 아래 `.md` 를 모은다 — **순회 뿌리와 하한을 인자로 받는다.**
///
/// 뿌리를 인자로 받는 것이 요점이다. 하한과 뿌리는 모수의 성질이지 이 파일의 성질이
/// 아니고, 상수로 박아 두면 이 순회를 레포 말고 다른 트리에 태울 방법이 없다.
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

/// 모인 파일에서 카테고리 이름을 뽑는다 — **디스크를 안 본다.**
///
/// `docs/` 바로 아래 디렉토리를 `read_dir` 로 직접 열지 않는 이유가 둘이다. 하나는
/// 공용 순회를 쓰기 위해서고(통합 테스트 타깃의 직접 `read_dir` 은 상한 래칫이 붙들고
/// 있다), 하나는 **빈 디렉토리를 세지 않기 위해서**다 — 문서가 하나도 없는 디렉토리는
/// "시작 전" 절차가 찾아갈 것이 없으므로 이 축의 대상이 아니다.
fn categories_of(files: &[Walked]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for w in files {
        // 접두를 **여기서** 본다. 순회의 거르개가 이미 봤더라도 이 함수는 혼자서도
        // 말이 되어야 한다 — 안 그러면 `zone/outside/x.md` 가 카테고리 `zone` 을
        // 낳고, 그 값을 낳은 술어는 이 함수 밖에 있다.
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

/// 색인이 그 카테고리로 **내려가는 링크**를 갖는가.
///
/// 링크 형태 둘을 다 인정한다: `](<cat>/…)` 와 `docs/<cat>/…`.
fn index_links_to(index: &str, cat: &str) -> bool {
    index.contains(&format!("]({cat}/")) || index.contains(&format!("docs/{cat}/"))
}

/// 뿌리 색인이 안 가리키는 카테고리.
fn unlinked<'a>(cats: &'a BTreeSet<String>, index: &str) -> Vec<&'a String> {
    cats.iter().filter(|c| !index_links_to(index, c)).collect()
}

/// 자기 색인(`<docs_dir>/<cat>/index.md`)이 없는 카테고리 — **`docs/` 디렉토리를 인자로 받는다.**
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
        "`docs/` 아래 카테고리를 {} 개만 찾았다 (2026-09-07 실측 9) — 걷기가 깨졌다. \
         모수가 비면 아래 대조는 '빠진 것 없음' 으로 공짜 통과한다",
        cats.len()
    );
    let missing = without_own_index(&cats, &root.join("docs"));
    assert!(
        missing.is_empty(),
        "카테고리에 `index.md` 가 없다: {missing:?}\n\
         `CLAUDE.md` 의 \"시작 전\" 은 `docs/index.md` 에서 **해당 영역의 가이드**로 \
         내려가라고 한다. 그 영역에 색인이 없으면 그 걸음이 갈 곳이 없다 — 절차를 \
         **정확히 지킨 사람이** 그 문서를 못 찾고, 못 찾은 사람은 자기가 절차를 어겼다고 \
         생각하지 않는다."
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
        "`{ROOT_INDEX}` 가 이 카테고리를 가리키지 않는다: {missing:?}\n\
         새 카테고리를 색인에 안 넣으면 \"시작 전\" 절차를 **정확히 지킨 사람이** 그 문서를 \
         못 찾는다. 그리고 못 찾은 사람은 자기가 절차를 어겼다고 생각하지 않는다 — 색인에 \
         없으니 없는 줄 안다.\n\
         ★ 이 목록에서 카테고리를 빼서 통과시키지 마라. 그건 문서를 지우는 것이 아니라 \
         **찾는 길만 지우는 것**이라 더 조용해진다."
    );
}

/// 술어의 극성 — 무엇을 "가리킨다" 로 세는가.
///
/// 이 픽스처가 없으면 위 대조는 색인이 카테고리 이름을 **산문으로만** 언급해도 통과한다.
/// 링크 형태를 요구하는 것이 이 축의 요지다 — "시작 전" 이 시키는 것은 읽는 것이 아니라
/// **따라 내려가는 것**이다.
///
/// 술어를 여기 베껴 적지 않고 `index_links_to` 를 부른다. 베낀 사본을 시험하면 본체가
/// 느슨해져도 이 시험은 초록이다 — 실제로 그랬다.
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

/// 합성 명부를 짓는다 — **디스크를 안 만진다.**
///
/// `Walked` 는 순회가 낸 것이든 손으로 지은 것이든 같은 두 칸이다. 이 축이 묻는 것은
/// 파일이 실재하는가가 아니라 **이름이 어떻게 갈리는가**라, 임시 디렉토리를 팔 이유가
/// 없다. 그러면 임시 경로 유일성 가드도, 순회 하한도 이 픽스처에 안 걸린다.
fn roster<S: AsRef<str>>(rels: &[S]) -> Vec<Walked> {
    rels.iter()
        .map(|r| Walked {
            path: PathBuf::from(r.as_ref()),
            rel: r.as_ref().to_string(),
        })
        .collect()
}

/// 이름 도출의 세 갈래 — 어디까지가 카테고리인가.
///
/// 레포에는 이 셋 중 첫째(정상)밖에 없다. 나머지 둘은 순회가 낼 수 있는 형태인데
/// 레포에 실물이 없어서 **판정이 한 번도 안 태워진 채로** 있었다.
#[test]
fn the_derivation_takes_the_first_segment_and_drops_the_rootless_forms() {
    let cats = categories_of(&roster(&[
        // 정상: 첫 마디가 카테고리다. 더 깊어도 첫 마디만 센다.
        "docs/zone-alpha/index.md",
        "docs/zone-alpha/deep/deeper/note.md",
        "docs/zone-beta/one.md",
        // `docs/` 바로 밑의 파일은 카테고리가 아니다 — 내려갈 곳이 없다.
        "docs/index.md",
        // 마디는 갈리는데 뒤가 비었다. 디렉토리 자체를 카테고리로 세면 안 된다.
        "docs/zone-ghost/",
        // `docs/` 밖. 이 축의 대상이 아니다.
        "zone/outside/x.md",
    ]));

    let got: Vec<&str> = cats.iter().map(String::as_str).collect();
    assert_eq!(
        got,
        vec!["zone-alpha", "zone-beta"],
        "첫 마디만, 그리고 뒤가 있는 것만 카테고리로 세야 한다"
    );
}

/// 두 보고 갈래에 처음으로 위반을 넣는다.
///
/// 레포는 초록이라 이 둘은 **늘 빈 목록을 반환한 채로** 통과해 왔다. 여기서 두 위반을
/// 심어 목록이 실제로 채워지는지, 그리고 초록인 카테고리는 안 섞이는지를 본다.
#[test]
fn the_two_reports_name_the_offender_and_leave_the_rest_alone() {
    // 대조군은 **실재로 색인을 가진** 카테고리여야 한다. 셋 다 합성이면 셋 다
    // 위반이 되어 "초록인 것은 안 섞인다" 는 절반이 안 태워진다. `dev-guide` 가
    // 색인을 잃으면 위 `every_category_has_its_own_index` 가 먼저 빨개지므로,
    // 이 의존은 조용히 낡지 않는다.
    let control = "dev-guide";
    let cats = categories_of(&roster(&[
        &format!("docs/{control}/build.md"),
        "docs/zone-unlinked/note.md",
        "docs/zone-mentioned/note.md",
    ]));

    // 색인은 하나만 링크로 가리키고, 하나는 산문으로만 언급하고, 하나는 아예 없다.
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
