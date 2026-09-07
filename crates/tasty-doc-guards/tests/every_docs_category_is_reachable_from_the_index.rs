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
    min: 80,
    measured: 200,
    measured_on: "2026-09-07",
    why_this_gap: "문서 수는 회차마다 늘고 줄어서 실측에 붙이면 무관한 추가·삭제가 이 \
                   가드를 깨운다. 하한이 막는 것은 순회가 죽어 카테고리가 0 이 되는 것 하나다",
};

/// `docs/` 바로 아래의 카테고리 이름 — **파일이 있는 디렉토리**로 도출한다.
///
/// 디렉토리를 직접 열거하지 않는 이유가 둘이다. 하나는 공용 순회를 쓰기 위해서고
/// (통합 테스트 타깃의 직접 `read_dir` 은 상한 래칫이 붙들고 있다), 하나는 **빈
/// 디렉토리를 세지 않기 위해서**다 — 문서가 하나도 없는 디렉토리는 "시작 전" 절차가
/// 찾아갈 것이 없으므로 이 축의 대상이 아니다.
fn categories(root: &Path) -> BTreeSet<String> {
    let files = walk_with_floor(
        root,
        root,
        &DOCS_FLOOR,
        Descend::SkipBuildCaches,
        &|w: &Walked| w.rel.starts_with("docs/") && w.rel.ends_with(".md"),
    )
    .unwrap_or_else(|e| panic!("`docs/` 순회가 실패했다 — {e}"));

    let mut out = BTreeSet::new();
    for w in files {
        let rest = w.rel.trim_start_matches("docs/");
        if let Some((head, tail)) = rest.split_once('/')
            && !tail.is_empty()
        {
            out.insert(head.to_string());
        }
    }
    out
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
    let missing: Vec<&String> = cats
        .iter()
        .filter(|c| !root.join("docs").join(c).join("index.md").is_file())
        .collect();
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
    // 링크 형태 둘을 다 인정한다: `](<cat>/…)` 와 `docs/<cat>/…`.
    let missing: Vec<&String> = cats
        .iter()
        .filter(|c| !index.contains(&format!("]({c}/")) && !index.contains(&format!("docs/{c}/")))
        .collect();
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
#[test]
fn the_predicate_counts_links_not_mentions() {
    let cat = "dev-guide";
    let linked_rel = format!("| 개발 | [가이드]({cat}/index.md) |");
    let linked_abs = format!("자세한 것은 `docs/{cat}/build.md` 를 보라");
    let mentioned = format!("{cat} 은 개발 가이드다");
    let hit = |s: &str| s.contains(&format!("]({cat}/")) || s.contains(&format!("docs/{cat}/"));
    assert!(hit(&linked_rel), "상대 링크를 못 센다");
    assert!(hit(&linked_abs), "레포 경로 형태를 못 센다");
    assert!(!hit(&mentioned), "산문 언급을 링크로 셌다");
}
