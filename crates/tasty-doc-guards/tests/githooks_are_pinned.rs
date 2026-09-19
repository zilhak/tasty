//! `.githooks/` 의 훅 파일을 **이름으로** 못 박는다 — 수가 아니라 이름이다.
//!
//! ## 왜 이 자리가 필요했나
//!
//! 훅 셋(`pre-commit` · `pre-merge-commit` · `pre-push`)은 이 레포에서 **커밋 전·push 전
//! 유일한 로컬 채널**이다. 그런데 그 파일들은 어떤 가드의 좌변에도 없었다. 실측: 훅 하나를
//! 디렉토리 밖으로 치우고 `-p tasty-doc-guards` 전체를 돌려도 한 건도 안 죽었다.
//!
//! 이 디렉토리를 좌변에 넣은 가드가 없는 것은 아니었지만 **성질이 다르다** — 파이프의 조기
//! 종료 소비자를 보는 가드는 파일이 없으면 볼 것이 없어 조용히 통과하고, 훅 모수 가드는
//! `pre-commit` **한 파일**만 이름으로 본다. 경로 인용을 검사하는 가드도 이 디렉토리를
//! 구조적으로 못 본다(그 가드의 뿌리 접두 명부에 `.githooks/` 가 없다).
//!
//! ## 왜 수가 아니라 이름인가
//!
//! 수는 **자리바꿈을 못 본다.** 훅 하나가 사라지고 다른 하나가 생기면 수가 그대로다.
//! 같은 판단을 [`automatic_job_roster_is_pinned`] 이 잡 명부에 대해 먼저 했다 — 이 파일은
//! 그 형태를 훅 디렉토리에 적용한 것이다.
//!
//! ## 이 파일이 답하지 않는 것
//!
//! **훅이 설치돼 있는가는 안 묻는다.** 그것은 `core.hooksPath` 가 가리키는 값이고 개발
//! 기계의 상태라, CI 에서 물으면 언제나 같은 답이 나와 뜻이 없다. 여기서 묻는 것은 **레포가
//! 그 파일들을 들고 있는가**다.
//!
//! [`automatic_job_roster_is_pinned`]: ../automatic_job_roster_is_pinned/index.html

use std::collections::BTreeSet;

use tasty_doc_guards::floored_walk::{Descend, Floor, walk_with_floor};
use tasty_doc_guards::repo_root;

/// 레포가 들고 있어야 하는 훅 파일. 실측 2026-09-20.
///
/// **이 명부에 이름을 더하는 것은 훅을 하나 더 놓는 일과 같다** — 더하기만 하고 파일을
/// 안 만들면 아래 첫 시험이 빨개지고, 파일만 만들고 명부에 안 넣으면 둘째가 빨개진다.
const REQUIRED_HOOKS: &[&str] = &["pre-commit", "pre-merge-commit", "pre-push"];

/// 순회가 죽은 것과 파일이 사라진 것을 가른다.
///
/// 하한이 0 과 1 만 가르는 것으로 족한 이유는 위 명부가 이미 이름까지 들고 있기 때문이다 —
/// 파일 하나가 빠지면 차집합이 그 이름을 찍는다. 하한이 낼 수 있는 어떤 수보다 구체적이다.
const LIVENESS: Floor = Floor {
    min: 1,
    measured: 3,
    measured_on: "2026-09-20",
    counted_on: tasty_doc_guards::floored_walk::CountedOn::Tree(
        "742b0dbf7 — `git ls-files .githooks/` 가 셋이고 순회도 셋이다. \
         이 디렉토리는 평평하고(하위 디렉토리 0) 확장자가 없다.",
    ),
    why_this_gap: "소실은 하한이 아니라 아래 명부 대조가 잡는다. 하한은 '디렉토리를 \
                   통째로 못 읽었다' 하나만 가른다 — 그때는 차집합이 셋 다를 찍어 \
                   실패문이 소실과 구별되지 않는다.",
};

/// 훅 디렉토리에 실제로 있는 파일 이름.
fn hooks_on_disk() -> BTreeSet<String> {
    let dir = repo_root().join(".githooks");
    assert!(
        dir.is_dir(),
        "`.githooks/` 가 디렉토리가 아니다 — 훅이 통째로 사라졌다. \
         이 줄이 없으면 아래 순회가 빈손으로 돌아와 '파일 셋이 다 사라졌다' 와 \
         '디렉토리를 못 읽었다' 가 같은 실패문이 된다."
    );

    walk_with_floor(&dir, &dir, &LIVENESS, Descend::Everything, &|_| true)
        .unwrap_or_else(|why| panic!("{why}"))
        .into_iter()
        .map(|w| w.rel)
        .collect()
}

#[test]
fn every_named_hook_is_still_in_the_tree() {
    let on_disk = hooks_on_disk();
    let missing: Vec<&str> = REQUIRED_HOOKS
        .iter()
        .copied()
        .filter(|name| !on_disk.contains(*name))
        .collect();

    assert!(
        missing.is_empty(),
        "`.githooks/` 에서 사라진 훅: {missing:?}\n\
         이 셋은 커밋 전·push 전 유일한 로컬 채널이다. 지우기로 한 것이면 \
         이 파일의 명부와 `docs/dev-guide/git-hooks.md` 를 같은 커밋에서 고쳐라 — \
         명부만 고치고 넘어가면 그 채널이 사라진 것이 아무 자리에도 안 남는다."
    );
}

#[test]
fn the_directory_holds_nothing_the_roster_does_not_name() {
    let named: BTreeSet<&str> = REQUIRED_HOOKS.iter().copied().collect();
    let extra: Vec<String> = hooks_on_disk()
        .into_iter()
        .filter(|name| !named.contains(name.as_str()))
        .collect();

    assert!(
        extra.is_empty(),
        "명부에 없는 훅 파일: {extra:?}\n\
         훅을 새로 놓았으면 이 파일의 명부와 `docs/dev-guide/git-hooks.md` 에 \
         같이 적어라. 명부 밖의 훅은 사라져도 아무것도 안 빨개진다 — \
         이 시험이 없던 동안 셋 전부가 그 상태였다."
    );
}

#[test]
fn each_hook_is_a_bash_script_with_content() {
    for name in REQUIRED_HOOKS {
        let path = repo_root().join(".githooks").join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("훅 `{name}` 을 못 읽었다: {e}"));

        // 빈 파일도 "있다" 로 세어진다. 훅은 내용이 없으면 아무 검사도 안 하면서
        // 초록을 내므로, 존재만 묻는 것은 이 가드가 겨냥한 형태를 절반만 막는다.
        assert!(
            text.lines().count() > 5,
            "훅 `{name}` 이 {} 줄뿐이다 — 내용이 비었으면 그 훅은 있으나 마나다.",
            text.lines().count()
        );
        assert!(
            text.starts_with("#!/usr/bin/env bash"),
            "훅 `{name}` 의 첫 줄이 bash shebang 이 아니다. \
             git 은 이 파일을 직접 실행하므로 shebang 이 곧 실행 셸이다."
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// 스텝 명부 — 훅이 선언한 검사 ID 와 문서의 표가 짝이 맞는가
//
// 파일 존재(위)를 먼저 고정한 뒤에야 이것이 뜻을 가진다. 순서가 반대면 파일이
// 통째로 사라질 때 명부 가드가 **좌변을 파일과 함께 잃는다** — 빈 훅에서 스텝을
// 0 개 읽고, 문서 표도 같이 지워졌다면 0 == 0 으로 통과한다.
// ─────────────────────────────────────────────────────────────────────────

/// 각 훅이 선언하는 검사 ID. 실측 2026-09-20, 트리 `742b0dbf7`.
///
/// **`pre-merge-commit` 은 ID 를 하나도 안 단다.** 그 훅은 검사가 하나뿐이고 그것이
/// 파일 전체라, 안에서 갈래를 가리킬 ID 가 필요 없다. 그 훅이 막는 것에 붙은 ID(`M.1`)는
/// **`pre-commit` 이 구현한다** — 충돌한 merge 는 `pre-merge-commit` 을 안 타고
/// `git commit` 으로 마무리되기 때문이다. 그래서 `M.1` 은 아래에서 `pre-commit` 아래에 있다.
/// 빈 명부를 남겨 두는 것은 "아직 안 적었다" 와 "적을 것이 없다" 를 가르기 위해서다.
const HOOK_STEPS: &[(&str, &[&str])] = &[
    (
        "pre-commit",
        &[
            "A.1", "A.2", "A.3", "C.6", "C.8", "C.9", "C.11", "C.12", "M.1", "P.1", "T.1", "W.1",
            "W.2",
        ],
    ),
    ("pre-merge-commit", &[]),
    (
        "pre-push",
        &["B.4", "B.5", "B.6", "B.7", "B.8", "B.9", "B.10"],
    ),
];

/// 훅 스텝을 **문서와 같은 좌표계로** 적는 문서. 표의 첫 칸이 ID 다.
const HOOK_DOC: &str = "docs/dev-guide/git-hooks.md";

/// 훅 안에서 스텝 ID 를 읽는다 — **구역 배너만** 센다.
///
/// 배너는 박스 괘선 바로 아래 줄이다. 이 표지가 필요한 이유는 같은 ID 가 산문에도
/// 나오기 때문이다(`# B.9 만 초 단위이고 …` 는 실행 순서를 설명하는 문장이지 구역이
/// 아니다). 괘선을 요구하면 둘이 갈린다. 머리말 요약(`#   A.1 …`)은 들여쓰기가 달라
/// 애초에 안 걸린다.
fn banner_ids(text: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    let mut prev = "";
    for line in text.lines() {
        let is_rule = prev.starts_with("# ─") && prev.trim_end().ends_with('─');
        if is_rule && let Some(id) = banner_id_of(line) {
            ids.insert(id);
        }
        prev = line;
    }
    ids
}

/// `# A.1  제목` 에서 `A.1` 을 꺼낸다. 형태가 아니면 `None`.
fn banner_id_of(line: &str) -> Option<String> {
    let rest = line.strip_prefix("# ")?;
    let id: String = rest
        .chars()
        .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '.')
        .collect();
    let (family, number) = id.split_once('.')?;
    let ok = family.len() == 1
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
        && rest[id.len()..].starts_with(|c: char| c.is_whitespace());
    ok.then_some(id)
}

#[test]
fn the_banners_in_each_hook_match_the_roster() {
    for (name, declared) in HOOK_STEPS {
        let path = repo_root().join(".githooks").join(name);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("훅 `{name}` 을 못 읽었다: {e}"));

        let found = banner_ids(&text);
        let want: BTreeSet<String> = declared.iter().map(|s| s.to_string()).collect();

        assert_eq!(
            found,
            want,
            "\n훅 `{name}` 의 구역 배너와 이 파일의 명부가 어긋난다.\n\
             훅에만 있는 것: {:?}\n명부에만 있는 것: {:?}\n\
             스텝을 지웠으면 명부와 `{HOOK_DOC}` 의 표를 같은 커밋에서 고쳐라. \
             스텝을 더했으면 셋 다에 적어라 — 어느 하나만 고치면 나머지가 그 사실을 \
             모르는 채 남는다.",
            found.difference(&want).collect::<Vec<_>>(),
            want.difference(&found).collect::<Vec<_>>(),
        );
    }
}

#[test]
fn the_hook_doc_tables_list_exactly_the_declared_steps() {
    let path = repo_root().join(HOOK_DOC);
    let text =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("`{HOOK_DOC}` 을 못 읽었다: {e}"));

    // 표의 첫 칸이 ID 인 행만 센다. 절이 둘(pre-commit · pre-push)로 나뉘어 있고
    // `M.1` 은 pre-commit 절에 적혀 있으므로, 절 단위가 아니라 **합집합**으로 묻는다 —
    // 어느 절에 적을지는 문서의 편집 판단이고, 이 가드가 물을 것은 "적혔는가" 다.
    let documented: BTreeSet<String> = text
        .lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| ")?.split('|').next()?.trim();
            banner_id_of(&format!("# {cell} "))
        })
        .collect();

    let declared: BTreeSet<String> = HOOK_STEPS
        .iter()
        .flat_map(|(_, ids)| ids.iter().map(|s| s.to_string()))
        .collect();

    assert_eq!(
        documented,
        declared,
        "\n`{HOOK_DOC}` 의 표와 훅 스텝 명부가 어긋난다.\n\
         문서에만 있는 것: {:?}\n훅에만 있는 것: {:?}\n\
         뒤엣것은 **문서가 말하지 않는 검사**다 — 훅을 안 설치한 사람이 무엇을 \
         놓치는지 그 표로 읽으므로, 빠진 줄만큼 그 답이 틀린다.",
        documented.difference(&declared).collect::<Vec<_>>(),
        declared.difference(&documented).collect::<Vec<_>>(),
    );
}

#[test]
fn the_pre_commit_run_list_and_its_definitions_still_agree() {
    let text = std::fs::read_to_string(repo_root().join(".githooks/pre-commit"))
        .unwrap_or_else(|e| panic!("pre-commit 을 못 읽었다: {e}"));

    let defined = text
        .lines()
        .filter(|l| l.starts_with("check_") && l.ends_with("() {"))
        .count();

    let listed = text
        .lines()
        .skip_while(|l| !l.starts_with("CHECKS=("))
        .skip(1)
        .take_while(|l| !l.starts_with(')'))
        .filter(|l| l.trim().starts_with("check_"))
        .count();

    // 훅 자신도 실행 시점에 이 둘을 견준다. 여기서 한 번 더 묻는 이유는 **그 비교가
    // 훅이 돌 때만 일어나기 때문**이다 — 훅을 안 설치했거나 `--no-verify` 로 넘긴
    // 사람의 커밋에서는 그 비교가 아예 없다. 이쪽은 자동 채널이 본다.
    assert!(
        defined > 0 && listed > 0,
        "pre-commit 에서 검사 정의 {defined} 개 · 실행 목록 {listed} 개를 읽었다 — \
         0 이면 파싱이 형태를 놓친 것이지 검사가 없는 것이 아니다. \
         `CHECKS=(` 블록이나 `check_*() {{` 형태가 바뀌었는지 봐라."
    );
    assert_eq!(
        defined, listed,
        "pre-commit 의 검사 정의 {defined} 개와 실행 목록 {listed} 개가 어긋난다. \
         정의만 있으면 그 검사는 **조용히 안 돈다**."
    );

    // 명부가 통째로 비는 방향을 여기서 막는다. 위 두 대조는 **양쪽이 같이 비면**
    // 0 == 0 으로 통과한다 — 명부를 지우고 문서 표를 지우면 그 상태가 된다. 그래서
    // 명부의 크기를 구현에 묶는다: pre-commit 의 검사 함수 하나하나는 ID 를 가지므로
    // 그 훅의 명부는 함수 수보다 작을 수 없다(`M.1` 처럼 함수가 아닌 자리가 있어
    // 같지는 않다).
    let declared_here = HOOK_STEPS
        .iter()
        .find(|(name, _)| *name == "pre-commit")
        .map(|(_, ids)| ids.len())
        .unwrap_or(0);
    assert!(
        declared_here >= defined,
        "pre-commit 이 검사 {defined} 개를 돌리는데 이 파일의 명부에는 {declared_here} 개뿐이다. \
         명부를 비우면 위의 두 대조가 '문서도 비었다' 와 짝이 맞아 조용히 통과한다 — \
         이 줄이 그 방향을 막는다."
    );
}
