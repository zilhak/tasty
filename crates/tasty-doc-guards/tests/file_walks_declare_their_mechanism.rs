//! 통합 테스트 소스에 알려진 대체 순회 수단이 나오면 사용 목록에 등록하도록 요구한다.
//! 새 수단의 사용을 금지하지는 않는다. 이 검사는 등록된 검색 문자열만 찾으므로
//! 모든 순회 방법이나 실제 수집 범위를 알아내지는 못한다.

// 이유: 테스트의 반환값 무시는 제품 코드의 lint 예외 명부에 포함하지 않는다.
#![allow(clippy::let_underscore_must_use)]

/// 등록된 순회 수단과 설명.
const KNOWN_MECHANISMS: &[(&str, &str)] = &[(
    "std::fs::read_dir",
    "표준 라이브러리 재귀 순회 — 2026-09-06 기준 유일한 수단",
)];

/// 발견 시 등록을 요구할 대체 수단. 이 목록 밖의 방식은 검출하지 못한다.
const UNDECLARED_MECHANISMS: &[&str] = &[
    "walkdir",
    "jwalk",
    "glob::glob",
    "include_dir",
    "ignore::Walk",
];

/// 통합 테스트 수집의 하한. 2026-09-06 실측 99개(루트 52·크레이트 47)를 기준으로 뒀다.
/// 실제 타깃을 삭제할 때는 두 위치를 각각 확인해 기준을 갱신한다.
/// 여유 안의 일부 누락은 잡지 못하며, 직접 read_dir 호출 수는 별도 스크립트가 검사한다.
const MIN_TARGETS: usize = 70;

/// 줄 주석을 제거한 뒤 대체 수단의 문자열을 찾는다. 호출 여부를 정밀하게 분석하지는 않는다.
fn undeclared_mechanisms_in(src: &str) -> Vec<&'static str> {
    let code = tasty_doc_guards::strip_line_comments(src);
    UNDECLARED_MECHANISMS
        .iter()
        .filter(|needle| code.contains(**needle))
        .copied()
        .collect()
}

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

    println!(
        "[순회 수단 선언] 통합 테스트 타깃 {} · 하한 {MIN_TARGETS}",
        targets.len()
    );
    assert!(
        targets.len() >= MIN_TARGETS,
        "통합 테스트 타깃을 {}개만 수집했다(하한 {MIN_TARGETS}). 실제 파일 수와 수집 범위를 확인한다.",
        targets.len()
    );

    // 검색 목록을 선언한 자기 파일은 제외한다.
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
        "미등록 순회 수단의 문자열을 찾았다:\n{}\n실제 사용을 확인하고 KNOWN_MECHANISMS에 설명을 추가한다. 순회 수단을 세는 다른 도구도 이 방식을 인식하는지 확인한다.",
        findings.join("\n")
    );
}

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

/// 루트와 크레이트 tests/의 수집을 각각 합성 입력으로 확인한다.
#[test]
fn the_target_floor_sees_a_collapsed_collection() {
    let root = std::env::temp_dir().join(format!(
        "tasty-targetfloor-{}-{}",
        std::process::id(),
        line!()
    ));
    // 앞선 실행의 잔여를 치운다 — 없는 것이 정상이라 실패가 정보가 아니다.
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("임시 디렉토리를 못 만들었다");

    assert_eq!(
        integration_test_targets(&root).len(),
        0,
        "뿌리가 비었는데 0 이 아니면 이 수집기는 입력을 안 보는 것이다"
    );

    std::fs::create_dir_all(root.join("tests")).expect("생성 실패");
    std::fs::write(root.join("tests/a.rs"), "").expect("쓰기 실패");
    assert_eq!(
        integration_test_targets(&root).len(),
        1,
        "루트 tests/의 Rust 타깃을 수집하지 못했다"
    );

    std::fs::write(root.join("tests/notes.md"), "").expect("쓰기 실패");
    assert_eq!(
        integration_test_targets(&root).len(),
        1,
        "Rust 파일이 아닌 항목을 통합 타깃으로 셌다"
    );

    std::fs::create_dir_all(root.join("crates/x/tests")).expect("생성 실패");
    std::fs::write(root.join("crates/x/tests/b.rs"), "").expect("쓰기 실패");
    assert_eq!(
        integration_test_targets(&root).len(),
        2,
        "크레이트 tests/의 Rust 타깃을 수집하지 못했다"
    );

    // 뒷정리 실패는 무시한다 — 임시 디렉토리라 남아도 다음 실행이 먼저 지우고, 여기서
    // 죽으면 위 단정의 결과가 정리 오류에 가린다.
    let _ = std::fs::remove_dir_all(&root);
}
