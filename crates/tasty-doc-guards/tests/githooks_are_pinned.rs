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
