//! 레포 파일을 훑는 타깃이 **어떤 수단으로 훑는지**를 한 곳에서 선언한다.
//!
//! "이 파일을 판정하는 것이 무엇인가" 는 새것을 만들 때마다 나오는 물음이고, 지금
//! 그 답을 아는 유일한 방법은 다 돌려 보는 것이다. 정적으로 답하려는 시도는
//! 순회 **범위**를 소스에서 읽어야 하는데 그것은 임의의 코드라 언제나 근사가 된다
//! (2026-09-06 실측: 순회 시작점을 기계적으로 뽑으니 36 중 17 이 안 읽혔고, 확장자
//! 필터를 뽑으니 디렉토리 이름을 확장자로 세었다).
//!
//! 범위는 못 읽어도 **"훑는가" 는 정확히 셀 수 있다** — 훑으려면 순회 API 를 불러야
//! 하기 때문이다. 그 셈이 성립하는 조건이 하나 있다: **수단이 알려진 것뿐이어야
//! 한다.** 다른 수단이 조용히 들어오면 "무엇이 새 파일을 볼 수 있는가" 를 세는 모든
//! 도구가 그날부터 덜 세고, 덜 세는 쪽은 초록으로 보인다.
//!
//! 그래서 이 가드는 대체 수단을 **금지하지 않는다.** 들어오면 여기 선언하라고 요구할
//! 뿐이다 — 선언이 늘어나는 것은 정상이고, 선언 없이 늘어나는 것만 막는다.

/// 오늘 쓰이는 순회 수단. 다른 것이 들어오면 여기에 한 줄을 더한다.
const KNOWN_MECHANISMS: &[(&str, &str)] = &[(
    "std::fs::read_dir",
    "표준 라이브러리 재귀 순회 — 2026-09-06 기준 유일한 수단",
)];

/// 선언 없이 들어올 수 있는 대체 수단들. 발견되면 위 목록에 추가하라는 뜻이지
/// 쓰지 말라는 뜻이 아니다.
const UNDECLARED_MECHANISMS: &[&str] = &[
    "walkdir",
    "jwalk",
    "glob::glob",
    "include_dir",
    "ignore::Walk",
];

/// 검사한 타깃 수의 하한 — 모수가 비면 "다른 수단 없음" 은 언제나 참이다.
/// 값의 근거: 2026-09-06 실측 99(루트 52 + 크레이트 47).
const MIN_TARGETS: usize = 70;

/// 소스에 선언되지 않은 순회 수단이 **코드로** 등장하는지. 주석 안의 언급은 세지
/// 않는다 — 이 판정의 물음은 "그 수단을 쓰는가" 이지 "그 낱말이 있는가" 가 아니다.
fn undeclared_mechanisms_in(src: &str) -> Vec<&'static str> {
    let code = tasty_doc_guards::strip_line_comments(src);
    UNDECLARED_MECHANISMS
        .iter()
        .filter(|needle| code.contains(**needle))
        .copied()
        .collect()
}

/// 통합 테스트 타깃 전부 — 루트 패키지와 각 크레이트의 `tests/`.
fn integration_test_targets(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut dirs = vec![root.join("tests")];
    if let Ok(entries) = std::fs::read_dir(root.join("crates")) {
        for entry in entries.flatten() {
            dirs.push(entry.path().join("tests"));
        }
    }
    let mut out = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_directory_walk_uses_a_declared_mechanism() {
    assert!(
        !KNOWN_MECHANISMS.is_empty(),
        "선언 목록이 비면 아래 판정은 아무것도 뜻하지 않는다"
    );
    let root = tasty_doc_guards::repo_root();
    let targets = integration_test_targets(&root);

    assert!(
        targets.len() >= MIN_TARGETS,
        "통합 테스트 타깃을 {} 개만 찾았다(하한 {MIN_TARGETS}) — 수집이 죽으면 \
         아래 판정은 빈 집합을 훑고 조용히 통과한다.",
        targets.len()
    );

    // 이 파일 자신은 대체 수단의 이름을 **목록으로** 담는 것이 본질이라 제외한다.
    // 이름이 아니라 자기 참조로 가른다 — 파일이 옮겨져도 따라온다.
    let own = std::path::Path::new(file!())
        .file_name()
        .expect("file!() 에 파일명이 없다");

    let mut findings = Vec::new();
    for path in &targets {
        if path.file_name() == Some(own) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(path) else {
            continue;
        };
        for needle in undeclared_mechanisms_in(&src) {
            let rel = path.strip_prefix(&root).unwrap_or(path);
            findings.push(format!("  {} — {needle}", rel.display()));
        }
    }

    assert!(
        findings.is_empty(),
        "선언되지 않은 순회 수단이 쓰인다:\n{}\n\n\
         쓰지 말라는 뜻이 아니다 — `KNOWN_MECHANISMS` 에 한 줄을 더하라는 뜻이다. \
         \"무엇이 이 파일을 볼 수 있는가\" 를 세는 도구는 순회 수단을 열거해서 센다. \
         선언되지 않은 수단이 있으면 그 셈은 그날부터 덜 세고, 덜 세는 쪽은 초록으로 \
         보인다.",
        findings.join("\n")
    );
}

/// 위 판정이 **무엇이든 잡을 수 있는지** 를 같은 함수로 확인한다.
/// 한 방향만 재면 "위반 0" 과 "판정이 죽었다" 가 구별되지 않는다.
#[test]
fn the_detector_separates_an_undeclared_mechanism_from_the_declared_one() {
    assert_eq!(
        undeclared_mechanisms_in("let w = walkdir::WalkDir::new(p);"),
        vec!["walkdir"],
        "코드에 쓰인 대체 수단을 잡아야 한다"
    );
    assert!(
        undeclared_mechanisms_in("let d = std::fs::read_dir(p);").is_empty(),
        "선언된 수단은 위반이 아니다"
    );
    assert!(
        undeclared_mechanisms_in("// walkdir 은 이 자리에서 쓰지 않는다").is_empty(),
        "주석 안의 언급은 그 수단을 쓰는 것이 아니다"
    );
}
